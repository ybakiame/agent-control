// Smoke test for the Streamable HTTP transport: starts the server on a random
// port, runs initialize + a tool call, then shuts it down.
// Usage: node scripts/mcp-http-e2e.mjs [path-to-agent-control-mcp binary]
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
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
  check("tools/list over HTTP", tools.result?.tools?.length === 18, tools);
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
    "verify_constraints available over HTTP",
    tools.result?.tools?.some((t) => t.name === "verify_constraints"),
    tools,
  );

  const res = await rpc("tools/call", {
    name: "create_session",
    arguments: { task: "http smoke", project_path: root, agent_id: "http-agent" },
  });
  const data = res.result?.structuredContent ?? JSON.parse(res.result?.content?.[0]?.text ?? "{}");
  check("create_session over HTTP", !!data.session_id, res);

  // export_native over HTTP against the throwaway root (include_user=false: no writes outside it).
  const exp = await rpc("tools/call", {
    name: "export_native",
    arguments: { project_path: root, include_user: false },
  });
  const expData = exp.result?.structuredContent ?? JSON.parse(exp.result?.content?.[0]?.text ?? "{}");
  check(
    "export_native over HTTP writes 3 native targets",
    expData.report?.targets?.length === 3,
    exp,
  );

  // check_export_drift over HTTP: right after export the targets are in sync (no rewrite).
  const drift = await rpc("tools/call", {
    name: "check_export_drift",
    arguments: { project_path: root, include_user: false },
  });
  const driftData = drift.result?.structuredContent ?? JSON.parse(drift.result?.content?.[0]?.text ?? "{}");
  check(
    "check_export_drift over HTTP reports in-sync after export",
    driftData.report?.stale === false && driftData.report?.targets?.length === 3,
    drift,
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