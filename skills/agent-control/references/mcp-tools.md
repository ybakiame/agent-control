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
  `conflicts[]`, `instruction`. Resolve conflicts with the user before proceeding.

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
Freeze a constraint snapshot for a unit of work.
- `task` (string, required), `project_path` (string, required), `agent_id` (string, required),
  `task_tags` (string[], default `[]`).
- Returns: `session_id` (UUID), `snapshot_hash`, `rules[]`, `instruction`. Acknowledge these rules
  before making changes.

### acknowledge_rules
Commit to the snapshot's rules; server compares against the frozen snapshot.
- `session_id` (UUID string, required), `agent_id` (string, required), `rule_ids` (string[],
  required) — the exact ids from `create_session.rules`.
- Returns an acknowledgment report listing `missing` / `unexpected` rules. Required after session
  start and after every handoff.

### record_evidence
Append a tamper-evident entry to the session hash chain.
- `session_id` (UUID string, required), `agent_id` (string, required).
- `kind` (string, required) — one of: `rule_acknowledgment`, `tool_call`, `file_change`,
  `test_result`, `handoff_out`, `handoff_in`, `constraint_violation`, `session_start`,
  `session_end`.
- `payload` (object, required) — arbitrary JSON. For `file_change` include
  `{ "file": "...", "before_sha256": "<64 hex>", "after_sha256": "<64 hex>" }`.
- Returns the entry: `seq` (>= 1), `entry_hash`, `prev_hash`, `kind`, `payload`, timestamps.

### get_session_context
Resume work started by another harness.
- `session_id` (UUID string, required).
- Returns: `session` (task, constraint snapshot, current `agent_id`, `handoffs[]`, status) and
  `evidence_head_hash`.

### export_handoff
Produce a bundle to transfer the session to another harness.
- `session_id` (UUID string, required), `to_agent` (string, required), `context` (object, default
  `{}`) — carry-over summary (conversation state, files touched, decisions).
- Returns the bundle (includes `bundle_hash`, frozen constraints, evidence head, and the source
  session's achieved `assurance[]` tiers as a baseline).

### import_handoff
Resume a session under a new harness; verifies bundle integrity + snapshot consistency.
- `bundle` (object, required) — the full JSON from `export_handoff`.
- `new_agent` (string, required).
- Returns: `session_id`, `agent_id`, `rules_to_acknowledge[]`, `carried_assurance[]` (the source
  tiers) and `tier_warning` (set when any carried tier is above Advisory). The warning is
  **warn-not-block**: the new harness must re-run `verify_constraints` to re-earn the tiers — they do
  not transfer automatically. Call `acknowledge_rules` next.

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
  chokepoint (ACP permission round-trip), so a rule declaring `hard` still caps at Advisory/Verified.
- Persists `Session.assurance[]` (`{ rule_id, declared, achieved, basis }`) beside the frozen snapshot
  (the `snapshot_hash` over rules is unchanged) and appends a `test_result` evidence entry (checks +
  assurance), plus a `constraint_violation` entry when any check failed.
- Returns: `ConstraintVerificationReport` — `session_id`, `agent_id`, `assurances[]`, and `verified` /
  `advisory` / `enforced` counts.
- Errors (`isError`): unknown `session_id`.

### list_rules
List stored rules (user preferences always; project contracts when `project_path` given).
- `project_path` (string, optional).
- Returns: `rules[]`, `count`.

### save_rule
Actively create or update a rule file — the way to record a **durable project contract** (or a
personal preference) so it shows up in the console and binds future sessions. Use this for lasting
project knowledge; use `record_evidence` for session-scoped actions instead.
- `id` (string, required) — stable unique id (kebab-case recommended). Reusing an id **updates** the
  rule and bumps its `version`; it never creates a duplicate. A content update preserves the rule's
  `enabled` state — use `set_rule_enabled` to toggle it.
- `title` (string, required), `content` (string, required) — Markdown body stating the contract and
  how to satisfy it.
- `agent_id` (string, required) — recorded as the rule's `source.author` for provenance.
- `kind` (string, default `contract`) — `preference` | `contract` | `constraint`.
- `scope` (string, default `project`) — `user` | `project`. `session` is rejected (snapshots only).
- `project_path` (string) — **required when `scope` is `project`**; absolute project path.
- `topic` (string, optional), `tags` (string[], default `[]`).
- `enforcement` (string, default `advisory`) — declared intent: `advisory` (semantic, not mechanically
  checkable) | `verify` (carries `code_checks`) | `hard` (claims a deny-capable chokepoint). Declaring
  `hard` does not itself enforce; the achieved tier is computed separately by `verify_constraints`.
- `code_checks` (object[], default `[]`) — each `{ file, key, expect, description? }`, evaluated
  against real repo files by `verify_constraints` and `check_conflicts`.
- Returns: `path` (written file), `rule` (the saved rule with its final `version`), `instruction`.
  The file lands in `<project>/.agent-control/rules/<id>.md` (project) or
  `~/.agent-control/rules/user/<id>.md` (user). Confirm with `get_applicable_rules` / `list_rules`.
- Errors (`isError`): empty id/title, unknown kind/scope, or project scope without `project_path`.

### set_rule_enabled
Pause or resume a rule **without changing its content**. A disabled rule is excluded from
resolution (`Decision::Disabled`) and stops binding sessions; enabling restores it. This is the
only tool that flips `enabled` — a `save_rule` content update preserves the existing state, so
editing a paused rule never silently re-activates it. Prefer disabling over deleting when a
contract may apply again later.
- `id` (string, required) — the rule to toggle.
- `enabled` (bool, required) — `false` to disable, `true` to enable.
- `scope` (string, default `project`) — `user` | `project`. `session` is rejected.
- `project_path` (string) — **required when `scope` is `project`**; absolute project path.
- Returns: `rule` (the updated rule with its new `enabled` and bumped `version`), `instruction`.
- Errors (`isError`): unknown scope, project scope without `project_path`, or an id that does not
  exist (it never creates a rule).

### suggest_rule
Propose a new rule for **human review** instead of writing it directly. The draft is queued
as a `pending` suggestion in the console's **Suggestions** view; a user reviews, edits, approves
(making it binding) or rejects it. **Prefer this over `save_rule` when the rule is
inferred, uncertain, or needs user confirmation.** A pending suggestion does **not** bind sessions.
- `scope` (string, default `project`) — `user` | `project`. `user` proposes a **personal preference**
  that follows the user across all projects (approval writes `~/.agent-control/rules/user/<id>.md`);
  `project` proposes a project contract (approval writes `<project>/.agent-control/rules/<id>.md`).
  `session` is rejected.
- `project_path` (string) — **required when `scope` is `project`**; absolute project path. Omit for
  `scope: "user"`.
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
- Errors (`isError`): empty id/title/content, project scope without `project_path`, or unknown
  kind/scope.

> **Where do environment notes go?** Facts about the user's working environment that should hold
> across projects — e.g. "I run codex/kimi CLIs inside WSL; the repo lives in WSL, but Windows-native
> tools are callable for compile/verify" — are **user-scope `constraint` rules**. Propose one with
> `suggest_rule { scope: "user", kind: "constraint", ... }` (or record a confident one with
> `save_rule { scope: "user", kind: "constraint" }`). User rules always load, are frozen into every
> session snapshot, and are injected proactively via the ACP `constraint_briefing` and
> `get_applicable_rules` — so the agent is told up front instead of needing a reminder mid-task.

### list_suggestions
List rule suggestions submitted by agents; check whether your proposals were reviewed.
- `project_path` (string, optional) — filter to one project.
- `status` (string, optional) — `pending` | `approved` | `rejected`; omit for all.
- Returns: `suggestions[]`, `count`, `pending_count`. An approved suggestion carries
  `approved_rule_id` (the binding rule it became) and `reviewed_at`; a rejected one carries
  `review_note` and `reviewed_at`.
- Errors (`isError`): unknown status.

### export_native
Compile the resolved **enabled** rules into agents' native entry points so a harness that never
calls MCP still receives the contracts. This is **reach** (Advisory tier), not verification or
enforcement — the exported text is guidance the agent reads like any AGENTS.md. `verify_constraints`
earns `Verified`; only a deny-capable chokepoint (the ACP permission round-trip) earns `Enforced`.
- `project_path` (string, required) — absolute project path; its enabled project contracts are
  rendered into a managed block.
- `include_user` (bool, default `false`) — also export enabled **user** preferences. When `true`,
  writes a user block to `~/.claude/CLAUDE.md` (global, follows the user). Leave `false` for a
  project-only export so nothing outside the repo is touched.
- Writes an in-place **managed block** delimited by `<!-- agent-control:begin -->` /
  `<!-- agent-control:end -->`. Everything **outside** the block is preserved verbatim; the block is
  replaced on re-export and appended if absent. **Byte-idempotent**: re-exporting unchanged rules
  yields `unchanged` (no newline growth). Targets:
  - `<project>/AGENTS.md` — project contracts block.
  - `<project>/CLAUDE.md` — project contracts block.
  - `<project>/.cursor/rules/agent-control.mdc` — same block with Cursor MDC frontmatter outside it.
  - `~/.claude/CLAUDE.md` — personal-preferences block (**only when `include_user: true`**).
  Disabled rules are never exported. A lone begin/end marker (malformed block) is an error, not a
  silent overwrite.
- Returns: `report` (`ExportReport`: `project_path`, `project_rules` count, `user_rules` count,
  `targets[]` each `{ path, action }` where action is `created` | `updated` | `unchanged`) and an
  `instruction`.
- Errors (`isError`): empty `project_path`, or a malformed existing managed block.

### check_export_drift
Detect whether the native exports lag the current rule store (**file-vs-store** staleness).
Read-only: it prompts a re-export and **never rewrites** files.
- `project_path` (string, required), `include_user` (bool, default `false`) — same scope as
  `export_native`.
- Each managed block embeds a ruleset hash (`<!-- agent-control:ruleset sha256:… -->`); this compares
  it against the hash of the currently enabled ruleset.
- Returns: `report` (`DriftReport`: `project_path`, `stale` bool, `targets[]` each
  `{ path, exists, exported_hash, current_hash, stale }`) and an `instruction`. A target is `stale`
  only when it exists and its embedded hash differs; a never-exported file is **not** stale.
- Scope limit: detects layer-1 (exported text out of date) only — **not** whether an already-running
  session's injected context is stale (layer 2). When stale, re-run `export_native`.
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

// 6. hand off to another harness
export_handoff { "session_id": "UUID", "to_agent": "harness-b", "context": { "summary": "in progress" } }
import_handoff { "bundle": { /* full bundle from export */ }, "new_agent": "harness-b" }
acknowledge_rules { "session_id": "UUID", "agent_id": "harness-b", "rule_ids": ["use-tabs", "prefer-concise"] }
get_session_context { "session_id": "UUID" }        // -> { "session": { "handoffs": [...] }, "evidence_head_hash": "..." }

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

// 9. propose a PERSONAL PREFERENCE / environment note for human review (user scope, no project_path)
suggest_rule { "scope": "user", "agent_id": "codex-cli", "id": "wsl-env",
  "title": "Agents run inside WSL",
  "content": "The repo lives in WSL; compile/verify there, but Windows-native tools are callable when needed.",
  "kind": "constraint", "rationale": "Agent kept needing reminding it was in WSL during build verification." }
// -> { "suggestion": { "id": "UUID", "status": "pending", "scope": "user", ... }, "instruction": "..." }
list_suggestions { "status": "pending" }   // unfiltered queue includes the user-scope proposal
// On approval it becomes a user rule (~/.agent-control/rules/user/wsl-env.md) that applies across
// all projects and is injected into every future session briefing.

// 10. pause a rule without deleting it, then resume it (content is untouched)
set_rule_enabled { "id": "api-isasc", "enabled": false, "scope": "project", "project_path": "/abs/repo" }
// -> { "rule": { "enabled": false, ... } }; get_applicable_rules no longer returns it
set_rule_enabled { "id": "api-isasc", "enabled": true, "scope": "project", "project_path": "/abs/repo" }
// -> { "rule": { "enabled": true, ... } }; it binds sessions again

// 11. compile enabled contracts into native entry points (reach for harnesses that don't call MCP)
export_native { "project_path": "/abs/repo" }   // include_user defaults to false (project-only)
// -> { "report": { "project_rules": 2, "user_rules": 0,
//        "targets": [ { "path": "/abs/repo/AGENTS.md", "action": "created" },
//                     { "path": "/abs/repo/CLAUDE.md", "action": "created" },
//                     { "path": "/abs/repo/.cursor/rules/agent-control.mdc", "action": "created" } ] }, ... }
// Hand-written prose outside the managed block is preserved; re-exporting unchanged rules -> all "unchanged".
// export_native { "project_path": "/abs/repo", "include_user": true } additionally writes ~/.claude/CLAUDE.md.

// 12. verify the snapshot's code_checks against the repo -> achieved assurance tiers (post-hoc)
verify_constraints { "session_id": "UUID", "agent_id": "harness-b" }
// -> { "assurances": [ { "rule_id": "use-tabs", "declared": "verify", "achieved": "verified",
//                        "basis": "1 code_check(s) passed" }, ... ],
//      "verified": 1, "advisory": 1, "enforced": 0 }
// A semantic rule (no code_checks) stays "advisory"; a failing check caps at "advisory";
// "enforced" is never assigned here — it needs a deny-capable chokepoint.

// 13. detect stale native exports after editing rules (read-only; prompts re-export, never rewrites)
check_export_drift { "project_path": "/abs/repo" }
// -> { "report": { "stale": true, "targets": [ { "path": "/abs/repo/AGENTS.md", "exists": true,
//        "exported_hash": "...", "current_hash": "...", "stale": true }, ... ] }, "instruction": "..." }
// Re-run export_native to refresh. Drift covers file-vs-store staleness only, not a running session's context.
```

A partial `acknowledge_rules` (omitting a snapshot rule) is flagged in the report — acknowledge
the full set. Run `node scripts/mcp-e2e.mjs` (stdio) or `node scripts/mcp-http-e2e.mjs` (HTTP) in
the source repo to see the whole lifecycle asserted against a throwaway data dir.