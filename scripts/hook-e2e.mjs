// End-to-end test for scripts/agent-control-hook.mjs: spawns the MCP HTTP daemon on a random port
// against a throwaway data dir, then runs the hook as a one-shot child for each event and asserts
// the real exit-code contract (PreToolUse deny => exit 2 + stderr reason; allow => exit 0;
// SessionStart => exit 0 + briefing JSON; unreachable daemon => fail-open exit 0).
// Usage: node scripts/hook-e2e.mjs [path-to-agent-control-mcp binary]
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import net from "node:net";

const bin = resolve(process.argv[2] ?? "target/debug/agent-control-mcp.exe");
const hook = resolve(import.meta.dirname, "agent-control-hook.mjs");
const root = mkdtempSync(join(tmpdir(), "ac-hook-e2e-"));

const port = await new Promise((res) => {
  const srv = net.createServer();
  srv.listen(0, "127.0.0.1", () => {
    const p = srv.address().port;
    srv.close(() => res(p));
  });
});
const url = `http://127.0.0.1:${port}/mcp`;
const child = spawn(bin, ["--bind", `127.0.0.1:${port}`, "--data-dir", root], {
  stdio: ["ignore", "ignore", "inherit"],
});

let sessionId = null;
let nextId = 1;
async function rpc(method, params, isNotification = false) {
  const body = { jsonrpc: "2.0", method, params };
  if (!isNotification) body.id = nextId++;
  const res = await fetch(url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
      ...(sessionId ? { "mcp-session-id": sessionId } : {}),
    },
    body: JSON.stringify(body),
  });
  const sid = res.headers.get("mcp-session-id");
  if (sid) sessionId = sid;
  if (isNotification) return null;
  const text = await res.text();
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${text.slice(0, 300)}`);
  if (text.trim().startsWith("{")) return JSON.parse(text);
  for (const line of text.split("\n")) {
    if (line.startsWith("data:")) {
      const payload = line.slice(5).trim();
      if (!payload) continue;
      const msg = JSON.parse(payload);
      if (msg.id === body.id) return msg;
    }
  }
  throw new Error(`no JSON-RPC response in: ${text.slice(0, 300)}`);
}
async function tool(name, args) {
  const r = await rpc("tools/call", { name, arguments: args });
  if (r.error) throw new Error(`${name}: ${JSON.stringify(r.error)}`);
  if (r.result?.isError) throw new Error(`${name}: ${r.result?.content?.[0]?.text}`);
  return r.result?.structuredContent ?? JSON.parse(r.result?.content?.[0]?.text ?? "{}");
}

let failures = 0;
function check(name, cond, detail) {
  if (cond) console.log(`  ok  ${name}`);
  else {
    failures++;
    console.error(`FAIL  ${name}`, detail !== undefined ? JSON.stringify(detail).slice(0, 300) : "");
  }
}

// Run the hook as a one-shot child with the given stdin payload and MCP endpoint. `argv` bakes in
// the vendor + event (the registered command shape); when omitted the script defaults to claude and
// falls back to stdin hook_event_name. `agentId` overrides the per-vendor default agent id.
// `failClosed` opts into fail-closed enforcement (a gate that cannot reach the daemon denies).
function runHook(payload, endpoint = url, argv = [], agentId = null, failClosed = false) {
  const env = {
    ...process.env,
    AGENT_CONTROL_MCP_URL: endpoint,
    AGENT_CONTROL_HOOK_TIMEOUT_MS: "8000",
  };
  if (agentId) env.AGENT_CONTROL_AGENT_ID = agentId;
  else delete env.AGENT_CONTROL_AGENT_ID;
  if (failClosed) env.AGENT_CONTROL_HOOK_FAIL_CLOSED = "1";
  else delete env.AGENT_CONTROL_HOOK_FAIL_CLOSED;
  const res = spawnSync(process.execPath, [hook, ...argv], {
    input: payload === null ? "" : JSON.stringify(payload),
    env,
    encoding: "utf8",
    timeout: 15000,
  });
  return { code: res.status, stdout: res.stdout ?? "", stderr: res.stderr ?? "" };
}

function parseOut(res) {
  try {
    return JSON.parse(res.stdout);
  } catch {
    return null;
  }
}

try {
  for (let i = 0; i < 50; i++) {
    try {
      await fetch(url, { method: "GET" });
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  await rpc("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "hook-e2e", version: "1.0" },
  });
  await rpc("notifications/initialized", {}, true);

  // A throwaway project with a hard rule: a passing repo gate + a tool_guard banning rm -rf.
  const proj = join(root, "project");
  mkdirSync(join(proj, ".agent-control", "rules"), { recursive: true });
  writeFileSync(join(proj, ".editorconfig"), "[*]\nindent_style = space\n");
  await tool("save_rule", {
    id: "no-rm-rf", title: "No recursive force delete", content: "Never run rm -rf.",
    agent_id: "claude-code", kind: "constraint", scope: "project", project_path: proj,
    enforcement: "hard",
    code_checks: [{ file: ".editorconfig", key: "indent_style", expect: "space" }],
    tool_guards: [{ tool: "Bash", pattern: "rm\\s+-rf", reason: "recursive force delete is banned" }],
  });

  // PreToolUse: a matching command is DENIED via exit 2 with the reason on stderr.
  const denied = runHook({
    hook_event_name: "PreToolUse", cwd: proj, tool_name: "Bash",
    tool_input: { command: "rm -rf /tmp/x" }, session_id: "harness-session-not-a-uuid",
  });
  check("PreToolUse deny exits 2", denied.code === 2, denied);
  check(
    "PreToolUse deny puts the reason on stderr",
    denied.stderr.includes("recursive force delete"),
    denied.stderr,
  );

  // PreToolUse: a non-matching command is ALLOWED (exit 0, no stderr block).
  const allowed = runHook({
    hook_event_name: "PreToolUse", cwd: proj, tool_name: "Bash",
    tool_input: { command: "echo hi" },
  });
  check("PreToolUse allow exits 0", allowed.code === 0, allowed);

  // PreToolUse: a different tool is not matched by the Bash guard (exit 0).
  const otherTool = runHook({
    hook_event_name: "PreToolUse", cwd: proj, tool_name: "Read",
    tool_input: { file_path: "x" },
  });
  check("PreToolUse non-matching tool exits 0", otherTool.code === 0, otherTool);

  // Repo gate: break the hard code_check -> every tool is denied until the repo is fixed.
  writeFileSync(join(proj, ".editorconfig"), "[*]\nindent_style = tab\n");
  const repoDenied = runHook({
    hook_event_name: "PreToolUse", cwd: proj, tool_name: "Bash",
    tool_input: { command: "echo hi" },
  });
  check("PreToolUse repo gate denies while a hard check fails (exit 2)", repoDenied.code === 2, repoDenied);
  writeFileSync(join(proj, ".editorconfig"), "[*]\nindent_style = space\n");
  const repoAllowed = runHook({
    hook_event_name: "PreToolUse", cwd: proj, tool_name: "Bash",
    tool_input: { command: "echo hi" },
  });
  check("PreToolUse repo gate allows once the check passes (exit 0)", repoAllowed.code === 0, repoAllowed);

  // SessionStart: injects the binding-rule briefing as hookSpecificOutput.additionalContext.
  const start = runHook({ hook_event_name: "SessionStart", cwd: proj });
  check("SessionStart exits 0", start.code === 0, start);
  let startJson = null;
  try {
    startJson = JSON.parse(start.stdout);
  } catch {
    /* asserted below */
  }
  check(
    "SessionStart emits a briefing with the binding rule",
    startJson?.hookSpecificOutput?.hookEventName === "SessionStart" &&
      startJson?.hookSpecificOutput?.additionalContext?.includes("no-rm-rf"),
    start.stdout.slice(0, 300),
  );

  // P2 — compaction continuity: SessionStart source=compact re-injects the briefing PLUS the on-disk
  // continuity brief and the session's live work state, so a post-compaction agent recovers context.
  const briefDir = join(proj, ".agent-control", "handoff");
  mkdirSync(briefDir, { recursive: true });
  writeFileSync(
    join(briefDir, "latest.md"),
    "# Continuity brief\n\nCOMPACT-MARKER-42\n\n## Goal\nShip the spike.\n",
  );
  const compactSession = await tool("create_session", {
    task: "hook", project_path: proj, agent_id: "claude-code", task_tags: [],
  });
  await tool("update_work_state", {
    session_id: compactSession.session_id,
    goal: "Ship the spike",
    work_items: [{ title: "LIVE-ITEM-7", status: "doing" }],
  });
  const compact = runHook(
    { hook_event_name: "SessionStart", source: "compact", cwd: proj },
    url,
    ["claude", "SessionStart"],
  );
  const compactJson = parseOut(compact);
  const compactCtx = compactJson?.hookSpecificOutput?.additionalContext ?? "";
  check("compact start exits 0", compact.code === 0, compact);
  check(
    "compact start injects brief + rules + live work state",
    compactCtx.includes("COMPACT-MARKER-42") &&
      compactCtx.includes("no-rm-rf") &&
      compactCtx.includes("LIVE-ITEM-7"),
    compact.stdout.slice(0, 400),
  );

  // Continuity survives an unreachable daemon: the brief is read from disk before any MCP call.
  const compactDown = runHook(
    { hook_event_name: "SessionStart", source: "compact", cwd: proj },
    "http://127.0.0.1:1/mcp",
    ["claude", "SessionStart"],
  );
  const downCtx = parseOut(compactDown)?.hookSpecificOutput?.additionalContext ?? "";
  check(
    "compact start injects the on-disk brief even when the daemon is down",
    compactDown.code === 0 && downCtx.includes("COMPACT-MARKER-42"),
    compactDown,
  );

  // A project without a brief still gets the rule briefing on compact (no crash, no empty inject).
  const projNoBrief = join(root, "project-no-brief");
  mkdirSync(join(projNoBrief, ".agent-control", "rules"), { recursive: true });
  const compactNoBrief = runHook(
    { hook_event_name: "SessionStart", source: "compact", cwd: projNoBrief },
    url,
    ["qoder-cn", "SessionStart"],
  );
  check(
    "compact start without a brief stays fail-open exit 0",
    compactNoBrief.code === 0,
    compactNoBrief,
  );

  // Item 1 — hook-only enforcement is recorded. PreToolUse resolved a session (get-or-create, task
  // "hook") and passed its id to check_tool_call, so the mediated hard rule earned Enforced and the
  // verdicts landed in the evidence chain — without any explicit MCP session from the harness.
  const hookSession = await tool("create_session", {
    task: "hook", project_path: proj, agent_id: "claude-code", task_tags: [],
  });
  check("hook PreToolUse created a reusable session", hookSession.reused === true, hookSession);
  const hookCtx = await tool("get_session_context", { session_id: hookSession.session_id });
  const enforced = (hookCtx?.session?.assurance ?? []).find(
    (a) => a.rule_id === "no-rm-rf" && a.achieved === "enforced",
  );
  check("hook-only gate recorded the Enforced tier", !!enforced, hookCtx?.session?.assurance);
  const chain = await tool("verify_evidence", { session_id: hookSession.session_id });
  check(
    "hook gate verdicts form a valid evidence chain",
    chain.valid === true && chain.entries_checked > 0,
    chain,
  );

  // Fail-open: an unreachable daemon must never block a session or deny a tool (exit 0 for both).
  const deadUrl = "http://127.0.0.1:1/mcp";
  const deadDeny = runHook(
    { hook_event_name: "PreToolUse", cwd: proj, tool_name: "Bash", tool_input: { command: "rm -rf /" } },
    deadUrl,
  );
  check("PreToolUse fail-open on unreachable daemon (exit 0)", deadDeny.code === 0, deadDeny);
  const deadStart = runHook({ hook_event_name: "SessionStart", cwd: proj }, deadUrl);
  check("SessionStart fail-open on unreachable daemon (exit 0)", deadStart.code === 0, deadStart);

  // No stdin: defaults to SessionStart and fail-open (exit 0) against the dead endpoint.
  const noStdin = runHook(null, deadUrl);
  check("missing stdin fail-open (exit 0)", noStdin.code === 0, noStdin);

  // ---- Per-vendor I/O contracts (vendor + event baked into argv) ----
  // The same hard rule (no-rm-rf) binds every agent, so each vendor's gate must deny `rm -rf` and
  // allow `echo hi` — but in its OWN output shape. The repo gate is back to passing (.editorconfig
  // restored above), so only the tool_guard decides these calls.

  // Cursor: valid-JSON permission decisions on stdout, exit 0 for both deny and allow.
  const cursorDeny = runHook(
    { cwd: proj, tool_name: "Bash", tool_input: { command: "rm -rf /tmp/x" } },
    url, ["cursor", "preToolUse"],
  );
  const cursorDenyJson = parseOut(cursorDeny);
  check("Cursor preToolUse deny exits 0", cursorDeny.code === 0, cursorDeny);
  check(
    "Cursor deny emits {permission:deny} with messages",
    cursorDenyJson?.permission === "deny" &&
      typeof cursorDenyJson?.user_message === "string" &&
      cursorDenyJson.user_message.includes("recursive force delete") &&
      typeof cursorDenyJson?.agent_message === "string",
    cursorDeny.stdout.slice(0, 300),
  );
  const cursorAllow = runHook(
    { cwd: proj, tool_name: "Bash", tool_input: { command: "echo hi" } },
    url, ["cursor", "preToolUse"],
  );
  check(
    "Cursor preToolUse allow emits {permission:allow}",
    cursorAllow.code === 0 && parseOut(cursorAllow)?.permission === "allow",
    cursorAllow,
  );
  const cursorStart = runHook({ cwd: proj }, url, ["cursor", "sessionStart"]);
  const cursorStartJson = parseOut(cursorStart);
  check(
    "Cursor sessionStart injects additional_context with the binding rule",
    cursorStart.code === 0 &&
      typeof cursorStartJson?.additional_context === "string" &&
      cursorStartJson.additional_context.includes("no-rm-rf"),
    cursorStart.stdout.slice(0, 300),
  );
  const cursorDead = runHook(
    { cwd: proj, tool_name: "Bash", tool_input: { command: "rm -rf /" } },
    deadUrl, ["cursor", "preToolUse"],
  );
  check(
    "Cursor fail-open emits {permission:allow} (never blocks on a dead daemon)",
    cursorDead.code === 0 && parseOut(cursorDead)?.permission === "allow",
    cursorDead,
  );

  // Antigravity: stdin nests the call under toolCall.{name,args}; silence == deny since agy 1.4.x, so
  // allow / fail-open MUST emit {decision:ask}. Deny emits {decision:deny,reason}.
  const agDeny = runHook(
    { cwd: proj, toolCall: { name: "Bash", args: { command: "rm -rf /tmp/x" } } },
    url, ["antigravity", "PreToolUse"],
  );
  const agDenyJson = parseOut(agDeny);
  check("Antigravity PreToolUse deny exits 0", agDeny.code === 0, agDeny);
  check(
    "Antigravity deny emits {decision:deny,reason}",
    agDenyJson?.decision === "deny" &&
      typeof agDenyJson?.reason === "string" &&
      agDenyJson.reason.includes("recursive force delete"),
    agDeny.stdout.slice(0, 300),
  );
  const agAllow = runHook(
    { cwd: proj, toolCall: { name: "Bash", args: { command: "echo hi" } } },
    url, ["antigravity", "PreToolUse"],
  );
  check(
    "Antigravity allow emits {decision:ask} (silence would deny)",
    agAllow.code === 0 && parseOut(agAllow)?.decision === "ask",
    agAllow,
  );
  const agStart = runHook({ cwd: proj }, url, ["antigravity", "PreInvocation"]);
  const agStartJson = parseOut(agStart);
  check(
    "Antigravity PreInvocation emits {} (no context-injection channel)",
    agStart.code === 0 && agStartJson !== null && Object.keys(agStartJson).length === 0,
    agStart.stdout.slice(0, 300),
  );
  const agDead = runHook(
    { cwd: proj, toolCall: { name: "Bash", args: { command: "rm -rf /" } } },
    deadUrl, ["antigravity", "PreToolUse"],
  );
  check(
    "Antigravity fail-open emits {decision:ask}",
    agDead.code === 0 && parseOut(agDead)?.decision === "ask",
    agDead,
  );

  // Kimi + Codex + Qoder-CN are claude-shaped: deny => exit 2 + stderr, allow => exit 0.
  for (const vendor of ["kimi", "codex", "qoder-cn"]) {
    const vDeny = runHook(
      { cwd: proj, tool_name: "Bash", tool_input: { command: "rm -rf /" } },
      url, [vendor, "PreToolUse"],
    );
    check(
      `${vendor} PreToolUse deny exits 2 with reason on stderr`,
      vDeny.code === 2 && vDeny.stderr.includes("recursive force delete"),
      vDeny,
    );
    const vAllow = runHook(
      { cwd: proj, tool_name: "Bash", tool_input: { command: "echo hi" } },
      url, [vendor, "PreToolUse"],
    );
    check(`${vendor} PreToolUse allow exits 0`, vAllow.code === 0, vAllow);
  }

  // ---- Fail-closed opt-in (AGENT_CONTROL_HOOK_FAIL_CLOSED) ----
  // A reachable daemon still decides normally: fail-closed only changes the UNREACHABLE path.
  const fcLiveAllow = runHook(
    { cwd: proj, tool_name: "Bash", tool_input: { command: "echo hi" } },
    url, ["claude", "PreToolUse"], null, true,
  );
  check("fail-closed still allows when the daemon reaches a verdict", fcLiveAllow.code === 0, fcLiveAllow);

  // Unreachable daemon + fail-closed: the GATE denies in each vendor's shape.
  const fcClaude = runHook(
    { cwd: proj, tool_name: "Bash", tool_input: { command: "echo hi" } },
    deadUrl, ["claude", "PreToolUse"], null, true,
  );
  check(
    "fail-closed claude gate denies on unreachable daemon (exit 2 + reason)",
    fcClaude.code === 2 && fcClaude.stderr.includes("fail-closed"),
    fcClaude,
  );
  const fcCursor = runHook(
    { cwd: proj, tool_name: "Bash", tool_input: { command: "echo hi" } },
    deadUrl, ["cursor", "preToolUse"], null, true,
  );
  const fcCursorJson = parseOut(fcCursor);
  check(
    "fail-closed Cursor gate emits {permission:deny}",
    fcCursor.code === 0 && fcCursorJson?.permission === "deny" &&
      fcCursorJson.user_message.includes("fail-closed"),
    fcCursor,
  );
  const fcAg = runHook(
    { cwd: proj, toolCall: { name: "Bash", args: { command: "echo hi" } } },
    deadUrl, ["antigravity", "PreToolUse"], null, true,
  );
  const fcAgJson = parseOut(fcAg);
  check(
    "fail-closed Antigravity gate emits {decision:deny}",
    fcAg.code === 0 && fcAgJson?.decision === "deny" && fcAgJson.reason.includes("fail-closed"),
    fcAg,
  );

  // The START/briefing event always stays fail-open, even in fail-closed mode (never blocks init).
  const fcClaudeStart = runHook({ cwd: proj }, deadUrl, ["claude", "SessionStart"], null, true);
  check("fail-closed claude SessionStart stays fail-open (exit 0)", fcClaudeStart.code === 0, fcClaudeStart);
  const fcCursorStart = runHook({ cwd: proj }, deadUrl, ["cursor", "sessionStart"], null, true);
  check(
    "fail-closed Cursor sessionStart stays fail-open (not denied)",
    fcCursorStart.code === 0 && parseOut(fcCursorStart)?.permission !== "deny",
    fcCursorStart,
  );
  const fcAgStart = runHook({ cwd: proj }, deadUrl, ["antigravity", "PreInvocation"], null, true);
  check(
    "fail-closed Antigravity PreInvocation stays fail-open (not denied)",
    fcAgStart.code === 0 && parseOut(fcAgStart)?.decision !== "deny",
    fcAgStart,
  );
} catch (e) {
  failures++;
  console.error("ERROR:", e.message);
} finally {
  child.kill();
  await new Promise((r) => child.once("exit", r));
  rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
console.log(failures === 0 ? "\nHOOK E2E: ALL PASSED" : `\nHOOK E2E: ${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);