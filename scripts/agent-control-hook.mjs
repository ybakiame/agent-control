#!/usr/bin/env node
// Agent Control — harness hook for Claude Code / CodeBuddy / WorkBuddy / Qoder.
//
// Dispatches on the stdin `hook_event_name`:
//
//   SessionStart — pushes the project's binding rules into the session context, so the harness
//     inherits the contracts before it calls any MCP tool. Read-only: calls get_applicable_rules
//     (+ a best-effort check_export_drift) over the local MCP HTTP endpoint and injects a concise
//     briefing via hookSpecificOutput.additionalContext. Never writes files.
//
//   PreToolUse — the deny-capable enforcement chokepoint. Calls check_tool_call with the pending
//     tool_name/tool_input; if the gate denies (a hard rule's repo gate fails, or a tool_guard
//     matches), it BLOCKS the call by exiting 2 with the denial reason on stderr (the universal
//     deny signal across these harnesses — stderr is fed back to the agent). On allow it exits 0.
//     This is what turns a declared hard rule into an Enforced one.
//
// Fail-open by design: if the Agent Control daemon is unreachable, times out, or errors, the hook
// exits 0 silently so a session is never blocked and a tool is never denied by infrastructure.
//
// Register both events with the MCP `register_hooks` tool (or the console's Hooks panel), which
// writes the managed group into each harness's settings.json. Manual copy-ready snippet — adjust
// the absolute path to this script:
//
//   {
//     "hooks": {
//       "SessionStart": [
//         { "hooks": [ { "type": "command", "command": "node \"C:/path/to/agent-control-hook.mjs\"" } ] }
//       ],
//       "PreToolUse": [
//         { "matcher": "", "hooks": [ { "type": "command", "command": "node \"C:/path/to/agent-control-hook.mjs\"" } ] }
//       ]
//     }
//   }
//
// Environment:
//   AGENT_CONTROL_MCP_URL          MCP endpoint (default http://127.0.0.1:9870/mcp)
//   AGENT_CONTROL_AGENT_ID         agent id used for resolution (default claude-code)
//   AGENT_CONTROL_HOOK_TIMEOUT_MS  total time budget in ms (default 4000)
//   AGENT_CONTROL_HOOK_DEBUG       if set, log diagnostics to stderr (never blocks the session)

const URL = process.env.AGENT_CONTROL_MCP_URL ?? "http://127.0.0.1:9870/mcp";
const AGENT_ID = process.env.AGENT_CONTROL_AGENT_ID ?? "claude-code";
const TIMEOUT = Number(process.env.AGENT_CONTROL_HOOK_TIMEOUT_MS ?? 4000);
const DEBUG = !!process.env.AGENT_CONTROL_HOOK_DEBUG;

// Hard deadline: a missing/slow daemon must never hang session start.
const guard = setTimeout(() => process.exit(0), TIMEOUT);

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

async function callMcp(tool, args) {
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

// PreToolUse: the deny-capable chokepoint. Ask the gate about the pending call; on deny, exit 2
// with the reason on stderr (the universal block signal — the harness feeds stderr to the agent).
// No session_id is passed: the harness's stdin session id is not an Agent Control session UUID, and
// the gate evaluates the project's enabled hard rules regardless of session.
async function handlePreToolUse(parsed, cwd) {
  const toolName = typeof parsed?.tool_name === "string" ? parsed.tool_name : "";
  if (!toolName) return { code: 0 };
  const data = await callMcp("check_tool_call", {
    project_path: cwd,
    tool_name: toolName,
    tool_input: parsed?.tool_input ?? {},
    agent_id: AGENT_ID,
  });
  if (data?.report?.allowed === false) {
    const reason = data.denial_reason || data.instruction || "Denied by Agent Control hard rules.";
    return { code: 2, stderr: `Agent Control gate denied this tool call. ${reason}\n` };
  }
  return { code: 0 };
}

async function main() {
  const input = await readStdin();
  let parsed = null;
  try {
    parsed = JSON.parse(input);
  } catch {
    /* no/invalid stdin: fall back to defaults */
  }
  const cwd = parsed?.cwd || process.cwd();
  const event = parsed?.hook_event_name;

  if (event === "PreToolUse") return handlePreToolUse(parsed, cwd);

  // SessionStart (and any unrecognized event): inject the binding-rule briefing.
  const data = await callMcp("get_applicable_rules", {
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
  // Check every entry point read-only so a legacy block in any of them is surfaced.
  try {
    const drift = await callMcp("check_export_drift", {
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

  if (!briefing) return { code: 0 };
  return {
    code: 0,
    stdout: JSON.stringify({
      hookSpecificOutput: { hookEventName: "SessionStart", additionalContext: briefing },
    }),
  };
}

// Flush both streams before exiting so a deny reason on stderr is never truncated. Exit code 2
// blocks the tool call; 0 allows. The guard timer is cleared so it cannot override the verdict.
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

main()
  .then((result) => finish(result?.code ?? 0, result?.stdout, result?.stderr))
  .catch((e) => {
    // Fail-open: an unreachable/slow daemon or any error must never block a session or deny a tool.
    clearTimeout(guard);
    debug(e?.message ?? String(e));
    process.exit(0);
  });