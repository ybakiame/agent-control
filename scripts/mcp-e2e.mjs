// End-to-end test: spawns the MCP stdio server against a throwaway data dir
// and exercises the full protocol lifecycle (rules -> session -> evidence -> handoff).
// Usage: node scripts/mcp-e2e.mjs [path-to-agent-control-mcp binary]
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, existsSync } from "node:fs";
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
  check("tools/list has 16 tools", names.length === 16, names);
  check("save_rule registered", names.includes("save_rule"), names);
  check("set_rule_enabled registered", names.includes("set_rule_enabled"), names);
  check("suggest_rule registered", names.includes("suggest_rule"), names);
  check("list_suggestions registered", names.includes("list_suggestions"), names);
  check("export_native registered", names.includes("export_native"), names);

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

  // set_rule_enabled: pause/resume a rule without deleting it; disabled rules stop binding.
  const disabled = await tool("set_rule_enabled", {
    id: "api-isasc", enabled: false, scope: "project", project_path: projectDir,
  });
  check("set_rule_enabled disables a rule", disabled.rule?.enabled === false, disabled);
  const afterDisable = await tool("get_applicable_rules", query);
  check(
    "disabled rule is excluded from resolution",
    !afterDisable.applied_rules?.some((m) => m.rule.id === "api-isasc"),
    afterDisable,
  );

  // A content update via save_rule must NOT silently re-enable a disabled rule.
  await tool("save_rule", {
    id: "api-isasc",
    title: "Upstream isAsc type is ambiguous",
    content: "Still paused; content edited while disabled.",
    agent_id: "harness-b",
    scope: "project",
    project_path: projectDir,
  });
  const stillDisabled = await tool("list_rules", { project_path: projectDir });
  check(
    "save_rule preserves the disabled state",
    stillDisabled.rules?.some((r) => r.id === "api-isasc" && r.enabled === false),
    stillDisabled,
  );

  const reenabled = await tool("set_rule_enabled", {
    id: "api-isasc", enabled: true, scope: "project", project_path: projectDir,
  });
  check("set_rule_enabled re-enables a rule", reenabled.rule?.enabled === true, reenabled);
  const afterEnable = await tool("get_applicable_rules", query);
  check(
    "re-enabled rule binds again",
    afterEnable.applied_rules?.some((m) => m.rule.id === "api-isasc"),
    afterEnable,
  );

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
  check(
    "set_rule_enabled rejects project scope without project_path",
    await toolRejects("set_rule_enabled", { id: "api-isasc", enabled: false, scope: "project" }),
  );
  check(
    "set_rule_enabled rejects unknown scope",
    await toolRejects("set_rule_enabled", { id: "api-isasc", enabled: false, scope: "bogus" }),
  );
  check(
    "set_rule_enabled rejects session scope",
    await toolRejects("set_rule_enabled", { id: "api-isasc", enabled: false, scope: "session" }),
  );
  check(
    "set_rule_enabled rejects an unknown rule id",
    await toolRejects("set_rule_enabled", { id: "does-not-exist", enabled: false, scope: "user" }),
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

  // suggest_rule scope=user: an agent proposes a personal preference (e.g. an environment note)
  // for human review; approval writes a user-scope rule that follows the user across projects.
  const userSuggested = await tool("suggest_rule", {
    scope: "user",
    agent_id: "codex-cli",
    id: "wsl-env",
    title: "Agents run inside WSL",
    content: "The project lives in WSL; compile/verify there, but Windows-native tools are callable when needed.",
    kind: "constraint",
    rationale: "Agent repeatedly needed reminding it was in WSL during build verification.",
  });
  check(
    "suggest_rule scope=user queues a pending user suggestion",
    userSuggested.suggestion?.status === "pending" && userSuggested.suggestion?.scope === "user",
    userSuggested,
  );
  const userQueue = await tool("list_suggestions", { status: "pending" });
  check(
    "user suggestion appears in the unfiltered queue with scope=user",
    userQueue.suggestions?.some((s) => s.rule_id === "wsl-env" && s.scope === "user"),
    userQueue,
  );
  const projQueueAfterUser = await tool("list_suggestions", { project_path: projectDir, status: "pending" });
  check(
    "user suggestion is excluded from the project-filtered queue",
    !projQueueAfterUser.suggestions?.some((s) => s.rule_id === "wsl-env"),
    projQueueAfterUser,
  );
  const userRulesBefore = await tool("list_rules", {});
  check(
    "user suggestion does not bind before approval",
    !userRulesBefore.rules?.some((r) => r.id === "wsl-env"),
    userRulesBefore,
  );
  check(
    "suggest_rule rejects unknown scope",
    await toolRejects("suggest_rule", { scope: "bogus", id: "x", title: "x", content: "x", agent_id: "a" }),
  );
  check(
    "suggest_rule rejects session scope",
    await toolRejects("suggest_rule", { scope: "session", id: "x", title: "x", content: "x", agent_id: "a" }),
  );

  // export_native: compile binding contracts into native harness entry points as a managed block,
  // preserving hand-written prose outside it. include_user=false keeps every write inside the
  // throwaway project dir (never the real ~/.claude/CLAUDE.md).
  const agentsPath = join(projectDir, "AGENTS.md");
  writeFileSync(agentsPath, "# Hand-written repo guide\n\nKeep this prose.\n");
  const exported = await tool("export_native", { project_path: projectDir, include_user: false });
  check(
    "export_native reports enabled project rules and 3 targets",
    exported.report?.project_rules === 2 && exported.report?.targets?.length === 3,
    exported,
  );
  const agentsText = readFileSync(agentsPath, "utf8");
  check(
    "export_native preserves prose and injects the managed block with both contracts",
    agentsText.startsWith("# Hand-written repo guide\n\nKeep this prose.\n") &&
      agentsText.includes("agent-control:begin") &&
      agentsText.includes("use-tabs") &&
      agentsText.includes("api-isasc"),
    agentsText.slice(0, 200),
  );
  const cursorPath = join(projectDir, ".cursor", "rules", "agent-control.mdc");
  check(
    "export_native writes a Cursor rule with frontmatter outside the block",
    existsSync(cursorPath) && readFileSync(cursorPath, "utf8").startsWith("---\n"),
    cursorPath,
  );
  check(
    "export_native writes project CLAUDE.md",
    existsSync(join(projectDir, "CLAUDE.md")),
  );
  const reexported = await tool("export_native", { project_path: projectDir, include_user: false });
  check(
    "export_native re-export is idempotent (all targets unchanged)",
    reexported.report?.targets?.every((t) => t.action === "unchanged"),
    reexported,
  );
  check(
    "export_native rejects an empty project_path",
    await toolRejects("export_native", { project_path: "  ", include_user: false }),
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