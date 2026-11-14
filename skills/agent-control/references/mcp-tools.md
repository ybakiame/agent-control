# MCP tool reference

Server name: `agent-control`. Transports: stdio (`agent-control-mcp --stdio`) and Streamable HTTP
(`http://127.0.0.1:9870/mcp`). All tools return JSON (both `content[0].text` and
`structuredContent`). Errors return `isError: true` with a message. UUIDs are strings.

## Tools

### get_applicable_rules
Resolve binding rules + provenance + conflicts before working.
- `project_path` (string, required) — absolute project path.
- `agent_id` (string, required) — your harness id.
- `file_paths` (string[], default `[]`) — task-relevant paths relative to project root.
- `languages` (string[], default `[]`), `task_tags` (string[], default `[]`).
- Returns: `provenance_id`, `applied_rules[]` (each with `decision`, `rule`, provenance trace),
  `conflicts[]`, `briefing` (rendered by core — binding rules + conflicts + a **work environment**
  section for this machine/project; the same text the
  SessionStart hook injects and the `constraint-briefing` prompt returns), `instruction`. Resolve
  conflicts with the user before proceeding.

### explain_rule
Why a rule exists and how it was decided (source, scope precedence, priority, superseded
alternatives, conflicts).
- `rule_id` (string, required), `project_path` (string, required), `agent_id` (string, required),
  optional `file_paths` / `languages` / `task_tags`.
- Returns the full explanation object; errors if the rule is not found.

### check_conflicts
Conflict-only view (rule-vs-rule and rule-vs-code, e.g. rule expects tabs but `.editorconfig`
says spaces). Same parameters as `get_applicable_rules`.
- Returns: `conflicts[]`, `has_conflicts` (bool).

### create_session
Freeze a constraint snapshot for a unit of work. **Idempotent per (project_path, agent_id, task):**
an existing **active** session with the same triple is returned unchanged (`reused: true`) instead
of creating a duplicate, so repeated calls from one harness conversation never pile up sessions.
A new task — or a closed (completed/failed/handed-off) source session — freezes a fresh snapshot.
- `task` (string, required), `project_path` (string, required), `agent_id` (string, required),
  `task_tags` (string[], default `[]`).
- Returns: `session_id` (UUID), `reused` (bool), `snapshot_hash`, `rules[]`, `instruction`.
  Acknowledge these rules before making changes.

### acknowledge_rules
Commit to the snapshot's rules; server compares against the frozen snapshot.
- `session_id` (UUID string, required), `agent_id` (string, required), `rule_ids` (string[],
  required) — the exact ids from `create_session.rules`.
- Returns an acknowledgment report listing `missing` / `unexpected` rules. Required after session
  start and after every handoff.

### record_evidence
Append a tamper-evident entry to the session hash chain.
- `session_id` (UUID string, required), `agent_id` (string, required).
- `kind` (string, required) — one of: `rule_acknowledgment`, `tool_call`, `tool_gate`,
  `file_change`, `test_result`, `handoff_out`, `handoff_in`, `constraint_violation`, `session_start`,
  `session_end`.
- `payload` (object, required) — arbitrary JSON. For `file_change` include
  `{ "file": "...", "before_sha256": "<64 hex>", "after_sha256": "<64 hex>" }`.
- Returns the entry: `seq` (>= 1), `entry_hash`, `prev_hash`, `kind`, `payload`, timestamps.

### get_session_context
Resume work started by another harness.
- `session_id` (UUID string, required).
- Returns: `session` (task, constraint snapshot, current `agent_id`, `handoffs[]`, status, `goal`,
  `work_items[]`) and `evidence_head_hash`.

### update_work_state
Record the session's resumable **work state** so a handoff carries progress instead of relying on the
next harness's own todo list.
- `session_id` (UUID string, required).
- `goal` (string, default `""`) — the objective (the `/goal`) the session is working toward.
- `work_items` (object[], default `[]`) — each `{ "title": string, "status": "done"|"doing"|"todo" }`
  (`status` defaults to `todo`). **Replaces** the session's current goal + list.
- Returns the updated `session` (including `goal` and `work_items[]`). Call it as work completes or
  starts, and before `export_handoff`; the goal + items then ride into the bundle and `latest.md`.

### set_session_status
Close or reopen a session so finished work leaves the active list instead of piling up forever.
- `session_id` (UUID string, required).
- `status` (string, required) — `active` | `completed` | `failed`. `handed_off` is rejected here;
  only `export_handoff` sets it.
- Returns the updated `session`. Completing/failing appends a `session_end` evidence entry (the
  chain stays auditable and is never deleted); `active` reopens it. A completed session no longer
  absorbs `create_session` calls for the same task.

### export_handoff
Produce a bundle to transfer the session to another harness.
- `session_id` (UUID string, required), `to_agent` (string, **optional**) — a receiver hint recorded
  in the handoff history; the real receiver chooses itself at `import_handoff` (`new_agent`), so
  omit it when unknown. `context` (object, default `{}`) — carry-over summary (conversation state,
  files touched, decisions).
- Returns the bundle (includes `bundle_hash`, frozen constraints, evidence head, the source
  session's achieved `assurance[]` tiers as a baseline, and the session's `goal` + `work_items[]`).
- **Dual write.** Besides the JSON bundle at `<data root>/bundles/<session_id>.json`, it also writes a
  readable continuity brief to `<project>/.agent-control/handoff/latest.md` (task, from-harness,
  session id, evidence-chain head, bundle hash, frozen constraints, assurance tiers, plus a **Goal**
  and a **Work state** section listing done/doing/todo items). This is the
  **file-based reach** counterpart of the bundle: the `AGENTS.md` managed block points agents at
  `latest.md`, so a harness that never calls MCP still inherits continuity by reading a file. Calling
  `export_handoff` again refreshes `latest.md` in place.

### import_handoff
Resume a session under a new harness; verifies bundle integrity + snapshot consistency.
- `bundle` (object, required) — the full JSON from `export_handoff`.
- `new_agent` (string, required).
- Returns: `session_id`, `agent_id`, `rules_to_acknowledge[]`, `carried_assurance[]` (the source
  tiers), `goal` + `work_items[]` (the carried work state) and `tier_warning` (set when any carried
  tier is above Advisory). The warning is
  **warn-not-block**: the new harness must re-run `verify_constraints` to re-earn the tiers — they do
  not transfer automatically. Call `acknowledge_rules` next.
- **Refreshes `latest.md`.** Like `export_handoff`, it rewrites
  `<project>/.agent-control/handoff/latest.md` — now in the *resumed* state: it names the new harness
  (`Resumed under`), keeps the from-harness and carried tiers, and appends the re-verify warning. So
  the file-based reach path stays consistent with the JSON bundle after a resume, not stale.

### verify_evidence
Recompute every entry hash; detect tampering or gaps.
- `session_id` (UUID string, required).
- Returns a report with `valid` (bool) and any broken/gapped entries.

### verify_constraints
Evaluate the frozen snapshot's `code_checks` against the **real repo** and record the **achieved
assurance tier** per rule. Distinct from `verify_evidence`: this asks *did the repo satisfy the
checks?*, not *is the record intact?*.
- `session_id` (UUID string, required), `agent_id` (string, required).
- Tier logic: no `code_checks` → `Advisory` (semantic rule); all checks pass → `Verified`; any
  fail/inconclusive → `Advisory`. `Enforced` is **never** assigned here — it requires a deny-capable
  chokepoint (the PreToolUse hook calling `check_tool_call`), so a rule declaring `hard` still caps at
  Advisory/Verified until the hook is registered.
- Persists `Session.assurance[]` (`{ rule_id, declared, achieved, basis }`) beside the frozen snapshot
  (the `snapshot_hash` over rules is unchanged) and appends a `test_result` evidence entry (checks +
  assurance), plus a `constraint_violation` entry when any check failed.
- Returns: `ConstraintVerificationReport` — `session_id`, `agent_id`, `assurances[]`, and `verified` /
  `advisory` / `enforced` counts.
- Errors (`isError`): unknown `session_id`.

### list_rules
List stored rules (user preferences always; project contracts **and environment notes** when
`project_path` given).
- `project_path` (string, optional).
- Returns: `rules[]`, `count`.

### save_rule
Actively create or update a rule file — the way to record a **durable project contract** (a
personal preference, or a machine/project environment note) so it shows up in the console and binds future sessions. Use this for lasting
project knowledge; use `record_evidence` for session-scoped actions instead.
- `id` (string, required) — stable unique id (kebab-case recommended). Reusing an id **updates** the
  rule and bumps its `version`; it never creates a duplicate. A content update preserves the rule's
  `enabled` state — use `set_rule_enabled` to toggle it.
- `title` (string, required), `content` (string, required) — Markdown body stating the contract and
  how to satisfy it.
- `agent_id` (string, required) — recorded as the rule's `source.author` for provenance.
- `kind` (string, default `contract`) — `preference` | `contract` | `constraint`.
- `scope` (string, default `project`) — `user` | `project` | `environment`. `session` is rejected
  (snapshots only). `environment` is a machine/project-local note (git-ignored `local/`, briefing-only).
- `project_path` (string) — **required when `scope` is `project` or `environment`**; absolute project path.
- `topic` (string, optional), `tags` (string[], default `[]`).
- `enforcement` (string, default `advisory`) — declared intent: `advisory` (semantic, not mechanically
  checkable) | `verify` (carries `code_checks`) | `hard` (claims a deny-capable chokepoint). Declaring
  `hard` does not itself enforce; the achieved tier is computed separately by `verify_constraints`.
- `code_checks` (object[], default `[]`) — each `{ file, key, expect, description? }`, evaluated
  against real repo files by `verify_constraints` and `check_conflicts`.
- `tool_guards` (object[], default `[]`) — each `{ tool, pattern, reason }`, enforced at the PreToolUse
  gate when `enforcement: "hard"`. `tool` is a name glob (`Bash`, `Edit|Write`, `*` or empty for any);
  `pattern` is an optional regex matched against the JSON-serialized tool input (empty matches any);
  `reason` is surfaced to the agent on deny. Only `check_tool_call` consults these, never
  `verify_constraints`. Use them to make a hard rule deny specific dangerous calls (e.g. `rm -rf`).
- Returns: `path` (written file), `rule` (the saved rule with its final `version`), `instruction`.
  The file lands in `<project>/.agent-control/rules/<id>.md` (project),
  `<project>/.agent-control/local/<id>.md` (environment, git-ignored) or
  `~/.agent-control/rules/user/<id>.md` (user). Confirm with `get_applicable_rules` / `list_rules`.
- Errors (`isError`): empty id/title, unknown kind/scope, or project/environment scope without `project_path`.

### set_rule_enabled
Pause or resume a rule **without changing its content**. A disabled rule is excluded from
resolution (`Decision::Disabled`) and stops binding sessions; enabling restores it. This is the
only tool that flips `enabled` — a `save_rule` content update preserves the existing state, so
editing a paused rule never silently re-activates it. Prefer disabling over deleting when a
contract may apply again later.
- `id` (string, required) — the rule to toggle.
- `enabled` (bool, required) — `false` to disable, `true` to enable.
- `scope` (string, default `project`) — `user` | `project` | `environment`. `session` is rejected.
- `project_path` (string) — **required when `scope` is `project` or `environment`**; absolute project path.
- Returns: `rule` (the updated rule with its new `enabled` and bumped `version`), `instruction`.
- Errors (`isError`): unknown scope, project/environment scope without `project_path`, or an id that does not
  exist (it never creates a rule).

### suggest_rule
Propose a new rule for **human review** instead of writing it directly. The draft is queued
as a `pending` suggestion in the console's **Suggestions** view; a user reviews, edits, approves
(making it binding) or rejects it. **Prefer this over `save_rule` when the rule is
inferred, uncertain, or needs user confirmation.** A pending suggestion does **not** bind sessions.
- `scope` (string, default `project`) — `user` | `project` | `environment`. `user` proposes a **personal
  preference** that follows the user across all projects (approval writes `~/.agent-control/rules/user/<id>.md`);
  `project` proposes a project contract (approval writes `<project>/.agent-control/rules/<id>.md`);
  `environment` proposes a machine/project-local note (approval writes the git-ignored
  `<project>/.agent-control/local/<id>.md`, injected only via the briefing). `session` is rejected.
- `project_path` (string) — **required when `scope` is `project` or `environment`**; absolute project
  path. Omit for `scope: "user"`.
- `agent_id` (string, required) — recorded as the proposer for provenance.
- `id` (string, required) — proposed stable rule id (kebab-case recommended).
- `title` (string, required), `content` (string, required) — proposed Markdown body.
- `kind` (string, default `contract`) — `preference` | `contract` | `constraint`.
- `topic` (string, optional), `tags` (string[], default `[]`).
- `enforcement` (string, default `advisory`) — declared intent (`advisory` | `verify` | `hard`),
  carried into the rule on approval.
- `rationale` (string, optional but recommended) — why the rule is proposed (discovery context,
  evidence); shown to the human reviewer.
- Returns: `suggestion` (the queued draft with `status: "pending"`, `scope`, `id` UUID, `created_at`)
  and `instruction`. Continue working under the currently applicable rules until a user approves it.
- Errors (`isError`): empty id/title/content, project/environment scope without `project_path`, or unknown
  kind/scope.

> **Where do environment notes go?** Facts about *this user on this machine in this project* — e.g.
> "I run codex/kimi CLIs inside WSL; the repo lives in WSL, but Windows-native tools are callable for
> compile/verify" — are **environment-scope `constraint` rules**, not portable user preferences and not
> committed project contracts. They are true only here, so they live project-local and **git-ignored**
> (`.agent-control/local/`) and reach the agent **only** via the briefing — never the committed
> `AGENTS.md`. Propose one with `suggest_rule { scope: "environment", kind: "constraint", project_path }`
> (or record a confident one with `save_rule { scope: "environment", kind: "constraint", project_path }`).
> Environment notes always load when enabled, are frozen into every session snapshot, and are injected
> proactively via the SessionStart hook briefing and `get_applicable_rules` — so the agent is told up
> front instead of needing a reminder mid-task. They are non-competitive (never supersede a contract)
> and always `advisory`.

### list_suggestions
List rule suggestions submitted by agents; check whether your proposals were reviewed.
- `project_path` (string, optional) — filter to one project.
- `status` (string, optional) — `pending` | `approved` | `rejected`; omit for all.
- Returns: `suggestions[]`, `count`, `pending_count`. An approved suggestion carries
  `approved_rule_id` (the binding rule it became) and `reviewed_at`; a rejected one carries
  `review_note` and `reviewed_at`.
- Errors (`isError`): unknown status.

### export_native
Write a static **pointer block** into the native entry points you select so a harness that never
calls MCP still learns where the contracts live. This is **reach** (Advisory tier), not verification
or enforcement — the block directs the agent to read the live rules like any AGENTS.md.
`verify_constraints` earns `Verified`; only a deny-capable chokepoint (the PreToolUse hook calling
`check_tool_call`) earns `Enforced`.
- `project_path` (string, required) — absolute project path.
- `targets` (string[], optional) — which entry points to write: `agents` | `claude` | `gemini` |
  `cursor` (the bare key or the file name, case-insensitive). **Omitted → `["agents"]` only.**
  Writing every harness at once is opt-in because Claude Code reads only `CLAUDE.md` when both it and
  `AGENTS.md` exist. Unknown names are an error; duplicates are collapsed.
- `include_user` (bool, default `false`) — also write a **user** preferences pointer to
  `~/.claude/CLAUDE.md` (global, follows the user). Leave `false` for a project-only export so nothing
  outside the repo is touched.
- Writes an in-place **managed block** delimited by `<!-- agent-control:begin -->` /
  `<!-- agent-control:end -->`. The body is a constant, **discovery-first** pointer (`PROJECT_GUIDANCE`):
  it opens with "understand this project before changing it" and tells the
  agent to **read and summarize the enabled** `contract`/`constraint` rules in `.agent-control/rules/`
  (priority wins, `applies_to` narrows, skip `enabled: false`), preferences in
  `~/.agent-control/rules/user/`, and to
  prefer MCP `get_applicable_rules` for precise resolution. It **excludes** the git-ignored environment
  notes (`.agent-control/local/`), which reach the agent only via the briefing/hook — never this
  committed file. It does **not** copy rule text, so the
  body never varies with the rule set: editing a rule never makes the block stale. Everything
  **outside** the block is preserved verbatim; the block is replaced on re-export and appended if
  absent. **Byte-idempotent**: re-exporting yields `unchanged` (no newline growth). Target files:
  - `agents` → `<project>/AGENTS.md`
  - `claude` → `<project>/CLAUDE.md`
  - `gemini` → `<project>/GEMINI.md`
  - `cursor` → `<project>/.cursor/rules/agent-control.mdc` (Cursor MDC frontmatter outside the block)
  - `include_user` → `~/.claude/CLAUDE.md` (personal-preferences pointer, `USER_GUIDANCE`)
  A lone begin/end marker (malformed block) is an error, not a silent overwrite.
- **Continuity guidance (optional, last).** The project pointer ends with a static
  `### Continuity (optional)` section telling the
  agent to read `<project>/.agent-control/handoff/latest.md` if it exists and that it may refresh
  that file via `export_handoff`, then resume with `import_handoff` + `acknowledge_rules` +
  `verify_constraints`. It is deliberately last (discovery comes first). The user-global preferences
  pointer omits it (`latest.md` is project-local).
- Returns: `report` (`ExportReport`: `project_path`, `project_rules` count, `user_rules` count,
  `targets[]` each `{ path, action }` where action is `created` | `updated` | `unchanged`) and an
  `instruction`.
- Errors (`isError`): empty `project_path`, an unknown `targets` name, or a malformed existing block.

### check_export_drift
Detect native entry points missing the **current-format** pointer block (**file-vs-store format**
drift). Read-only: it prompts a re-export and **never rewrites** files.
- `project_path` (string, required), `targets` (string[], optional, same selection as `export_native`,
  omitted → `["agents"]`), `include_user` (bool, default `false`).
- Each managed block embeds a **block-format version** (`<!-- agent-control:block-format:2 -->`). A
  target is `stale` when it exists and that version differs from the current `BLOCK_FORMAT` — e.g. a
  legacy v1 full-text block (which carried a ruleset hash, no version marker) reads as stale and
  should be migrated. Because the pointer is static, a **rule edit does not** make it stale.
- Returns: `report` (`DriftReport`: `project_path`, `stale` bool, `targets[]` each
  `{ path, exists, exported_format, current_format, stale }`) and an `instruction`. A never-exported
  file is **not** stale (`exists: false`).
- Scope limit: detects layer-1 (missing/older-format block) only — **not** whether an already-running
  session's injected context is stale (layer 2). When stale, re-run `export_native`.
- Errors (`isError`): empty `project_path`, an unknown `targets` name.

### revoke_native
The explicit undo of `export_native`: remove the pointer block from the selected entry points.
- `project_path` (string, required), `targets` (string[], optional, omitted → `["agents"]`),
  `include_user` (bool, default `false`) — same selection as `export_native`.
- Strips the managed block, **preserving surrounding prose** (`action: removed`). A file that held
  only the block — or only the generated Cursor frontmatter + block — is **deleted** (`action:
  deleted`). A target with no block (or absent) is left `unchanged`, so re-running is a safe no-op.
- Returns: `report` (`RevokeReport`: `project_path`, `targets[]` each `{ path, action }` where action
  is `removed` | `deleted` | `unchanged`) and an `instruction`.
- Errors (`isError`): empty `project_path`, an unknown `targets` name, or a malformed existing block.

### check_tool_call
The deny-capable **PreToolUse gate** — the only path that achieves the `Enforced` tier. The hook calls
this before each tool call; on deny it exits `2` with the reason on stderr so the harness blocks the
call. Evaluates the project's enabled `hard` rules two ways: a **repo gate** (a hard rule's
`code_checks` failing/inconclusive denies tools until the repo complies) and **tool guards** (a
matching `tool_guards` glob + input regex denies the call). With no `hard` rules it is fail-open.
- `project_path` (string, required) — absolute project path whose hard rules gate the call.
- `tool_name` (string, required) — the tool about to run (e.g. `Bash`, `Edit`, `Write`).
- `tool_input` (object, default `{}`) — the tool's arguments as JSON; matched against `tool_guards`.
- `agent_id` (string, required) — narrows rules via `applies_to.agents`; recorded in evidence.
- `session_id` (UUID string, optional) — when given, the verdict is appended to the evidence chain
  (`tool_gate`) and mediated hard rules are marked `Enforced`. Omit for a stateless pre-check (the
  gate still evaluates project rules).
- Returns: `report` (`ToolGateReport`: `allowed` bool, `tool_name`, `denials[]` each
  `{ rule_id, reason }`, `enforced_rules[]`, `hard_rules_checked`), `denial_reason` (concatenated
  reasons for hook feedback), `instruction`.
- Errors (`isError`): empty `project_path` or `tool_name`, malformed `session_id`.

### register_hooks
Register the Agent Control hook into the selected harnesses' **project `settings.json`** so the
harness itself drives enforcement. Writes a managed group with a **SessionStart** hook (injects the
briefing: binding rules + conflicts + work environment) and a **PreToolUse** hook (runs `check_tool_call`) — this is what turns a `hard`
rule into an `Enforced` one. Every other settings key and user-authored hook is preserved;
re-registering is idempotent.
- `project_path` (string, required) — absolute project path.
- `targets` (string[], optional) — `claude` | `codebuddy` | `workbuddy` | `qoder`. **Omitted →
  all four.** CodeBuddy and WorkBuddy are separate products (`.codebuddy/` vs `.workbuddy/`); each
  harness reads only its own `settings.json`, so there is no cross-harness conflict.
- `command` (string, optional) — full command line that runs the hook (e.g.
  `node "/abs/agent-control-hook.mjs"`). Omitted resolves the bundled script beside the executable
  (release) or in the repo `scripts/` dir (dev); errors if not found and no `command` given.
- Returns: `report` (`HookReport`: `project_path`, `targets[]` each `{ path, action }`), `command`,
  `instruction`. Restart the harness session for the hook to take effect. The daemon must be reachable
  at the hook endpoint (default `http://127.0.0.1:9870/mcp`).
- Errors (`isError`): empty `project_path`, unknown `targets` name, or no resolvable hook command.

### revoke_hooks
The explicit undo of `register_hooks`: remove the managed hook group from the selected harnesses'
project `settings.json`. Other config and user-authored hooks are preserved; a settings file that held
only our hook is deleted. After revoking, hard rules are no longer mediated and cap at Advisory/Verified.
- `project_path` (string, required), `targets` (string[], optional, omitted → all four).
- Returns: `report` (`HookReport`: `targets[]` each `{ path, action }` where action is `removed` |
  `deleted` | `unchanged`) and an `instruction`. A target with no managed hook is left `unchanged`, so
  re-running is safe.
- Errors (`isError`): empty `project_path`, unknown `targets` name.

### detect_hooks
Read-only: report which harnesses (Claude Code, CodeBuddy, WorkBuddy, Qoder) are installed and whether
the Agent Control hook is already registered in each one's project `settings.json`. Drives the
console's one-click register/restore UI; use before `register_hooks` to choose targets.
- `project_path` (string, required) — absolute project path to inspect.
- Returns: `harnesses[]` (`HookStatus`: `target`, `label`, `installed` bool, `settings_path`,
  `registered` bool).
- Errors (`isError`): empty `project_path`.

## Resources

- `control://overview` — JSON: `user_rules`, `sessions`, `active_sessions` counts.
- `control://sessions` — JSON: all sessions with snapshots and handoff state.
- `rules://index` — JSON: all user-level preference rules.

## Prompt

- `constraint-briefing` — generates binding instructions from applicable rules with provenance.
  Arguments: `project_path` (required), `agent_id` (required), `task` (optional). Useful as a
  system-prompt briefing when wiring a harness manually.

## Concrete lifecycle example

Arguments below mirror `scripts/mcp-e2e.mjs` (a runnable end-to-end check).

```jsonc
// 1. resolve rules
get_applicable_rules { "project_path": "/abs/repo", "agent_id": "harness-a", "languages": ["rust"] }
// -> { "provenance_id": "...", "applied_rules": [...], "conflicts": [...] }

// 2. freeze a session
create_session { "task": "refactor module", "project_path": "/abs/repo", "agent_id": "harness-a" }
// -> { "session_id": "UUID", "snapshot_hash": "...", "rules": [{ "id": "use-tabs", ... }] }

// 3. acknowledge exactly the snapshot rule ids
acknowledge_rules { "session_id": "UUID", "agent_id": "harness-a", "rule_ids": ["use-tabs", "prefer-concise"] }

// 4. record a file change
record_evidence { "session_id": "UUID", "kind": "file_change", "agent_id": "harness-a",
  "payload": { "file": "src/lib.rs", "before_sha256": "<64 hex>", "after_sha256": "<64 hex>" } }
// -> { "seq": 1, "entry_hash": "...", ... }

// 5. verify the chain
verify_evidence { "session_id": "UUID" }            // -> { "valid": true, ... }

// 5b. record resumable work state (the /goal + done/doing/todo) before handing off
update_work_state { "session_id": "UUID", "goal": "ship the refactor without breaking the public API",
  "work_items": [ { "title": "extract module", "status": "done" },
                  { "title": "wire hook", "status": "doing" },
                  { "title": "e2e coverage", "status": "todo" } ] }
// -> the updated session { "goal": "...", "work_items": [...], ... }

// 6. hand off to another harness
export_handoff { "session_id": "UUID", "to_agent": "harness-b", "context": { "summary": "in progress" } }
// -> bundle JSON (carries goal + work_items + assurance), and also writes
//    <project>/.agent-control/handoff/latest.md (the continuity brief the AGENTS.md managed block
//    points at, with Goal + Work state sections; a harness can read it without any MCP call).
import_handoff { "bundle": { /* full bundle from export */ }, "new_agent": "harness-b" }
acknowledge_rules { "session_id": "UUID", "agent_id": "harness-b", "rule_ids": ["use-tabs", "prefer-concise"] }
get_session_context { "session_id": "UUID" }        // -> { "session": { "handoffs": [...], "goal": "...", "work_items": [...] }, "evidence_head_hash": "..." }

// 6b. close the session when the unit of work ends (session_end evidence; chain kept auditable)
set_session_status { "session_id": "UUID", "status": "completed" }   // -> the updated session
// reopen later with { "status": "active" }; handed_off is set only by export_handoff

// 7. record a durable project contract discovered during the work (binds future sessions)
save_rule { "id": "api-isasc", "title": "Upstream isAsc type is ambiguous",
  "content": "isAsc may be bool or string; normalize explicitly before calling upstream.",
  "agent_id": "harness-b", "kind": "contract", "scope": "project", "project_path": "/abs/repo" }
// -> { "path": "/abs/repo/.agent-control/rules/api-isasc.md", "rule": { "version": 1, ... } }
// re-calling with the same id updates it and returns version 2

// 8. propose an uncertain/inferred contract for HUMAN REVIEW (does not bind until approved)
suggest_rule { "project_path": "/abs/repo", "agent_id": "harness-b", "id": "retry-backoff",
  "title": "Upstream calls need exponential backoff",
  "content": "Retry transient upstream failures with exponential backoff (max 5 attempts).",
  "kind": "contract", "rationale": "Observed intermittent 503s from the upstream API during load." }
// -> { "suggestion": { "id": "UUID", "status": "pending", ... }, "instruction": "..." }
list_suggestions { "project_path": "/abs/repo", "status": "pending" }
// -> { "suggestions": [...], "count": 1, "pending_count": 1 }
// A user approves it in the console (Suggestions view); it then becomes a binding project rule
// and appears here with status "approved" + approved_rule_id.

// 9. propose an ENVIRONMENT note for human review (environment scope, needs project_path; git-ignored)
suggest_rule { "scope": "environment", "project_path": "/abs/repo", "agent_id": "codex-cli", "id": "wsl-env",
  "title": "Agents run inside WSL",
  "content": "The repo lives in WSL; compile/verify there, but Windows-native tools are callable when needed.",
  "kind": "constraint", "rationale": "Agent kept needing reminding it was in WSL during build verification." }
// -> { "suggestion": { "id": "UUID", "status": "pending", "scope": "environment", ... }, "instruction": "..." }
list_suggestions { "status": "pending" }   // unfiltered queue includes the environment-scope proposal
// On approval it becomes an environment note (/abs/repo/.agent-control/local/wsl-env.md, git-ignored)
// injected into every future session briefing on this machine/project — never committed to AGENTS.md.
// For a portable personal preference instead, use "scope": "user" (no project_path) -> ~/.agent-control/rules/user/.

// 10. pause a rule without deleting it, then resume it (content is untouched)
set_rule_enabled { "id": "api-isasc", "enabled": false, "scope": "project", "project_path": "/abs/repo" }
// -> { "rule": { "enabled": false, ... } }; get_applicable_rules no longer returns it
set_rule_enabled { "id": "api-isasc", "enabled": true, "scope": "project", "project_path": "/abs/repo" }
// -> { "rule": { "enabled": true, ... } }; it binds sessions again

// 11. write a pointer block into selected native entry points (reach for harnesses that don't call MCP)
export_native { "project_path": "/abs/repo", "targets": ["agents", "claude", "gemini", "cursor"] }
// -> { "report": { "project_rules": 2, "user_rules": 0,
//        "targets": [ { "path": "/abs/repo/AGENTS.md", "action": "created" },
//                     { "path": "/abs/repo/CLAUDE.md", "action": "created" },
//                     { "path": "/abs/repo/GEMINI.md", "action": "created" },
//                     { "path": "/abs/repo/.cursor/rules/agent-control.mdc", "action": "created" } ] }, ... }
// Omit "targets" to write AGENTS.md only. The block is discovery-first: it points at .agent-control/rules/
// (read + summarize the enabled rules; it does NOT copy rule text) and excludes the git-ignored local/
// environment notes; hand-written prose outside it is preserved; re-exporting -> all "unchanged"; a rule edit
// never makes it stale. It ends with an optional "### Continuity" section pointing at handoff/latest.md.
// export_native { "project_path": "/abs/repo", "include_user": true } additionally writes ~/.claude/CLAUDE.md.

// 12. verify the snapshot's code_checks against the repo -> achieved assurance tiers (post-hoc)
verify_constraints { "session_id": "UUID", "agent_id": "harness-b" }
// -> { "assurances": [ { "rule_id": "use-tabs", "declared": "verify", "achieved": "verified",
//                        "basis": "1 code_check(s) passed" }, ... ],
//      "verified": 1, "advisory": 1, "enforced": 0 }
// A semantic rule (no code_checks) stays "advisory"; a failing check caps at "advisory";
// "enforced" is never assigned here — it needs a deny-capable chokepoint.

// 13. detect missing/older-format pointer blocks (read-only; prompts re-export, never rewrites)
check_export_drift { "project_path": "/abs/repo", "targets": ["agents", "claude", "gemini", "cursor"] }
// -> { "report": { "stale": false, "targets": [ { "path": "/abs/repo/AGENTS.md", "exists": true,
//        "exported_format": 2, "current_format": 2, "stale": false }, ... ] }, "instruction": "..." }
// A legacy v1 full-text block (no format marker) reads exported_format: null -> stale: true; re-export
// to migrate. Drift is file-vs-store format only — not a rule edit, not a running session's context.

// 14. revoke the pointer block (the undo of export_native): prose kept, block-only files deleted
revoke_native { "project_path": "/abs/repo", "targets": ["agents", "claude", "gemini", "cursor"] }
// -> { "report": { "targets": [ { "path": "/abs/repo/AGENTS.md", "action": "removed" },
//        { "path": "/abs/repo/CLAUDE.md", "action": "deleted" }, ... ] }, "instruction": "..." }
// "removed" = block stripped, surrounding prose kept; "deleted" = file held only the block;
// "unchanged" = no block. Re-running is a safe no-op.

// 15. register the enforcement hooks (SessionStart briefing + PreToolUse gate) into harness settings
detect_hooks { "project_path": "/abs/repo" }
// -> { "harnesses": [ { "target": "claude", "installed": true, "registered": false, ... }, ... ] }
register_hooks { "project_path": "/abs/repo", "targets": ["claude", "codebuddy", "qoder"] }
// -> { "report": { "targets": [ { "path": "/abs/repo/.claude/settings.json", "action": "created" }, ... ] },
//      "command": "node \".../agent-control-hook.mjs\"", "instruction": "..." }
// Omit "targets" to register all four. Other settings keys + user hooks are preserved; re-run is
// idempotent. Restart the harness session; hard rules can now reach the Enforced tier.

// 16. the PreToolUse gate: deny a call that violates a hard rule (the hook exits 2 on deny)
check_tool_call { "project_path": "/abs/repo", "tool_name": "Bash",
  "tool_input": { "command": "rm -rf /" }, "agent_id": "claude-code", "session_id": "UUID" }
// -> { "report": { "allowed": false, "denials": [ { "rule_id": "no-rm-rf", "reason": "..." } ],
//        "enforced_rules": [], "hard_rules_checked": 1 },
//      "denial_reason": "[no-rm-rf] ...", "instruction": "Tool call DENIED ..." }
// A passing call returns allowed: true and lists the hard rules that mediated it in enforced_rules
// (the basis for the Enforced tier). Omit session_id for a stateless pre-check.

// 17. revoke the hooks (the undo of register_hooks): user config kept, hook-only files deleted
revoke_hooks { "project_path": "/abs/repo", "targets": ["claude", "codebuddy", "qoder"] }
// -> { "report": { "targets": [ { "path": "/abs/repo/.claude/settings.json", "action": "removed" }, ... ] } }
// Hard rules are no longer mediated at a chokepoint and cap at Advisory/Verified again.
```

A partial `acknowledge_rules` (omitting a snapshot rule) is flagged in the report — acknowledge
the full set. Run `node scripts/mcp-e2e.mjs` (stdio) or `node scripts/mcp-http-e2e.mjs` (HTTP) in
the source repo to see the whole lifecycle asserted against a throwaway data dir.