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

// Run the hook as a one-shot child with the given stdin payload and MCP endpoint.
function runHook(payload, endpoint = url) {
  const res = spawnSync(process.execPath, [hook], {
    input: payload === null ? "" : JSON.stringify(payload),
    env: { ...process.env, AGENT_CONTROL_MCP_URL: endpoint, AGENT_CONTROL_HOOK_TIMEOUT_MS: "8000" },
    encoding: "utf8",
    timeout: 15000,
  });
  return { code: res.status, stdout: res.stdout ?? "", stderr: res.stderr ?? "" };
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