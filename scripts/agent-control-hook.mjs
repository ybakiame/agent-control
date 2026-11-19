#!/usr/bin/env node
// Agent Control — harness hook for nine coding agents.
//
// One script serves every supported harness; the registered command bakes in the vendor id and the
// event name as argv (`node "<path>" <vendor> <event>`), so the script knows which harness it is
// serving even when a vendor omits `hook_event_name` in stdin. Vendors fall into three I/O shapes:
//
//   claude-shaped (claude, codebuddy, workbuddy, qoder, qoder-cn, codex, kimi) — the universal
//     exit-2 + stderr deny signal; allow exits 0 silently; the briefing is injected via
//     hookSpecificOutput.additionalContext. stdin carries tool_name / tool_input.
//
//   cursor — must always print valid JSON (a permission hook blocks on empty/invalid stdout).
//     preToolUse: allow → {"permission":"allow"}, deny → {"permission":"deny",user_message,
//     agent_message}. sessionStart: briefing → {"additional_context": "..."}.
//
//   antigravity — PreToolUse treats silence (and, since agy 1.4.x, even `{}`) as DENIAL, so the
//     allow / fail-open output must be {"decision":"ask"} to preserve the user's permission policy;
//     deny → {"decision":"deny","reason":"..."}. stdin nests the call under toolCall.{name,args}.
//     PreInvocation has no context-injection channel, so reach there is via the exported GEMINI.md
//     pointer block; the hook only freezes the session and gates.
//
// Two events drive the framework:
//
//   start (SessionStart / sessionStart / PreInvocation) — freezes the constraint snapshot
//     (get-or-create an Agent Control session) and, where the vendor supports it, pushes the project's
//     binding rules into the session context. Calls create_session + get_applicable_rules (+ a
//     best-effort check_export_drift) over the local MCP HTTP endpoint.
//
//   gate (PreToolUse / preToolUse) — the deny-capable enforcement chokepoint. Resolves the same
//     session and calls check_tool_call with the pending tool name/input AND that session_id; if the
//     gate denies (a hard rule's repo gate fails, or a tool_guard matches) it BLOCKS the call in the
//     vendor's deny shape. Passing session_id records the verdict in the evidence chain and earns a
//     mediated hard rule the Enforced tier — so a hook-only deployment still produces verifiable
//     enforcement.
//
//   compact continuity (SessionStart source=compact) — when a vendor re-enters the session after
//     context compaction (Claude family, Qoder/Qoder CN, Codex, Kimi), the start invocation re-injects
//     the binding-rule briefing PLUS the on-disk continuity brief (.agent-control/handoff/latest.md)
//     and the session's live goal/work state, so the post-compaction agent recovers continuity. The
//     brief is read from disk before any daemon call, so it survives an unreachable daemon (and the
//     guard timer). PreCompact itself is observe-only on most vendors (Cursor preCompact exposes only
//     user_message; Antigravity has no compaction event), so no separate hook group is registered —
//     the compact session-start is the documented injection channel.
//
// Fail-open by default: if the daemon is unreachable, times out, or errors, the hook emits the
// vendor's allow output (silent exit 0 for claude-shaped, {"permission":"allow"} for Cursor,
// {"decision":"ask"} for Antigravity) so a session is never blocked by infrastructure. Set
// AGENT_CONTROL_HOOK_FAIL_CLOSED to opt into fail-closed enforcement: a GATE invocation that cannot
// reach a verdict then DENIES (exit 2 / {permission:deny} / {decision:deny}) because compliance could
// not be verified. The start/briefing event always stays fail-open so session init is never blocked.
//
// Register both events with the MCP `register_hooks` tool (or the console's Hooks panel), which
// writes the managed entry into each harness's config in its own schema. Manual copy-ready snippet
// for a claude-shaped harness — adjust the absolute path and append the vendor + event:
//
//   {
//     "hooks": {
//       "SessionStart": [
//         { "hooks": [ { "type": "command", "command": "node \"C:/path/to/agent-control-hook.mjs\" claude SessionStart" } ] }
//       ],
//       "PreToolUse": [
//         { "matcher": "", "hooks": [ { "type": "command", "command": "node \"C:/path/to/agent-control-hook.mjs\" claude PreToolUse" } ] }
//       ]
//     }
//   }
//
// Environment:
//   AGENT_CONTROL_MCP_URL          MCP endpoint (default http://127.0.0.1:9870/mcp)
//   AGENT_CONTROL_AGENT_ID         agent id used for resolution (default: per-vendor, e.g. claude-code)
//   AGENT_CONTROL_HOOK_TIMEOUT_MS  total time budget in ms (default 4000)
//   AGENT_CONTROL_HOOK_DEBUG       if set, log diagnostics to stderr (never blocks the session)
//   AGENT_CONTROL_HOOK_FAIL_CLOSED if set, a gate that cannot reach the daemon denies (default: allow)

import { readFileSync } from "node:fs";
import { join } from "node:path";

const URL = process.env.AGENT_CONTROL_MCP_URL ?? "http://127.0.0.1:9870/mcp";
const TIMEOUT = Number(process.env.AGENT_CONTROL_HOOK_TIMEOUT_MS ?? 4000);
const DEBUG = !!process.env.AGENT_CONTROL_HOOK_DEBUG;
// Opt-in fail-closed: when set, a gate that cannot reach a verdict (daemon down/timeout/error) DENIES
// instead of allowing. Default (unset) keeps the uniform fail-open behavior. Only the gate event fails
// closed; the start/briefing event always stays fail-open so a session can still initialize.
const FAIL_CLOSED = !!process.env.AGENT_CONTROL_HOOK_FAIL_CLOSED;
// On-disk continuity brief for a compaction start; also the guard-timer fallback so a slow daemon
// cannot drop continuity. Empty for every non-compaction invocation.
let compactBrief = "";

// Vendor + event are baked into the registered command (argv). They are authoritative even when a
// harness omits hook_event_name in stdin; the event falls back to stdin only for legacy registrations.
const VENDOR = (process.argv[2] || "claude").toLowerCase();
let EVENT = process.argv[3] || null;

const DEFAULT_AGENT_ID = {
  claude: "claude-code",
  codebuddy: "codebuddy",
  workbuddy: "workbuddy",
  qoder: "qoder",
  "qoder-cn": "qoder-cn",
  codex: "codex",
  cursor: "cursor",
  antigravity: "antigravity",
  kimi: "kimi-code",
};
const AGENT_ID = process.env.AGENT_CONTROL_AGENT_ID ?? DEFAULT_AGENT_ID[VENDOR] ?? VENDOR;

// The three output shapes. Anything unrecognized falls back to the claude-shaped exit-2 contract.
const SHAPE = VENDOR === "cursor" ? "cursor" : VENDOR === "antigravity" ? "antigravity" : "claude";

function isGateEvent(ev) {
  return ev === "PreToolUse" || ev === "preToolUse";
}

// Allow / fail-open output, vendor-aware. Antigravity must emit {"decision":"ask"} on the gate because
// silence (and `{}` since agy 1.4.x) is treated as denial; "ask" preserves the user's permission policy.
function allowResult() {
  if (SHAPE === "cursor") return { code: 0, stdout: JSON.stringify({ permission: "allow" }) };
  if (SHAPE === "antigravity") {
    return { code: 0, stdout: JSON.stringify(isGateEvent(EVENT) ? { decision: "ask" } : {}) };
  }
  return { code: 0 };
}

// Deny output in the vendor's shape. claude-shaped blocks via exit 2 + stderr (fed back to the agent);
// Cursor and Antigravity block via a valid-JSON decision on stdout (exit 0).
function denyResult(reason) {
  const msg = `Agent Control gate denied this tool call. ${reason}`;
  if (SHAPE === "cursor") {
    return {
      code: 0,
      stdout: JSON.stringify({ permission: "deny", user_message: msg, agent_message: msg }),
    };
  }
  if (SHAPE === "antigravity") {
    return { code: 0, stdout: JSON.stringify({ decision: "deny", reason: msg }) };
  }
  return { code: 2, stderr: `${msg}\n` };
}

// The verdict when the gate cannot reach the daemon (unreachable, timeout, or error). Fail-open by
// default; in fail-closed mode a GATE invocation denies because compliance could not be verified. The
// start/briefing event always fails open — there is no tool call to deny, and blocking session init
// would be disruptive. EVENT is resolved from argv at load (or from stdin in main) before this fires.
function unreachableResult() {
  if (FAIL_CLOSED && isGateEvent(EVENT)) {
    return denyResult(
      "compliance could not be verified (Agent Control daemon unreachable; fail-closed mode is enabled).",
    );
  }
  return allowResult();
}

// Briefing injection on the start event, vendor-aware. Antigravity's PreInvocation has no injection
// channel, so it emits {} (reach there is via the exported GEMINI.md pointer block).
function briefingResult(text) {
  if (SHAPE === "cursor") {
    return { code: 0, stdout: JSON.stringify(text ? { additional_context: text } : {}) };
  }
  if (SHAPE === "antigravity") return { code: 0, stdout: JSON.stringify({}) };
  if (!text) return { code: 0 };
  return {
    code: 0,
    stdout: JSON.stringify({
      hookSpecificOutput: { hookEventName: "SessionStart", additionalContext: text },
    }),
  };
}

function debug(msg) {
  if (DEBUG) process.stderr.write(`agent-control-hook: ${msg}\n`);
}

async function readStdin() {
  if (process.stdin.isTTY) return "";
  const chunks = [];
  try {
    for await (const chunk of process.stdin) chunks.push(chunk);
  } catch {
    /* ignore */
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function post(body, headers) {
  return fetch(URL, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(TIMEOUT),
  });
}

// The Streamable HTTP transport answers with either JSON or SSE framing; handle both.
async function parseBody(res) {
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const text = await res.text();
  if (text.trim().startsWith("{")) return JSON.parse(text);
  for (const line of text.split("\n")) {
    if (!line.startsWith("data:")) continue;
    const payload = line.slice(5).trim();
    if (!payload) continue;
    try {
      const msg = JSON.parse(payload);
      if (msg.id !== undefined) return msg;
    } catch {
      /* keep scanning */
    }
  }
  return null;
}

// One MCP handshake per hook invocation; the returned headers carry the session id so several
// tools/call requests reuse it (the gate resolves a session, then gates the call).
async function mcpInit() {
  const headers = {
    "content-type": "application/json",
    accept: "application/json, text/event-stream",
  };
  const initRes = await post(
    {
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "agent-control-hook", version: "1.0" },
      },
    },
    headers,
  );
  const sid = initRes.headers.get("mcp-session-id");
  if (sid) headers["mcp-session-id"] = sid;
  await initRes.text();

  const noteRes = await post(
    { jsonrpc: "2.0", method: "notifications/initialized", params: {} },
    headers,
  );
  await noteRes.text();
  return headers;
}

async function mcpCall(headers, tool, args) {
  const res = await post(
    { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: tool, arguments: args } },
    headers,
  );
  const msg = await parseBody(res);
  if (msg?.error) throw new Error(msg.error.message ?? "mcp error");
  const r = msg?.result;
  if (r?.isError) throw new Error(r.content?.[0]?.text ?? "tool error");
  return r?.structuredContent ?? JSON.parse(r?.content?.[0]?.text ?? "{}");
}

// Resolve this project's Agent Control hook session. create_session is get-or-create keyed by
// (project, agent, task); a fixed task means every hook invocation for this project+agent lands on
// the same session UUID without sharing state between separate processes — an enforcement ledger
// that records each gated call. The start event freezes the snapshot; the gate attaches its verdict.
async function resolveSession(headers, cwd) {
  const data = await mcpCall(headers, "create_session", {
    task: "hook",
    project_path: cwd,
    agent_id: AGENT_ID,
    task_tags: [],
  });
  return typeof data?.session_id === "string" ? data.session_id : null;
}

// Normalize the pending call across vendors: claude-shaped uses tool_name/tool_input; Antigravity
// nests them under toolCall.{name,args}.
function normalizeToolName(p) {
  const n = p?.tool_name ?? p?.toolCall?.name ?? p?.name ?? p?.tool;
  return typeof n === "string" ? n : "";
}

function normalizeToolInput(p) {
  return p?.tool_input ?? p?.toolCall?.args ?? p?.args ?? p?.input ?? {};
}

function renderStaleWarning(report) {
  const stale = Array.isArray(report?.targets) ? report.targets.filter((t) => t.stale) : [];
  const lines = [
    "## Agent Control — native export needs migration",
    "",
    "Some native entry points in this project are missing the current Agent Control pointer block, or still hold a legacy full-text block. The pointer block directs agents to read the live rules in `.agent-control/rules/`; rules resolved live from Agent Control are authoritative for this session regardless.",
    "",
  ];
  for (const t of stale) {
    if (t?.path) lines.push(`- outdated: ${t.path}`);
  }
  lines.push(
    "",
    "Re-run the MCP `export_native` tool (or the console's Export to harnesses button) to write the current pointer block. This hook is read-only and rewrote nothing. This detects a missing/older-format block only — not whether an already-running session's context is stale, and not rule edits (the pointer is static).",
    "",
  );
  return lines.join("\n");
}

// The gate: resolve the session (best-effort) so the verdict is recorded and mediated hard rules earn
// Enforced, then ask check_tool_call about the pending call. On deny, emit the vendor's deny shape. If
// session resolution fails the gate still runs (stateless): enforcement is never lost, only the
// per-session record.
async function handleGate(headers, parsed, cwd) {
  const toolName = normalizeToolName(parsed);
  if (!toolName) return allowResult();
  let sessionId = null;
  try {
    sessionId = await resolveSession(headers, cwd);
  } catch (e) {
    debug(`session resolution skipped: ${e?.message ?? String(e)}`);
  }
  const args = {
    project_path: cwd,
    tool_name: toolName,
    tool_input: normalizeToolInput(parsed),
    agent_id: AGENT_ID,
  };
  if (sessionId) args.session_id = sessionId;
  const data = await mcpCall(headers, "check_tool_call", args);
  if (data?.report?.allowed === false) {
    const reason = data.denial_reason || data.instruction || "Denied by Agent Control hard rules.";
    return denyResult(reason);
  }
  return allowResult();
}

// Read the project-local continuity brief written by export_handoff. Dependency-free and synchronous
// so compaction continuity survives even when the MCP daemon is unreachable.
function readBrief(cwd) {
  try {
    return readFileSync(join(cwd, ".agent-control", "handoff", "latest.md"), "utf8").trim();
  } catch {
    return "";
  }
}

// Render the session's live goal + done/doing/todo work state (from get_session_context) so a
// post-compaction agent inherits current progress, not just the last exported brief.
function renderWorkState(ctx) {
  const s = ctx?.session ?? ctx;
  const goal = typeof s?.goal === "string" ? s.goal.trim() : "";
  const items = Array.isArray(s?.work_items) ? s.work_items : [];
  if (!goal && items.length === 0) return "";
  const lines = ["## Agent Control — live work state", ""];
  if (goal) lines.push(`Goal: ${goal}`, "");
  for (const status of ["doing", "todo", "done"]) {
    for (const it of items.filter((i) => i?.status === status)) {
      lines.push(`- [${status}] ${it?.title ?? ""}`);
    }
  }
  return lines.join("\n");
}

// Compaction continuity (P2). Vendors that re-enter a session after compaction report
// SessionStart source=compact (Claude family, Qoder/Qoder CN, Codex, Kimi). PreCompact itself is
// observe-only on most of them (Cursor preCompact exposes only user_message; Antigravity has no
// compaction event), so the documented injection channel is the compact session-start. Re-inject the
// binding-rule briefing + the on-disk continuity brief + live work state. The brief is read from disk
// first, so continuity is preserved even when the daemon is down.
async function handleCompact(cwd, brief) {
  let briefing = "";
  let workState = "";
  try {
    const headers = await mcpInit();
    let sessionId = null;
    try {
      sessionId = await resolveSession(headers, cwd);
    } catch (e) {
      debug(`compact session resolution skipped: ${e?.message ?? String(e)}`);
    }
    const data = await mcpCall(headers, "get_applicable_rules", {
      project_path: cwd,
      agent_id: AGENT_ID,
      file_paths: [],
      languages: [],
      task_tags: [],
    });
    briefing = typeof data?.briefing === "string" ? data.briefing : "";
    if (sessionId) {
      const ctx = await mcpCall(headers, "get_session_context", {
        session_id: sessionId,
        agent_id: AGENT_ID,
      });
      workState = renderWorkState(ctx);
    }
  } catch (e) {
    debug(`compact daemon lookup skipped: ${e?.message ?? String(e)}`);
  }
  const text = [briefing, brief, workState].filter(Boolean).join("\n\n");
  if (!text) return allowResult();
  return briefingResult(text);
}

async function main() {
  const input = await readStdin();
  let parsed = null;
  try {
    parsed = JSON.parse(input);
  } catch {
    /* no/invalid stdin: fall back to defaults */
  }
  // argv event is authoritative; fall back to stdin for legacy registrations without argv.
  if (!EVENT) EVENT = parsed?.hook_event_name || parsed?.event || null;
  const cwd =
    parsed?.cwd ||
    parsed?.workspace ||
    (Array.isArray(parsed?.workspace_roots) ? parsed.workspace_roots[0] : null) ||
    process.cwd();

  // Compaction continuity: react to the compact session-start source before any daemon dependency.
  if (parsed?.source === "compact" && SHAPE !== "antigravity") {
    compactBrief = readBrief(cwd);
    return handleCompact(cwd, compactBrief);
  }

  const headers = await mcpInit();

  if (isGateEvent(EVENT)) return handleGate(headers, parsed, cwd);

  // Start event: freeze the snapshot, then inject the binding-rule briefing where the vendor supports it.
  try {
    await resolveSession(headers, cwd);
  } catch (e) {
    debug(`session creation skipped: ${e?.message ?? String(e)}`);
  }
  // Antigravity's PreInvocation cannot inject context; the session is frozen for the gate ledger only.
  if (SHAPE === "antigravity") return briefingResult("");

  const data = await mcpCall(headers, "get_applicable_rules", {
    project_path: cwd,
    agent_id: AGENT_ID,
    file_paths: [],
    languages: [],
    task_tags: [],
  });
  // The briefing is rendered by core (render_constraint_briefing) and returned by get_applicable_rules,
  // so the hook, the MCP constraint-briefing prompt and any other reach path present identical text.
  let briefing = typeof data?.briefing === "string" ? data.briefing : "";

  // Best-effort format-drift check (layer 1: missing/legacy pointer block). Never blocks the session.
  try {
    const drift = await mcpCall(headers, "check_export_drift", {
      project_path: cwd,
      targets: ["agents", "claude", "gemini", "cursor"],
      include_user: false,
    });
    if (drift?.report?.stale) {
      const warning = renderStaleWarning(drift.report);
      briefing = briefing ? `${briefing}\n${warning}` : warning;
    }
  } catch (e) {
    debug(`drift check skipped: ${e?.message ?? String(e)}`);
  }

  return briefingResult(briefing);
}

// Flush both streams before exiting so a deny reason is never truncated. The guard timer is cleared
// so it cannot override the verdict.
function finish(code, stdout, stderr) {
  clearTimeout(guard);
  let pending = 0;
  const done = () => {
    if (--pending <= 0) process.exit(code);
  };
  if (stderr) {
    pending++;
    process.stderr.write(stderr, done);
  }
  if (stdout) {
    pending++;
    process.stdout.write(stdout, done);
  }
  if (pending === 0) process.exit(code);
}

// Hard deadline: a missing/slow daemon must never hang the hook. Emit the vendor's verdict-shaped
// output (not a bare exit 0) so Cursor/Antigravity still receive a valid permission decision on
// timeout — allow by default, or deny when fail-closed is enabled and this is a gate invocation.
// A compaction start still injects the on-disk brief on timeout so continuity is not lost.
const guard = setTimeout(() => {
  const r = compactBrief ? briefingResult(compactBrief) : unreachableResult();
  finish(r.code, r.stdout, r.stderr);
}, TIMEOUT);

main()
  .then((result) => finish(result?.code ?? 0, result?.stdout, result?.stderr))
  .catch((e) => {
    // An unreachable/slow daemon or any error: fail-open by default, fail-closed (deny) only for a
    // gate invocation when explicitly opted in. The start event never blocks session init.
    debug(e?.message ?? String(e));
    const r = unreachableResult();
    finish(r.code, r.stdout, r.stderr);
  });