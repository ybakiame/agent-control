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
  check("tools/list has 28 tools", names.length === 28, names);
  check("save_rule registered", names.includes("save_rule"), names);
  check("set_rule_enabled registered", names.includes("set_rule_enabled"), names);
  check("suggest_rule registered", names.includes("suggest_rule"), names);
  check("list_suggestions registered", names.includes("list_suggestions"), names);
  check("export_native registered", names.includes("export_native"), names);
  check("check_export_drift registered", names.includes("check_export_drift"), names);
  check("revoke_native registered", names.includes("revoke_native"), names);
  check("verify_constraints registered", names.includes("verify_constraints"), names);
  check("update_work_state registered", names.includes("update_work_state"), names);
  check("check_tool_call registered", names.includes("check_tool_call"), names);
  check("register_hooks registered", names.includes("register_hooks"), names);
  check("revoke_hooks registered", names.includes("revoke_hooks"), names);
  check("detect_hooks registered", names.includes("detect_hooks"), names);
  check("doctor registered", names.includes("doctor"), names);
  check("search registered", names.includes("search"), names);
  check("setup registered", names.includes("setup"), names);

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
  // The briefing is rendered by core and shared with the SessionStart hook + constraint-briefing prompt.
  check(
    "get_applicable_rules returns a unified briefing",
    typeof applicable.briefing === "string" &&
      applicable.briefing.includes("project context") &&
      applicable.briefing.includes("summarize") &&
      applicable.briefing.includes("use-tabs"),
    applicable.briefing,
  );

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

  // Idempotent creation: same (project, agent, task) while active returns the same session.
  const again = await tool("create_session", {
    task: "e2e: refactor module",
    project_path: projectDir,
    agent_id: "harness-a",
  });
  check(
    "create_session reuses active same-task session",
    again.session_id === sessionId && again.reused === true,
    again,
  );

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

  // Resumable work state: goal + done/doing/todo items travel with the handoff, not the harness todo list.
  const workState = await tool("update_work_state", {
    session_id: sessionId,
    goal: "ship the refactor without breaking the public API",
    work_items: [
      { title: "extract module", status: "done" },
      { title: "wire hook", status: "doing" },
      { title: "e2e coverage", status: "todo" },
    ],
  });
  check(
    "update_work_state records goal",
    workState.goal === "ship the refactor without breaking the public API",
    workState,
  );
  check(
    "update_work_state records 3 items",
    workState.work_items?.length === 3 && workState.work_items[1].status === "doing",
    workState,
  );

  // UX5 — acting-identity attribution: every session mutation records the CALLER in the chain.
  const ux5 = await tool("create_session", {
    task: "ux5-audit", project_path: projectDir, agent_id: "harness-a", task_tags: [],
    harness_session_id: "conv-ux5",
  });
  await tool("update_work_state", {
    session_id: ux5.session_id, goal: "audit trail", work_items: [], agent_id: "auditor",
  });
  await tool("set_session_status", { session_id: ux5.session_id, status: "completed", agent_id: "auditor" });
  await tool("set_session_status", { session_id: ux5.session_id, status: "active", agent_id: "harness-a" });
  const ux5ctx = await tool("get_session_context", { session_id: ux5.session_id, include_evidence: true });
  const kinds = ux5ctx.evidence.map((e) => `${e.kind}:${e.agent_id}`);
  check(
    "work-state/status mutations carry the acting agent (incl. reopen)",
    kinds.includes("work_state_update:auditor") &&
      kinds.includes("status_change:auditor") &&
      kinds.includes("session_end:auditor") &&
      kinds.includes("status_change:harness-a"),
    kinds,
  );
  check(
    "harness_session_id round-trips into session + evidence",
    ux5ctx.session?.harness_session_id === "conv-ux5" &&
      ux5ctx.evidence.some((e) => e.payload?.harness_session_id === "conv-ux5"),
    ux5ctx.session?.harness_session_id,
  );
  const ux5bundle = await tool("export_handoff", { session_id: ux5.session_id, context: {} });
  await tool("import_handoff", { bundle: ux5bundle, new_agent: "harness-b" });
  const ux5own = await tool("get_session_context", { session_id: ux5.session_id, include_evidence: true });
  check(
    "ownership timeline derives the agent flip from evidence",
    ux5own.ownership?.length === 2 &&
      ux5own.ownership[0].agent_id === "harness-a" &&
      ux5own.ownership[1].agent_id === "harness-b" &&
      ux5own.ownership[1].until === null,
    ux5own.ownership,
  );
  check("ux5 chain stays valid", (await tool("verify_evidence", { session_id: ux5.session_id })).valid === true);

  const bundle = await tool("export_handoff", {
    session_id: sessionId,
    context: { summary: "module refactor in progress" },
  });
  check("export_handoff returns bundle", !!bundle.bundle_hash, bundle);
  const handedOff = await tool("get_session_context", { session_id: sessionId });
  check(
    "export_handoff marks source handed_off (to_agent optional)",
    handedOff.session?.status === "handed_off",
    handedOff,
  );
  check(
    "handoff bundle carries goal + work items",
    bundle.goal === "ship the refactor without breaking the public API" &&
      bundle.work_items?.length === 3,
    bundle,
  );

  // Layer 1: export_handoff also writes a project-local continuity brief the managed block points at.
  const latestPath = join(projectDir, ".agent-control", "handoff", "latest.md");
  check("export_handoff writes latest.md", existsSync(latestPath), latestPath);
  const latestText = existsSync(latestPath) ? readFileSync(latestPath, "utf8") : "";
  check("latest.md carries task", latestText.includes("e2e: refactor module"), latestText);
  check("latest.md carries from-agent", latestText.includes("harness-a"), latestText);
  check("latest.md renders goal", latestText.includes("## Goal") && latestText.includes("public API"), latestText);
  check(
    "latest.md renders work state",
    latestText.includes("## Work state") &&
      latestText.includes("**done**: extract module") &&
      latestText.includes("**doing**: wire hook") &&
      latestText.includes("**todo**: e2e coverage"),
    latestText,
  );

  const resumed = await tool("import_handoff", { bundle, new_agent: "harness-b" });
  check("import_handoff switches agent", resumed.agent_id === "harness-b", resumed);
  check(
    "import_handoff carries work state",
    resumed.goal === "ship the refactor without breaking the public API" &&
      resumed.work_items?.length === 3,
    resumed,
  );
  // import_handoff refreshes latest.md to the RESUMED state so the file-based reach path stays current.
  const resumedText = existsSync(latestPath) ? readFileSync(latestPath, "utf8") : "";
  check(
    "import_handoff refreshes latest.md with resumed-under",
    resumedText.includes("Resumed under") && resumedText.includes("harness-b"),
    resumedText,
  );
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

  // suggest_rule scope=user: an agent proposes a portable personal preference for human review;
  // approval writes a user-scope rule that follows the user across projects. (Machine/project facts
  // like WSL are NOT preferences — they belong in scope=environment, covered below.)
  const userSuggested = await tool("suggest_rule", {
    scope: "user",
    agent_id: "codex-cli",
    id: "prefer-tabs",
    title: "Prefer tabs over spaces",
    content: "I prefer tabs for indentation across all my projects; a portable personal preference.",
    kind: "preference",
    rationale: "User repeatedly stated a personal indentation preference.",
  });
  check(
    "suggest_rule scope=user queues a pending user suggestion",
    userSuggested.suggestion?.status === "pending" && userSuggested.suggestion?.scope === "user",
    userSuggested,
  );
  const userQueue = await tool("list_suggestions", { status: "pending" });
  check(
    "user suggestion appears in the unfiltered queue with scope=user",
    userQueue.suggestions?.some((s) => s.rule_id === "prefer-tabs" && s.scope === "user"),
    userQueue,
  );
  const projQueueAfterUser = await tool("list_suggestions", { project_path: projectDir, status: "pending" });
  check(
    "user suggestion is excluded from the project-filtered queue",
    !projQueueAfterUser.suggestions?.some((s) => s.rule_id === "prefer-tabs"),
    projQueueAfterUser,
  );
  const userRulesBefore = await tool("list_rules", {});
  check(
    "user suggestion does not bind before approval",
    !userRulesBefore.rules?.some((r) => r.id === "prefer-tabs"),
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

  // scope=environment: machine/project-local descriptive context (e.g. WSL). Stored git-ignored in
  // .agent-control/local/, always applied (non-competitive), delivered via the briefing, and NEVER
  // exported into the committed AGENTS.md. Uses its own throwaway project dir.
  const envDir = join(root, "env-project");
  mkdirSync(envDir, { recursive: true });
  const envSaved = await tool("save_rule", {
    id: "wsl-env",
    title: "Agents run inside WSL",
    content: "MACHINE-ONLY-FACT: harnesses run in WSL; Windows-native tools are callable for compile/verify.",
    agent_id: "harness-a",
    kind: "constraint",
    scope: "environment",
    project_path: envDir,
  });
  check(
    "save_rule scope=environment writes into git-ignored local/",
    envSaved.rule?.scope === "environment" &&
      envSaved.path?.replaceAll("\\", "/").includes("/.agent-control/local/"),
    envSaved,
  );
  check(
    "save_rule scope=environment creates a .gitignore",
    existsSync(join(envDir, ".agent-control", "local", ".gitignore")),
  );
  const envApplicable = await tool("get_applicable_rules", {
    project_path: envDir,
    agent_id: "harness-a",
    file_paths: [],
    languages: [],
    task_tags: [],
  });
  check(
    "get_applicable_rules applies the environment note as context",
    envApplicable.applied_rules?.some((r) => r.rule?.id === "wsl-env" && r.rule?.scope === "environment"),
    envApplicable,
  );
  check(
    "briefing renders a dedicated work-environment section",
    typeof envApplicable.briefing === "string" &&
      envApplicable.briefing.includes("Work environment") &&
      envApplicable.briefing.includes("MACHINE-ONLY-FACT"),
    envApplicable.briefing?.slice(0, 200),
  );
  const envGlobalRules = await tool("list_rules", {});
  check(
    "environment note is project-local (absent without project_path)",
    !envGlobalRules.rules?.some((r) => r.id === "wsl-env"),
    envGlobalRules,
  );
  const envExport = await tool("export_native", {
    project_path: envDir,
    targets: ["agents"],
    include_user: false,
  });
  const envAgentsText = readFileSync(join(envDir, "AGENTS.md"), "utf8");
  check(
    "export_native excludes environment content from the committed block",
    envExport.report?.project_rules === 0 && !envAgentsText.includes("MACHINE-ONLY-FACT"),
    { report: envExport.report, head: envAgentsText.slice(0, 120) },
  );
  const envSuggested = await tool("suggest_rule", {
    scope: "environment",
    project_path: envDir,
    agent_id: "harness-a",
    id: "wsl-tools",
    title: "Use Windows-native toolchains from WSL",
    content: "cargo via mise is callable from WSL for compile/verify.",
    kind: "constraint",
    rationale: "Observed the build toolchain lives on the Windows side.",
  });
  check(
    "suggest_rule scope=environment queues a pending environment suggestion",
    envSuggested.suggestion?.status === "pending" && envSuggested.suggestion?.scope === "environment",
    envSuggested,
  );
  const envQueue = await tool("list_suggestions", { project_path: envDir, status: "pending" });
  check(
    "environment suggestion appears in the project-filtered queue",
    envQueue.suggestions?.some((s) => s.rule_id === "wsl-tools" && s.scope === "environment"),
    envQueue,
  );
  check(
    "suggest_rule scope=environment rejects missing project_path",
    await toolRejects("suggest_rule", { scope: "environment", id: "x", title: "x", content: "x", agent_id: "a" }),
  );
  check(
    "save_rule scope=environment rejects missing project_path",
    await toolRejects("save_rule", { id: "x", title: "x", content: "x", agent_id: "a", scope: "environment" }),
  );
  const envToggled = await tool("set_rule_enabled", {
    id: "wsl-env", enabled: false, scope: "environment", project_path: envDir,
  });
  check(
    "set_rule_enabled scope=environment disables the local note",
    envToggled.rule?.enabled === false,
    envToggled,
  );

  // export_native: write a STATIC POINTER block into the selected native harness entry points. The
  // block points at the live rule files in .agent-control/rules/ — it does NOT copy rule text — so a
  // rule edit never makes it stale and re-export is byte-idempotent. Targets are opt-in; include_user
  // =false keeps every write inside the throwaway project dir (never the real ~/.claude/CLAUDE.md).
  const allTargets = ["agents", "claude", "gemini", "cursor"];
  const agentsPath = join(projectDir, "AGENTS.md");
  const claudePath = join(projectDir, "CLAUDE.md");
  const geminiPath = join(projectDir, "GEMINI.md");
  const cursorPath = join(projectDir, ".cursor", "rules", "agent-control.mdc");
  writeFileSync(agentsPath, "# Hand-written repo guide\n\nKeep this prose.\n");
  const exported = await tool("export_native", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  check(
    "export_native reports enabled project rules and the 4 selected targets",
    exported.report?.project_rules === 2 && exported.report?.targets?.length === 4,
    exported,
  );
  const agentsText = readFileSync(agentsPath, "utf8");
  check(
    "export_native preserves prose and injects a pointer block (not rule text)",
    agentsText.startsWith("# Hand-written repo guide\n\nKeep this prose.\n") &&
      agentsText.includes("agent-control:begin") &&
      agentsText.includes(".agent-control/rules/") &&
      !agentsText.includes("use-tabs") &&
      !agentsText.includes("api-isasc"),
    agentsText.slice(0, 200),
  );
  check(
    "export_native block embeds a format version and continuity guidance to latest.md",
    agentsText.includes("agent-control:block-format:") &&
      agentsText.includes("### Continuity") &&
      agentsText.includes(".agent-control/handoff/latest.md"),
    agentsText.slice(0, 400),
  );
  check(
    "export_native writes a Cursor rule with frontmatter outside the block",
    existsSync(cursorPath) && readFileSync(cursorPath, "utf8").startsWith("---\n"),
    cursorPath,
  );
  check(
    "export_native writes project CLAUDE.md and GEMINI.md when selected",
    existsSync(claudePath) && existsSync(geminiPath),
  );
  const reexported = await tool("export_native", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  check(
    "export_native re-export is idempotent (all targets unchanged)",
    reexported.report?.targets?.every((t) => t.action === "unchanged"),
    reexported,
  );
  check(
    "export_native rejects an empty project_path",
    await toolRejects("export_native", { project_path: "  ", include_user: false }),
  );
  check(
    "export_native rejects an unknown target",
    await toolRejects("export_native", { project_path: projectDir, targets: ["bogus"] }),
  );

  // Omitted targets default to AGENTS.md only — never write every supported harness at once.
  const defaultDir = join(root, "default-targets");
  mkdirSync(join(defaultDir, ".agent-control", "rules"), { recursive: true });
  const defaultExport = await tool("export_native", { project_path: defaultDir, include_user: false });
  check(
    "export_native defaults to AGENTS.md only (1 target, no CLAUDE/GEMINI)",
    defaultExport.report?.targets?.length === 1 &&
      defaultExport.report.targets[0].path.endsWith("AGENTS.md") &&
      !existsSync(join(defaultDir, "CLAUDE.md")) &&
      !existsSync(join(defaultDir, "GEMINI.md")),
    defaultExport,
  );

  // check_export_drift: detect a missing/outdated-FORMAT block (layer 1) without rewriting anything.
  // The pointer is static, so a rule edit must NOT induce drift; only a legacy v1 block does.
  const driftSync = await tool("check_export_drift", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  check(
    "check_export_drift reports in-sync right after export",
    driftSync.report?.stale === false && driftSync.report?.targets?.length === 4,
    driftSync,
  );
  check(
    "check_export_drift embeds a matching format version per target",
    driftSync.report?.targets?.every(
      (t) => t.exists && t.exported_format === t.current_format && t.current_format === 2,
    ),
    driftSync,
  );
  // Edit a rule WITHOUT re-exporting -> the pointer block is unchanged, so drift stays clear.
  await tool("save_rule", {
    id: "api-isasc",
    title: "Upstream isAsc type is ambiguous",
    content: "Edited, but a static pointer block must not go stale.",
    agent_id: "harness-b",
    scope: "project",
    project_path: projectDir,
  });
  const driftAfterEdit = await tool("check_export_drift", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  check(
    "check_export_drift stays in sync after a rule edit (pointer is static)",
    driftAfterEdit.report?.stale === false,
    driftAfterEdit,
  );
  // Overwrite AGENTS.md with a legacy v1 full-text block (ruleset hash, no format marker) -> stale.
  writeFileSync(
    agentsPath,
    "# Hand-written repo guide\n\nKeep this prose.\n\n<!-- agent-control:begin -->\n" +
      "<!-- agent-control:ruleset sha256:deadbeef -->\n## Contracts\n\nold full text\n" +
      "<!-- agent-control:end -->\n",
  );
  const driftLegacy = await tool("check_export_drift", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  const legacyAgents = driftLegacy.report?.targets?.find((t) => t.path.endsWith("AGENTS.md"));
  check(
    "check_export_drift flags a legacy v1 block as stale (no format marker)",
    driftLegacy.report?.stale === true &&
      legacyAgents?.stale === true &&
      legacyAgents?.exported_format === null,
    driftLegacy,
  );
  await tool("export_native", { project_path: projectDir, targets: allTargets, include_user: false });
  const driftResync = await tool("check_export_drift", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  check("check_export_drift clears after re-export", driftResync.report?.stale === false, driftResync);
  check(
    "check_export_drift rejects an empty project_path",
    await toolRejects("check_export_drift", { project_path: "  ", include_user: false }),
  );

  // UX9 inline opt-in: export_native inline=true embeds enabled contract/constraint bodies plus a
  // ruleset hash; a rule edit then DOES induce drift until re-export. inline=false restores the
  // static pointer (byte-idempotent again).
  const inlineExport = await tool("export_native", {
    project_path: projectDir,
    targets: ["agents"],
    include_user: false,
    inline: true,
  });
  check(
    "export_native inline=true writes the inline snapshot",
    inlineExport.report?.targets?.length === 1 &&
      ["created", "updated"].includes(inlineExport.report?.targets?.[0]?.action),
    inlineExport,
  );
  const inlineText = readFileSync(agentsPath, "utf8");
  check(
    "inline block carries the mode marker, ruleset hash and contract body",
    inlineText.includes("<!-- agent-control:mode:inline -->") &&
      inlineText.includes("<!-- agent-control:ruleset:") &&
      inlineText.includes("Edited, but a static pointer block must not go stale."),
    inlineText.slice(0, 400),
  );
  const inlineSync = await tool("check_export_drift", {
    project_path: projectDir,
    targets: ["agents"],
    include_user: false,
  });
  check(
    "check_export_drift in-sync right after inline export (inline flag set)",
    inlineSync.report?.stale === false && inlineSync.report?.targets?.[0]?.inline === true,
    inlineSync,
  );
  await tool("save_rule", {
    id: "api-isasc",
    title: "Upstream isAsc type is ambiguous",
    content: "Inline snapshot must go stale after this edit.",
    agent_id: "harness-b",
    scope: "project",
    project_path: projectDir,
  });
  const inlineDrift = await tool("check_export_drift", {
    project_path: projectDir,
    targets: ["agents"],
    include_user: false,
  });
  const inlineTarget = inlineDrift.report?.targets?.[0];
  check(
    "check_export_drift flags inline ruleset drift after a rule edit",
    inlineDrift.report?.stale === true &&
      inlineTarget?.inline === true &&
      inlineTarget?.exported_ruleset !== inlineTarget?.current_ruleset,
    inlineDrift,
  );
  await tool("export_native", {
    project_path: projectDir,
    targets: ["agents"],
    include_user: false,
    inline: true,
  });
  const inlineResync = await tool("check_export_drift", {
    project_path: projectDir,
    targets: ["agents"],
    include_user: false,
  });
  check(
    "inline drift clears after re-export",
    inlineResync.report?.stale === false &&
      readFileSync(agentsPath, "utf8").includes("Inline snapshot must go stale after this edit."),
    inlineResync,
  );
  await tool("export_native", {
    project_path: projectDir,
    targets: ["agents"],
    include_user: false,
    inline: false,
  });
  const pointerText = readFileSync(agentsPath, "utf8");
  check(
    "inline=false restores the static pointer (no mode marker, no rule text)",
    !pointerText.includes("<!-- agent-control:mode:inline -->") &&
      !pointerText.includes("Inline snapshot must go stale after this edit."),
    pointerText.slice(0, 400),
  );

  // revoke_native: the explicit undo. AGENTS.md keeps its prose (removed); the block-only CLAUDE.md,
  // GEMINI.md and Cursor rule (frontmatter + block) are deleted. Re-running is a safe no-op.
  const revoked = await tool("revoke_native", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  const byPath = (p) => revoked.report?.targets?.find((t) => t.path.endsWith(p))?.action;
  check(
    "revoke_native removes the block but keeps prose, deletes block-only files",
    byPath("AGENTS.md") === "removed" &&
      byPath("CLAUDE.md") === "deleted" &&
      byPath("GEMINI.md") === "deleted" &&
      byPath("agent-control.mdc") === "deleted",
    revoked,
  );
  const revokedAgents = readFileSync(agentsPath, "utf8");
  check(
    "revoke_native leaves AGENTS.md prose intact and block-free",
    revokedAgents.includes("# Hand-written repo guide") &&
      revokedAgents.includes("Keep this prose.") &&
      !revokedAgents.includes("agent-control:begin") &&
      !existsSync(claudePath) &&
      !existsSync(geminiPath) &&
      !existsSync(cursorPath),
    revokedAgents,
  );
  const rerevoked = await tool("revoke_native", {
    project_path: projectDir,
    targets: allTargets,
    include_user: false,
  });
  check(
    "revoke_native re-run is a no-op (all targets unchanged)",
    rerevoked.report?.targets?.every((t) => t.action === "unchanged"),
    rerevoked,
  );
  check(
    "revoke_native rejects an empty project_path",
    await toolRejects("revoke_native", { project_path: "  ", include_user: false }),
  );

  // enforcement + code_checks passthrough on save_rule, then verify_constraints achieves a tier.
  const verifiedRule = await tool("save_rule", {
    id: "editorconfig-space",
    title: "Editorconfig uses spaces",
    content: "The repo .editorconfig sets indent_style = space; keep it consistent.",
    agent_id: "harness-b",
    kind: "contract",
    scope: "project",
    project_path: projectDir,
    enforcement: "verify",
    code_checks: [{ file: ".editorconfig", key: "indent_style", expect: "space" }],
  });
  check(
    "save_rule persists enforcement=verify and code_checks",
    verifiedRule.rule?.enforcement === "verify" && verifiedRule.rule?.code_checks?.length === 1,
    verifiedRule,
  );
  check(
    "save_rule rejects unknown enforcement",
    await toolRejects("save_rule", { id: "x", title: "x", content: "x", agent_id: "a", scope: "user", enforcement: "bogus" }),
  );

  const session2 = await tool("create_session", {
    task: "e2e: verify constraints",
    project_path: projectDir,
    agent_id: "harness-a",
  });
  const sid2 = session2.session_id;
  const vc = await tool("verify_constraints", { session_id: sid2, agent_id: "harness-a" });
  const byRule = (id) => vc.assurances?.find((a) => a.rule_id === id);
  check(
    "verify_constraints achieves Verified for a passing code_check",
    byRule("editorconfig-space")?.achieved === "verified" && byRule("editorconfig-space")?.declared === "verify",
    vc,
  );
  check(
    "verify_constraints caps a failing check at Advisory (use-tabs expects tab, repo is space)",
    byRule("use-tabs")?.achieved === "advisory",
    vc,
  );
  check(
    "verify_constraints caps a semantic rule at Advisory",
    byRule("prefer-concise")?.achieved === "advisory",
    vc,
  );
  check("verify_constraints counts verified >= 1 and enforced == 0", vc.verified >= 1 && vc.enforced === 0, vc);
  const vcChain = await tool("verify_evidence", { session_id: sid2 });
  check("evidence chain still valid after verify_constraints", vcChain.valid === true, vcChain);

  // A hard-declared suggestion still only reaches Advisory once approved without a chokepoint;
  // here we just confirm the declared intent round-trips through the review queue.
  const hardSuggested = await tool("suggest_rule", {
    project_path: projectDir,
    agent_id: "harness-b",
    id: "hard-semantic",
    title: "A hard-declared semantic rule",
    content: "Never push to main without review.",
    kind: "constraint",
    enforcement: "hard",
    rationale: "Testing enforcement passthrough on suggestions.",
  });
  check(
    "suggest_rule persists enforcement=hard",
    hardSuggested.suggestion?.enforcement === "hard",
    hardSuggested,
  );

  // Handoff carries the achieved assurance baseline and warns the new harness to re-verify.
  const bundle2 = await tool("export_handoff", {
    session_id: sid2,
    to_agent: "harness-c",
    context: { summary: "verified constraints" },
  });
  check(
    "export_handoff carries assurance baseline",
    Array.isArray(bundle2.assurance) && bundle2.assurance.some((a) => a.achieved === "verified"),
    bundle2,
  );
  const resumed2 = await tool("import_handoff", { bundle: bundle2, new_agent: "harness-c" });
  check(
    "import_handoff carries assurance and warns to re-verify (warn-not-block)",
    resumed2.agent_id === "harness-c" &&
      Array.isArray(resumed2.carried_assurance) &&
      typeof resumed2.tier_warning === "string" && resumed2.tier_warning.length > 0,
    resumed2,
  );
  check(
    "verify_constraints rejects an unknown session",
    await toolRejects("verify_constraints", { session_id: "00000000-0000-0000-0000-000000000000", agent_id: "a" }),
  );

  // Session lifecycle: close/reopen via set_session_status; handed_off stays export-only.
  const lifeSession = await tool("create_session", {
    task: "e2e: lifecycle",
    project_path: projectDir,
    agent_id: "harness-a",
  });
  const closed = await tool("set_session_status", {
    session_id: lifeSession.session_id,
    status: "completed",
  });
  check("set_session_status completes", closed.status === "completed", closed);
  const lifeChain = await tool("verify_evidence", { session_id: lifeSession.session_id });
  check("session_end evidence keeps chain valid", lifeChain.valid === true, lifeChain);
  const reopened = await tool("set_session_status", {
    session_id: lifeSession.session_id,
    status: "active",
  });
  check("set_session_status reopens", reopened.status === "active", reopened);
  check(
    "set_session_status rejects handed_off",
    await toolRejects("set_session_status", { session_id: lifeSession.session_id, status: "handed_off" }),
  );
  check(
    "set_session_status rejects unknown status",
    await toolRejects("set_session_status", { session_id: lifeSession.session_id, status: "bogus" }),
  );

  // ---- Layer 2: the deny-capable PreToolUse gate (check_tool_call) ----
  // A hard rule's tool_guards deny matching calls; its code_checks form a repo gate that denies while
  // failing; a mediated-and-allowed hard rule reaches Enforced and records ToolGate evidence. Every
  // write stays inside throwaway dirs — never the real repo or ~/.claude.
  const guardDir = join(root, "gate-guard");
  mkdirSync(join(guardDir, ".agent-control", "rules"), { recursive: true });
  writeFileSync(join(guardDir, ".editorconfig"), "[*]\nindent_style = space\n");
  const guardRule = await tool("save_rule", {
    id: "no-rm-rf", title: "No recursive force delete", content: "Never run rm -rf.",
    agent_id: "harness-a", kind: "constraint", scope: "project", project_path: guardDir,
    enforcement: "hard",
    code_checks: [{ file: ".editorconfig", key: "indent_style", expect: "space" }],
    tool_guards: [{ tool: "Bash", pattern: "rm\\s+-rf", reason: "recursive force delete is banned" }],
  });
  check(
    "save_rule persists enforcement=hard and tool_guards",
    guardRule.rule?.enforcement === "hard" && guardRule.rule?.tool_guards?.length === 1,
    guardRule,
  );

  const guardSession = await tool("create_session", { task: "e2e: gate", project_path: guardDir, agent_id: "harness-a" });
  const gsid = guardSession.session_id;
  const gateArgs = { project_path: guardDir, agent_id: "harness-a", session_id: gsid };

  const denied = await tool("check_tool_call", { ...gateArgs, tool_name: "Bash", tool_input: { command: "rm -rf /tmp/x" } });
  check(
    "check_tool_call denies a matching tool_guard",
    denied.report?.allowed === false && denied.report?.denials?.some((d) => d.source === "tool_guard"),
    denied,
  );
  check(
    "check_tool_call surfaces a denial_reason for hook feedback",
    typeof denied.denial_reason === "string" && denied.denial_reason.includes("recursive force delete"),
    denied,
  );

  const allowedCall = await tool("check_tool_call", { ...gateArgs, tool_name: "Bash", tool_input: { command: "echo hi" } });
  check(
    "check_tool_call allows a non-matching command and lists the enforced hard rule",
    allowedCall.report?.allowed === true && allowedCall.report?.enforced_rules?.includes("no-rm-rf"),
    allowedCall,
  );

  const otherTool = await tool("check_tool_call", { ...gateArgs, tool_name: "Read", tool_input: { file_path: "x" } });
  check("check_tool_call tool glob narrows to Bash (Read allowed)", otherTool.report?.allowed === true, otherTool);

  const gateCtx = await tool("get_session_context", { session_id: gsid });
  check(
    "gate marks the mediated hard rule Enforced in session assurance",
    gateCtx.session?.assurance?.some((a) => a.rule_id === "no-rm-rf" && a.achieved === "enforced"),
    gateCtx,
  );
  const gateChain = await tool("verify_evidence", { session_id: gsid });
  check("evidence chain valid after gate calls", gateChain.valid === true, gateChain);

  // Repo gate: a hard rule whose code_check FAILS denies every tool until the repo is fixed.
  const repoDir = join(root, "gate-repo");
  mkdirSync(join(repoDir, ".agent-control", "rules"), { recursive: true });
  writeFileSync(join(repoDir, ".editorconfig"), "[*]\nindent_style = space\n");
  await tool("save_rule", {
    id: "must-use-tabs", title: "Indent with tabs", content: "Tabs required.",
    agent_id: "harness-a", kind: "constraint", scope: "project", project_path: repoDir,
    enforcement: "hard",
    code_checks: [{ file: ".editorconfig", key: "indent_style", expect: "tab" }],
  });
  const repoDenied = await tool("check_tool_call", { project_path: repoDir, tool_name: "Bash", tool_input: { command: "echo hi" }, agent_id: "harness-a" });
  check(
    "check_tool_call repo gate denies while a hard code_check fails",
    repoDenied.report?.allowed === false && repoDenied.report?.denials?.some((d) => d.source === "repo_gate"),
    repoDenied,
  );
  writeFileSync(join(repoDir, ".editorconfig"), "[*]\nindent_style = tab\n");
  const repoAllowed = await tool("check_tool_call", { project_path: repoDir, tool_name: "Bash", tool_input: { command: "echo hi" }, agent_id: "harness-a" });
  check(
    "check_tool_call repo gate allows once the hard check passes",
    repoAllowed.report?.allowed === true && repoAllowed.report?.enforced_rules?.includes("must-use-tabs"),
    repoAllowed,
  );

  // No enabled hard rules -> fail-open (allowed, nothing to enforce).
  const openDir = join(root, "gate-open");
  mkdirSync(join(openDir, ".agent-control", "rules"), { recursive: true });
  const openCall = await tool("check_tool_call", { project_path: openDir, tool_name: "Bash", tool_input: { command: "rm -rf /" }, agent_id: "harness-a" });
  check(
    "check_tool_call allows when no hard rules exist",
    openCall.report?.allowed === true && openCall.report?.hard_rules_checked === 0,
    openCall,
  );
  check(
    "check_tool_call rejects an empty project_path",
    await toolRejects("check_tool_call", { project_path: "  ", tool_name: "Bash", agent_id: "a" }),
  );

  // ---- Layer 2: hooks registration (register_hooks / detect_hooks / revoke_hooks) ----
  const hooksDir = join(root, "hooks-project");
  mkdirSync(join(hooksDir, ".agent-control", "rules"), { recursive: true });
  mkdirSync(join(hooksDir, ".claude"), { recursive: true });
  const claudeSettings = join(hooksDir, ".claude", "settings.json");
  writeFileSync(claudeSettings, JSON.stringify({
    model: "opus",
    hooks: { PreToolUse: [{ matcher: "Bash", hooks: [{ type: "command", command: "echo user" }] }] },
  }));
  const hookCmd = 'node "C:/throwaway/agent-control-hook.mjs"';
  // The seven project-scope targets write inside the throwaway project dir (qoder + qoder-cn share
  // .qoder/settings.json, so seven targets yield six files). Kimi + Antigravity are user-scope (they
  // resolve to the real home config) and are covered by the core codec unit tests — never written
  // here, so this e2e stays fully isolated.
  const hookTargets = ["claude", "codebuddy", "workbuddy", "qoder", "qoder-cn", "codex", "cursor"];
  const registered = await tool("register_hooks", { project_path: hooksDir, targets: hookTargets, command: hookCmd });
  check(
    "register_hooks writes the six project-scope config files (qoder + qoder-cn share one)",
    registered.report?.targets?.length === 6 && registered.command === hookCmd,
    registered,
  );
  const claudeJson = JSON.parse(readFileSync(claudeSettings, "utf8"));
  check(
    "register_hooks preserves other config and the user's hook",
    claudeJson.model === "opus" &&
      claudeJson.hooks.PreToolUse.some((g) => g.hooks?.[0]?.command === "echo user") &&
      claudeJson.hooks.PreToolUse.some((g) => g.hooks?.[0]?.command?.includes("agent-control-hook.mjs")) &&
      Array.isArray(claudeJson.hooks.SessionStart),
    claudeJson,
  );
  check(
    "register_hooks creates the nested-JSON stubs (codebuddy/workbuddy/qoder/codex)",
    existsSync(join(hooksDir, ".codebuddy", "settings.json")) &&
      existsSync(join(hooksDir, ".workbuddy", "settings.json")) &&
      existsSync(join(hooksDir, ".qoder", "settings.json")) &&
      existsSync(join(hooksDir, ".codex", "hooks.json")),
  );
  // Qoder CN shares the international build's project file: no separate .qoder-cn dir is written,
  // and the shared entry carries the first requested target's vendor id (qoder).
  const qoderJson = JSON.parse(readFileSync(join(hooksDir, ".qoder", "settings.json"), "utf8"));
  check(
    "register_hooks writes qoder + qoder-cn to the shared .qoder/settings.json once",
    !existsSync(join(hooksDir, ".qoder-cn", "settings.json")) &&
      qoderJson.hooks.PreToolUse.filter((g) => g.hooks?.[0]?.command?.includes("agent-control-hook.mjs")).length === 1 &&
      qoderJson.hooks.PreToolUse.some((g) => g.hooks?.[0]?.command?.endsWith("qoder PreToolUse")),
    qoderJson,
  );
  // Codex uses the same nested shape as Claude, in a dedicated hooks.json.
  const codexJson = JSON.parse(readFileSync(join(hooksDir, ".codex", "hooks.json"), "utf8"));
  check(
    "register_hooks writes Codex nested hooks.json (SessionStart + PreToolUse)",
    Array.isArray(codexJson.hooks?.SessionStart) &&
      Array.isArray(codexJson.hooks?.PreToolUse) &&
      codexJson.hooks.PreToolUse.some((g) => g.hooks?.[0]?.command?.includes("agent-control-hook.mjs")),
    codexJson,
  );
  // Cursor uses a flat version:1 schema with the command on the definition itself (an array per event).
  const cursorHooksPath = join(hooksDir, ".cursor", "hooks.json");
  const cursorJson = JSON.parse(readFileSync(cursorHooksPath, "utf8"));
  check(
    "register_hooks writes Cursor flat hooks.json (version:1, command on definition)",
    cursorJson.version === 1 &&
      Array.isArray(cursorJson.hooks?.sessionStart) &&
      cursorJson.hooks.sessionStart.some((d) => d.command?.includes("agent-control-hook.mjs")) &&
      Array.isArray(cursorJson.hooks?.preToolUse) &&
      cursorJson.hooks.preToolUse.some((d) => d.command?.includes("agent-control-hook.mjs")),
    cursorJson,
  );

  const detected = await tool("detect_hooks", { project_path: hooksDir });
  const detectedProject = (detected.harnesses ?? []).filter((h) => h.scope === "project");
  const detectedUser = (detected.harnesses ?? []).filter((h) => h.scope === "user");
  check(
    "detect_hooks reports all nine harnesses with scope (7 project + 2 user)",
    detected.harnesses?.length === 9 && detectedProject.length === 7 && detectedUser.length === 2,
    detected,
  );
  check(
    "detect_hooks reports the seven project-scope targets registered",
    detectedProject.every((h) => h.registered),
    detectedProject,
  );
  check(
    "detect_hooks marks kimi + antigravity as user-scope",
    detectedUser.map((h) => h.target).sort().join(",") === "antigravity,kimi",
    detectedUser,
  );

  // ---- doctor: read-only aggregated diagnostics ----
  const doc = await tool("doctor", { project_path: projectDir, daemon_bind: "127.0.0.1:1" });
  check(
    "doctor aggregates chains, harnesses and daemon probe",
    doc.sessions_checked >= 1 &&
      doc.chains?.length === doc.sessions_checked &&
      doc.chains.every((c) => c.valid) &&
      doc.harnesses?.length === 9 &&
      doc.daemon?.bind === "127.0.0.1:1" &&
      doc.daemon.reachable === false &&
      typeof doc.healthy === "boolean" &&
      typeof doc.generated_at === "string",
    doc,
  );
  const docHooks = await tool("doctor", { project_path: hooksDir, daemon_bind: "127.0.0.1:1" });
  check(
    "doctor counts the seven registered project-scope hooks and reports healthy",
    docHooks.hooks_registered === 7 && docHooks.stale_exports === 0 && docHooks.healthy === true,
    docHooks,
  );

  // ---- P1: full-text search with CJK bigram segmentation ----
  await tool("save_rule", {
    id: "zh-search-rule",
    title: "文档腐化检测",
    content: "登记文档声明并求值",
    scope: "project",
    kind: "contract",
    project_path: projectDir,
    agent_id: "harness-a",
  });
  const zhSession = await tool("create_session", {
    task: "中文搜索任务",
    project_path: projectDir,
    agent_id: "harness-a",
  });
  await tool("update_work_state", {
    session_id: zhSession.session_id,
    goal: "实现全文搜索",
    work_items: [{ title: "分词器", status: "done" }],
  });
  await tool("record_evidence", {
    session_id: zhSession.session_id,
    agent_id: "harness-a",
    kind: "tool_call",
    payload: { note: "中文证据" },
  });
  const zhHits = await tool("search", { query: "腐化" });
  check(
    "search matches a two-char Chinese query on rule titles",
    zhHits.hits?.some((h) => h.kind === "rules" && h.id === "zh-search-rule"),
    zhHits,
  );
  const sessHits = await tool("search", { query: "分词", kind: "sessions" });
  check(
    "search matches Chinese work items in sessions",
    sessHits.hits?.some((h) => h.id === zhSession.session_id),
    sessHits,
  );
  const evHits = await tool("search", { query: "证据", kind: "evidence" });
  check("search matches Chinese evidence payloads", evHits.count >= 1, evHits);
  const filtered = await tool("search", { query: "腐化", kind: "rules", project_path: projectDir });
  check(
    "search project filter keeps the project's rule",
    filtered.count === 1 && filtered.hits[0].id === "zh-search-rule",
    filtered,
  );
  const elsewhere = await tool("search", { query: "腐化", kind: "rules", project_path: hooksDir });
  check("search project filter excludes other projects", elsewhere.count === 0, elsewhere);
  check("search empty query returns nothing", (await tool("search", { query: "  " })).count === 0);
  check(
    "search rejects an unknown kind",
    await toolRejects("search", { query: "x", kind: "bogus" }),
  );

  // ---- P3: one-command setup (export_native + register_hooks composition) ----
  const setupDir = mkdtempSync(join(tmpdir(), "ac-setup-"));
  const setup1 = await tool("setup", { project_path: setupDir, targets: hookTargets });
  check(
    "setup writes the pointer block and registers the project-scope hooks",
    setup1.export?.targets?.some((t) => t.action === "created") &&
      setup1.hooks?.targets?.length === 6 &&
      setup1.hooks.targets.every((t) => t.action === "created") &&
      typeof setup1.command === "string" &&
      setup1.command.includes("agent-control-hook.mjs"),
    setup1,
  );
  const setup2 = await tool("setup", { project_path: setupDir, targets: hookTargets });
  check(
    "setup re-run is idempotent (all unchanged)",
    setup2.export?.targets?.every((t) => t.action === "unchanged") &&
      setup2.hooks?.targets?.every((t) => t.action === "unchanged"),
    setup2,
  );
  await tool("revoke_hooks", { project_path: setupDir, targets: hookTargets });
  await tool("revoke_native", { project_path: setupDir });

  const reregistered = await tool("register_hooks", { project_path: hooksDir, targets: hookTargets, command: hookCmd });
  check(
    "register_hooks re-run is idempotent (all unchanged)",
    reregistered.report?.targets?.every((t) => t.action === "unchanged"),
    reregistered,
  );

  await tool("revoke_hooks", { project_path: hooksDir, targets: hookTargets });
  const claudeAfter = JSON.parse(readFileSync(claudeSettings, "utf8"));
  check(
    "revoke_hooks keeps the user's config and hook, removes ours",
    claudeAfter.model === "opus" &&
      claudeAfter.hooks.PreToolUse.length === 1 &&
      claudeAfter.hooks.PreToolUse[0].hooks[0].command === "echo user" &&
      claudeAfter.hooks.SessionStart === undefined,
    claudeAfter,
  );
  check(
    "revoke_hooks deletes configs that held only our hook",
    !existsSync(join(hooksDir, ".codebuddy", "settings.json")) &&
      !existsSync(join(hooksDir, ".workbuddy", "settings.json")) &&
      !existsSync(join(hooksDir, ".qoder", "settings.json")) &&
      !existsSync(join(hooksDir, ".qoder-cn", "settings.json")) &&
      !existsSync(join(hooksDir, ".codex", "hooks.json")) &&
      !existsSync(cursorHooksPath),
  );
  const detectedAfter = await tool("detect_hooks", { project_path: hooksDir });
  check(
    "detect_hooks reports the project-scope targets unregistered after revoke",
    (detectedAfter.harnesses ?? []).filter((h) => h.scope === "project").every((h) => !h.registered),
    detectedAfter,
  );
  check(
    "register_hooks rejects an unknown target",
    await toolRejects("register_hooks", { project_path: hooksDir, targets: ["bogus"], command: hookCmd }),
  );
  check(
    "register_hooks rejects an empty project_path",
    await toolRejects("register_hooks", { project_path: "  ", command: hookCmd }),
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