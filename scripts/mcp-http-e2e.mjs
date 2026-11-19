// Smoke test for the Streamable HTTP transport: starts the server on a random
// port, runs initialize + a tool call, then shuts it down.
// Usage: node scripts/mcp-http-e2e.mjs [path-to-agent-control-mcp binary]
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import net from "node:net";

const bin = resolve(process.argv[2] ?? "target/debug/agent-control-mcp.exe");
const root = mkdtempSync(join(tmpdir(), "ac-http-e2e-"));

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
  // SSE framing: extract data lines
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

let failures = 0;
function check(name, cond, detail) {
  if (cond) console.log(`  ok  ${name}`);
  else {
    failures++;
    console.error(`FAIL  ${name}`, detail ?? "");
  }
}

try {
  // wait for listener
  for (let i = 0; i < 50; i++) {
    try {
      await fetch(url, { method: "GET" });
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  const init = await rpc("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "http-e2e", version: "1.0" },
  });
  check("initialize over HTTP", !!init.result?.serverInfo, init);
  check("session id header issued", !!sessionId);
  await rpc("notifications/initialized", {}, true);

  const tools = await rpc("tools/list", {});
  check("tools/list over HTTP", tools.result?.tools?.length === 28, tools);
  check(
    "update_work_state available over HTTP",
    tools.result?.tools?.some((t) => t.name === "update_work_state"),
    tools,
  );
  check(
    "save_rule available over HTTP",
    tools.result?.tools?.some((t) => t.name === "save_rule"),
    tools,
  );
  check(
    "set_rule_enabled available over HTTP",
    tools.result?.tools?.some((t) => t.name === "set_rule_enabled"),
    tools,
  );
  check(
    "suggest_rule available over HTTP",
    tools.result?.tools?.some((t) => t.name === "suggest_rule"),
    tools,
  );
  check(
    "export_native available over HTTP",
    tools.result?.tools?.some((t) => t.name === "export_native"),
    tools,
  );
  check(
    "check_export_drift available over HTTP",
    tools.result?.tools?.some((t) => t.name === "check_export_drift"),
    tools,
  );
  check(
    "revoke_native available over HTTP",
    tools.result?.tools?.some((t) => t.name === "revoke_native"),
    tools,
  );
  check(
    "verify_constraints available over HTTP",
    tools.result?.tools?.some((t) => t.name === "verify_constraints"),
    tools,
  );
  check(
    "set_session_status available over HTTP",
    tools.result?.tools?.some((t) => t.name === "set_session_status"),
    tools,
  );
  for (const name of ["check_tool_call", "register_hooks", "revoke_hooks", "detect_hooks"]) {
    check(
      `${name} available over HTTP`,
      tools.result?.tools?.some((t) => t.name === name),
      tools,
    );
  }

  const res = await rpc("tools/call", {
    name: "create_session",
    arguments: { task: "http smoke", project_path: root, agent_id: "http-agent" },
  });
  const data = res.result?.structuredContent ?? JSON.parse(res.result?.content?.[0]?.text ?? "{}");
  check("create_session over HTTP", !!data.session_id, res);

  const statusRes = await rpc("tools/call", {
    name: "set_session_status",
    arguments: { session_id: data.session_id, status: "completed" },
  });
  const statusData =
    statusRes.result?.structuredContent ??
    JSON.parse(statusRes.result?.content?.[0]?.text ?? "{}");
  check("set_session_status over HTTP", statusData.status === "completed", statusRes);

  // export_native over HTTP against the throwaway root (include_user=false: no writes outside it).
  // Select all four targets explicitly; the block is a static pointer, not a copy of rule text.
  const allTargets = ["agents", "claude", "gemini", "cursor"];
  const exp = await rpc("tools/call", {
    name: "export_native",
    arguments: { project_path: root, targets: allTargets, include_user: false },
  });
  const expData = exp.result?.structuredContent ?? JSON.parse(exp.result?.content?.[0]?.text ?? "{}");
  check(
    "export_native over HTTP writes the 4 selected targets",
    expData.report?.targets?.length === 4,
    exp,
  );
  const agentsText = readFileSync(join(root, "AGENTS.md"), "utf8");
  check(
    "export_native over HTTP writes a pointer block with a format version",
    agentsText.includes("agent-control:begin") &&
      agentsText.includes(".agent-control/rules/") &&
      agentsText.includes("agent-control:block-format:"),
    agentsText.slice(0, 200),
  );

  // check_export_drift over HTTP: right after export the targets are in sync (no rewrite).
  const drift = await rpc("tools/call", {
    name: "check_export_drift",
    arguments: { project_path: root, targets: allTargets, include_user: false },
  });
  const driftData = drift.result?.structuredContent ?? JSON.parse(drift.result?.content?.[0]?.text ?? "{}");
  check(
    "check_export_drift over HTTP reports in-sync after export",
    driftData.report?.stale === false &&
      driftData.report?.targets?.length === 4 &&
      driftData.report.targets.every((t) => t.exported_format === t.current_format),
    drift,
  );

  // revoke_native over HTTP: the root held only block-only files, so all four are deleted.
  const rev = await rpc("tools/call", {
    name: "revoke_native",
    arguments: { project_path: root, targets: allTargets, include_user: false },
  });
  const revData = rev.result?.structuredContent ?? JSON.parse(rev.result?.content?.[0]?.text ?? "{}");
  check(
    "revoke_native over HTTP deletes the block-only targets",
    revData.report?.targets?.length === 4 &&
      revData.report.targets.every((t) => t.action === "deleted") &&
      !existsSync(join(root, "AGENTS.md")),
    rev,
  );
  // ---- Layer 2 over HTTP: the deny-capable gate + hooks registration ----
  // Mirrors the stdio e2e semantics against throwaway subdirs (never the real repo or ~/.claude).
  async function call(name, args) {
    const r = await rpc("tools/call", { name, arguments: args });
    if (r.error) throw new Error(`${name}: ${JSON.stringify(r.error)}`);
    if (r.result?.isError) throw new Error(`${name}: ${r.result?.content?.[0]?.text}`);
    return r.result?.structuredContent ?? JSON.parse(r.result?.content?.[0]?.text ?? "{}");
  }

  const guardDir = join(root, "gate-guard");
  mkdirSync(join(guardDir, ".agent-control", "rules"), { recursive: true });
  writeFileSync(join(guardDir, ".editorconfig"), "[*]\nindent_style = space\n");
  await call("save_rule", {
    id: "no-rm-rf", title: "No recursive force delete", content: "Never run rm -rf.",
    agent_id: "http-agent", kind: "constraint", scope: "project", project_path: guardDir,
    enforcement: "hard",
    code_checks: [{ file: ".editorconfig", key: "indent_style", expect: "space" }],
    tool_guards: [{ tool: "Bash", pattern: "rm\\s+-rf", reason: "recursive force delete is banned" }],
  });
  const gsid = (await call("create_session", { task: "http gate", project_path: guardDir, agent_id: "http-agent" })).session_id;
  const gateArgs = { project_path: guardDir, agent_id: "http-agent", session_id: gsid };
  const denied = await call("check_tool_call", { ...gateArgs, tool_name: "Bash", tool_input: { command: "rm -rf /tmp/x" } });
  check(
    "check_tool_call over HTTP denies a matching tool_guard",
    denied.report?.allowed === false && denied.report?.denials?.some((d) => d.source === "tool_guard"),
    denied,
  );
  const allowedCall = await call("check_tool_call", { ...gateArgs, tool_name: "Bash", tool_input: { command: "echo hi" } });
  check(
    "check_tool_call over HTTP allows a non-matching command and enforces the hard rule",
    allowedCall.report?.allowed === true && allowedCall.report?.enforced_rules?.includes("no-rm-rf"),
    allowedCall,
  );
  const gateCtx = await call("get_session_context", { session_id: gsid });
  check(
    "gate over HTTP marks the mediated hard rule Enforced",
    gateCtx.session?.assurance?.some((a) => a.rule_id === "no-rm-rf" && a.achieved === "enforced"),
    gateCtx,
  );

  const hooksDir = join(root, "hooks-project");
  mkdirSync(join(hooksDir, ".agent-control", "rules"), { recursive: true });
  mkdirSync(join(hooksDir, ".claude"), { recursive: true });
  const claudeSettings = join(hooksDir, ".claude", "settings.json");
  writeFileSync(claudeSettings, JSON.stringify({
    model: "opus",
    hooks: { PreToolUse: [{ matcher: "Bash", hooks: [{ type: "command", command: "echo user" }] }] },
  }));
  const hookCmd = 'node "C:/throwaway/agent-control-hook.mjs"';
  const hookTargets = ["claude", "codebuddy", "workbuddy", "qoder", "qoder-cn", "codex", "cursor"];
  const registered = await call("register_hooks", { project_path: hooksDir, targets: hookTargets, command: hookCmd });
  check(
    "register_hooks over HTTP writes the six project-scope config files (qoder + qoder-cn share one)",
    registered.report?.targets?.length === 6 &&
      registered.command === hookCmd &&
      existsSync(join(hooksDir, ".qoder", "settings.json")) &&
      !existsSync(join(hooksDir, ".qoder-cn", "settings.json")),
    registered,
  );
  const claudeJson = JSON.parse(readFileSync(claudeSettings, "utf8"));
  check(
    "register_hooks over HTTP preserves other config and the user's hook",
    claudeJson.model === "opus" &&
      claudeJson.hooks.PreToolUse.some((g) => g.hooks?.[0]?.command === "echo user") &&
      claudeJson.hooks.PreToolUse.some((g) => g.hooks?.[0]?.command?.includes("agent-control-hook.mjs")) &&
      Array.isArray(claudeJson.hooks.SessionStart),
    claudeJson,
  );
  const detected = await call("detect_hooks", { project_path: hooksDir });
  check(
    "detect_hooks over HTTP reports nine harnesses, seven project-scope registered",
    detected.harnesses?.length === 9 &&
      detected.harnesses.filter((h) => h.scope === "project").every((h) => h.registered),
    detected,
  );
  const reregistered = await call("register_hooks", { project_path: hooksDir, targets: hookTargets, command: hookCmd });
  check(
    "register_hooks over HTTP re-run is idempotent",
    reregistered.report?.targets?.every((t) => t.action === "unchanged"),
    reregistered,
  );
  await call("revoke_hooks", { project_path: hooksDir, targets: hookTargets });
  const claudeAfter = JSON.parse(readFileSync(claudeSettings, "utf8"));
  check(
    "revoke_hooks over HTTP keeps the user's config, removes ours, deletes stubs",
    claudeAfter.model === "opus" &&
      claudeAfter.hooks.PreToolUse.length === 1 &&
      claudeAfter.hooks.PreToolUse[0].hooks[0].command === "echo user" &&
      claudeAfter.hooks.SessionStart === undefined &&
      !existsSync(join(hooksDir, ".codebuddy", "settings.json")) &&
      !existsSync(join(hooksDir, ".workbuddy", "settings.json")) &&
      !existsSync(join(hooksDir, ".qoder", "settings.json")) &&
      !existsSync(join(hooksDir, ".qoder-cn", "settings.json")) &&
      !existsSync(join(hooksDir, ".codex", "hooks.json")) &&
      !existsSync(join(hooksDir, ".cursor", "hooks.json")),
    claudeAfter,
  );
} catch (e) {
  failures++;
  console.error("ERROR:", e.message);
} finally {
  child.kill();
  await new Promise((r) => child.once("exit", r));
  rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
console.log(failures === 0 ? "\nMCP HTTP E2E: ALL PASSED" : `\nMCP HTTP E2E: ${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);