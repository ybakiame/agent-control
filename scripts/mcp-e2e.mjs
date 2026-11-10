// End-to-end test: spawns the MCP stdio server against a throwaway data dir
// and exercises the full protocol lifecycle (rules -> session -> evidence -> handoff).
// Usage: node scripts/mcp-e2e.mjs [path-to-agent-control-mcp binary]
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const bin = resolve(process.argv[2] ?? "target/debug/agent-control-mcp.exe");
const root = mkdtempSync(join(tmpdir(), "ac-e2e-"));
const dataDir = join(root, "data");
const projectDir = join(root, "project");
mkdirSync(join(dataDir, "rules", "user"), { recursive: true });
mkdirSync(join(projectDir, ".agent-control", "rules"), { recursive: true });

const now = new Date().toISOString();
writeFileSync(
  join(dataDir, "rules", "user", "prefer-concise.md"),
  `---
id: prefer-concise
title: Prefer concise answers
kind: preference
scope: user
source:
  kind: user
  created_at: "${now}"
---
Answer concisely; avoid filler.
`,
);
writeFileSync(
  join(projectDir, ".agent-control", "rules", "use-tabs.md"),
  `---
id: use-tabs
title: Indent with tabs
kind: contract
scope: project
source:
  kind: project
  created_at: "${now}"
code_checks:
  - file: .editorconfig
    key: indent_style
    expect: tab
---
All indentation in this project must use tabs.
`,
);
// Deliberately contradicts the rule -> expect a rule-vs-code conflict.
writeFileSync(join(projectDir, ".editorconfig"), "root = true\n\n[*]\nindent_style = space\n");

const child = spawn(bin, ["--stdio", "--data-dir", dataDir], {
  stdio: ["pipe", "pipe", "inherit"],
});
child.on("error", (e) => {
  console.error("spawn failed:", e.message);
  process.exit(1);
});

let buf = "";
const pending = new Map();
let nextId = 1;
child.stdout.on("data", (chunk) => {
  buf += chunk.toString();
  let idx;
  while ((idx = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (!line) continue;
    const msg = JSON.parse(line);
    if (msg.id !== undefined && pending.has(msg.id)) {
      const { resolve, timer } = pending.get(msg.id);
      clearTimeout(timer);
      pending.delete(msg.id);
      resolve(msg);
    }
  }
});

function rpc(method, params) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`timeout: ${method}`)), 15000);
    pending.set(id, { resolve, timer });
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  });
}

function notify(method, params) {
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n");
}

let failures = 0;
function check(name, cond, detail) {
  if (cond) {
    console.log(`  ok  ${name}`);
  } else {
    failures++;
    console.error(`FAIL  ${name}${detail ? " — " + JSON.stringify(detail).slice(0, 300) : ""}`);
  }
}

async function tool(name, args) {
  const res = await rpc("tools/call", { name, arguments: args });
  if (res.error) throw new Error(`${name}: ${JSON.stringify(res.error)}`);
  const r = res.result;
  if (r.isError) throw new Error(`${name}: ${r.content?.[0]?.text}`);
  return r.structuredContent ?? JSON.parse(r.content[0].text);
}

try {
  const init = await rpc("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "e2e", version: "1.0" },
  });
  check("initialize", !!init.result?.serverInfo, init);
  notify("notifications/initialized", {});

  const tools = await rpc("tools/list", {});
  const names = tools.result.tools.map((t) => t.name);
  check("tools/list has 14 tools", names.length === 14, names);
  check("save_rule registered", names.includes("save_rule"), names);
  check("suggest_rule registered", names.includes("suggest_rule"), names);
  check("list_suggestions registered", names.includes("list_suggestions"), names);

  const rules = await tool("list_rules", { project_path: projectDir });
  check("list_rules returns user + project rule", rules.rules?.length === 2, rules);

  const query = { project_path: projectDir, agent_id: "harness-a", languages: ["rust"] };
  const applicable = await tool("get_applicable_rules", query);
  check("get_applicable_rules applies both rules", applicable.applied_rules?.length === 2, applicable);
  check(
    "rule-vs-code conflict detected (.editorconfig space vs rule tab)",
    applicable.conflicts?.some((c) => c.kind === "rule_vs_code"),
    applicable.conflicts,
  );
  check("provenance id returned", !!applicable.provenance_id);

  const explanation = await tool("explain_rule", { rule_id: "use-tabs", ...query });
  check("explain_rule returns explanation", !!explanation, explanation);

  const conflicts = await tool("check_conflicts", query);
  check("check_conflicts reports conflict", conflicts.has_conflicts === true, conflicts);

  const session = await tool("create_session", {
    task: "e2e: refactor module",
    project_path: projectDir,
    agent_id: "harness-a",
  });
  const sessionId = session.session_id;
  check("create_session freezes 2 rules", session.rules?.length === 2, session);
  const ruleIds = session.rules.map((r) => r.id);

  const ack = await tool("acknowledge_rules", {
    session_id: sessionId,
    agent_id: "harness-a",
    rule_ids: ruleIds,
  });
  check("acknowledge_rules accepted", !!ack, ack);

  const badAck = await tool("acknowledge_rules", {
    session_id: sessionId,
    agent_id: "harness-a",
    rule_ids: ["prefer-concise"],
  });
  check("partial acknowledgment flagged", JSON.stringify(badAck).includes("use-tabs"), badAck);

  const entry = await tool("record_evidence", {
    session_id: sessionId,
    kind: "file_change",
    agent_id: "harness-a",
    payload: { file: "src/lib.rs", before_sha256: "a".repeat(64), after_sha256: "b".repeat(64) },
  });
  check("record_evidence appends entry", entry.seq >= 1 && !!entry.entry_hash, entry);

  const verify = await tool("verify_evidence", { session_id: sessionId });
  check("evidence chain valid", verify.valid === true, verify);

  const bundle = await tool("export_handoff", {
    session_id: sessionId,
    to_agent: "harness-b",
    context: { summary: "module refactor in progress" },
  });
  check("export_handoff returns bundle", !!bundle.bundle_hash, bundle);

  const resumed = await tool("import_handoff", { bundle, new_agent: "harness-b" });
  check("import_handoff switches agent", resumed.agent_id === "harness-b", resumed);
  check(
    "handoff carries same rule ids",
    JSON.stringify(resumed.rules_to_acknowledge.sort()) === JSON.stringify([...ruleIds].sort()),
    resumed,
  );

  await tool("acknowledge_rules", {
    session_id: sessionId,
    agent_id: "harness-b",
    rule_ids: ruleIds,
  });
  const ctx2 = await tool("get_session_context", { session_id: sessionId });
  check("session context shows handoff", ctx2.session?.handoffs?.length === 1, ctx2.session?.handoffs);
  check("evidence head carried over", !!ctx2.evidence_head_hash, ctx2);

  // save_rule: an agent actively records a newly discovered project contract.
  const saved = await tool("save_rule", {
    id: "api-isasc",
    title: "Upstream isAsc type is ambiguous",
    content: "isAsc may be bool or string; normalize explicitly before calling upstream.",
    agent_id: "harness-b",
    kind: "contract",
    scope: "project",
    project_path: projectDir,
    tags: ["api"],
  });
  check("save_rule writes a rule file", !!saved.path && saved.rule?.id === "api-isasc", saved);
  const afterSave = await tool("list_rules", { project_path: projectDir });
  check("save_rule rule is loaded", afterSave.rules?.some((r) => r.id === "api-isasc" && r.scope === "project"), afterSave);

  // Reusing the id updates the rule and bumps its version (no duplicate).
  const updated = await tool("save_rule", {
    id: "api-isasc",
    title: "Upstream isAsc type is ambiguous",
    content: "Confirmed: endpoint /sort accepts string only.",
    agent_id: "harness-b",
    scope: "project",
    project_path: projectDir,
  });
  check("save_rule upsert bumps version", updated.rule?.version === 2, updated);
  const afterUpdate = await tool("list_rules", { project_path: projectDir });
  check("save_rule upsert creates no duplicate", afterUpdate.rules?.filter((r) => r.id === "api-isasc").length === 1, afterUpdate);

  // Boundary validation: project scope requires project_path; unknown kind is rejected.
  async function toolRejects(name, args) {
    const res = await rpc("tools/call", { name, arguments: args });
    return res.result?.isError === true || !!res.error;
  }
  check(
    "save_rule rejects project scope without project_path",
    await toolRejects("save_rule", { id: "x", title: "x", content: "x", agent_id: "a", scope: "project" }),
  );
  check(
    "save_rule rejects unknown kind",
    await toolRejects("save_rule", { id: "x", title: "x", content: "x", agent_id: "a", kind: "bogus", scope: "user" }),
  );

  // suggest_rule: an agent proposes a rule for human review instead of writing it directly.
  const suggested = await tool("suggest_rule", {
    project_path: projectDir,
    agent_id: "harness-b",
    id: "api-rate-limit",
    title: "Upstream API rate limit",
    content: "Batch requests; upstream allows at most 10 calls per second.",
    kind: "constraint",
    tags: ["api"],
    rationale: "Observed 429 responses during integration.",
  });
  check(
    "suggest_rule queues a pending suggestion",
    suggested.suggestion?.status === "pending" && !!suggested.suggestion?.id,
    suggested,
  );
  const beforeReview = await tool("list_rules", { project_path: projectDir });
  check(
    "suggestion does not bind before approval",
    !beforeReview.rules?.some((r) => r.id === "api-rate-limit"),
    beforeReview,
  );
  const queue = await tool("list_suggestions", { project_path: projectDir, status: "pending" });
  check(
    "list_suggestions returns the pending queue",
    queue.pending_count === 1 && queue.suggestions?.some((s) => s.rule_id === "api-rate-limit"),
    queue,
  );
  check(
    "suggest_rule rejects missing project_path",
    await toolRejects("suggest_rule", { id: "x", title: "x", content: "x", agent_id: "a" }),
  );
  check(
    "suggest_rule rejects unknown kind",
    await toolRejects("suggest_rule", { project_path: projectDir, id: "x", title: "x", content: "x", agent_id: "a", kind: "bogus" }),
  );
  check(
    "list_suggestions rejects unknown status",
    await toolRejects("list_suggestions", { status: "bogus" }),
  );

  const resources = await rpc("resources/list", {});
  check("resources/list works", Array.isArray(resources.result?.resources), resources);
  const prompts = await rpc("prompts/list", {});
  check("prompts/list works", Array.isArray(prompts.result?.prompts), prompts);
} catch (e) {
  failures++;
  console.error("ERROR:", e.message);
} finally {
  child.kill();
  await new Promise((r) => child.once("exit", r));
  rmSync(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}

console.log(failures === 0 ? "\nMCP E2E: ALL PASSED" : `\nMCP E2E: ${failures} FAILURE(S)`);
process.exit(failures === 0 ? 0 : 1);