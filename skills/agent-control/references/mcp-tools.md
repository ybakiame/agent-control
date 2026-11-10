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
- Returns the bundle (includes `bundle_hash`, frozen constraints, evidence head).

### import_handoff
Resume a session under a new harness; verifies bundle integrity + snapshot consistency.
- `bundle` (object, required) — the full JSON from `export_handoff`.
- `new_agent` (string, required).
- Returns: `session_id`, `agent_id`, `rules_to_acknowledge[]`. Call `acknowledge_rules` next.

### verify_evidence
Recompute every entry hash; detect tampering or gaps.
- `session_id` (UUID string, required).
- Returns a report with `valid` (bool) and any broken/gapped entries.

### list_rules
List stored rules (user preferences always; project contracts when `project_path` given).
- `project_path` (string, optional).
- Returns: `rules[]`, `count`.

### save_rule
Actively create or update a rule file — the way to record a **durable project contract** (or a
personal preference) so it shows up in the console and binds future sessions. Use this for lasting
project knowledge; use `record_evidence` for session-scoped actions instead.
- `id` (string, required) — stable unique id (kebab-case recommended). Reusing an id **updates** the
  rule and bumps its `version`; it never creates a duplicate.
- `title` (string, required), `content` (string, required) — Markdown body stating the contract and
  how to satisfy it.
- `agent_id` (string, required) — recorded as the rule's `source.author` for provenance.
- `kind` (string, default `contract`) — `preference` | `contract` | `constraint`.
- `scope` (string, default `project`) — `user` | `project`. `session` is rejected (snapshots only).
- `project_path` (string) — **required when `scope` is `project`**; absolute project path.
- `topic` (string, optional), `tags` (string[], default `[]`).
- Returns: `path` (written file), `rule` (the saved rule with its final `version`), `instruction`.
  The file lands in `<project>/.agent-control/rules/<id>.md` (project) or
  `~/.agent-control/rules/user/<id>.md` (user). Confirm with `get_applicable_rules` / `list_rules`.
- Errors (`isError`): empty id/title, unknown kind/scope, or project scope without `project_path`.

### suggest_rule
Propose a new project rule for **human review** instead of writing it directly. The draft is queued
as a `pending` suggestion in the console's **Suggestions** view; a user reviews, edits, approves
(making it a binding project rule) or rejects it. **Prefer this over `save_rule` when the contract is
inferred, uncertain, or needs user confirmation.** A pending suggestion does **not** bind sessions.
- `project_path` (string, required) — absolute project path the suggestion targets.
- `agent_id` (string, required) — recorded as the proposer for provenance.
- `id` (string, required) — proposed stable rule id (kebab-case recommended).
- `title` (string, required), `content` (string, required) — proposed Markdown body.
- `kind` (string, default `contract`) — `preference` | `contract` | `constraint`.
- `topic` (string, optional), `tags` (string[], default `[]`).
- `rationale` (string, optional but recommended) — why the rule is proposed (discovery context,
  evidence); shown to the human reviewer.
- Returns: `suggestion` (the queued draft with `status: "pending"`, `id` UUID, `created_at`) and
  `instruction`. Continue working under the currently applicable rules until a user approves it.
- Errors (`isError`): empty project_path/id/title/content, or unknown kind.

### list_suggestions
List rule suggestions submitted by agents; check whether your proposals were reviewed.
- `project_path` (string, optional) — filter to one project.
- `status` (string, optional) — `pending` | `approved` | `rejected`; omit for all.
- Returns: `suggestions[]`, `count`, `pending_count`. An approved suggestion carries
  `approved_rule_id` (the binding rule it became) and `reviewed_at`; a rejected one carries
  `review_note` and `reviewed_at`.
- Errors (`isError`): unknown status.

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
```

A partial `acknowledge_rules` (omitting a snapshot rule) is flagged in the report — acknowledge
the full set. Run `node scripts/mcp-e2e.mjs` (stdio) or `node scripts/mcp-http-e2e.mjs` (HTTP) in
the source repo to see the whole lifecycle asserted against a throwaway data dir.