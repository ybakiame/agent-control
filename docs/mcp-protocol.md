# MCP protocol

> Part of the Agent Control engineering conventions. Golden rules, toolchain commands and
> the must-know-before-edit list live in [AGENTS.md](../AGENTS.md); this file holds the detail.

## MCP protocol / MCP 协议

Transports (rmcp 3.9): **stdio** and **Streamable HTTP** (axum, route `/mcp`, `mcp-session-id`
header). The desktop app runs MCP as a **background daemon** at `http://127.0.0.1:<mcp_port>/mcp`
(default port `9870`):
`console.rs::start_mcp_http` first probes the bind address and reuses an already-running daemon,
otherwise spawns the sibling `agent-control-mcp` binary **detached** (`DETACHED_PROCESS |
CREATE_NO_WINDOW`) so it survives closing the GUI; it falls back to in-process
`agent_control_mcp::serve_http` only when no sibling binary exists (dev). `mcp_external` tracks
which mode is active. The daemon resolves the same data root as the app (the `portable.marker`
sits next to both executables). `serve_http` also exposes a **loopback-only `POST /shutdown`**
route (rejects non-loopback peers) so the console's Stop button can stop a daemon it does not own
as a child; `stop_mcp_http` signals it for external daemons and cancels the token for in-process.
Both binaries are `#![windows_subsystem = "windows"]` (no popped cmd window) and log to
`<root>/logs/{app,mcp}.log` via `tracing-appender` (the `WorkerGuard` must outlive the process).

**App config (`AppConfig`, `core/src/config.rs`):** user-editable preferences live in `config.toml`
at the data root (`DataLayout::config_file`): `locale` (`en`|`zh`) and `mcp_port` (u16, default
`DEFAULT_MCP_PORT = 9870`). `AppConfig::load` reads it (serde defaults fill missing keys) and, on
first run, **migrates** a legacy `ui.json` (locale only) into `config.toml` then deletes the json —
a one-time conversion, not a lingering shim. `Console` loads it in `new()` (deriving `locale` and
`mcp_bind = 127.0.0.1:<mcp_port>`); `set_locale` and `apply_mcp_port` persist via `AppConfig::save`.
The port is editable in **Settings → MCP server** (`mcp_port_input` + Apply): `save_mcp_port_from_input`
parses/validates (1–65535), then `apply_mcp_port` saves, stops the old daemon and starts a new one on
the new bind. Because each data root keeps its own `config.toml`, a **portable/dev** instance can bind
a different port (e.g. `9871`) and stay fully isolated from a production daemon on `9870`. The sidebar
footer also carries an **icon start/stop toggle** (`mcp-sidebar-toggle`, `Play`/`Square` from the
default icon set) beside the MCP status, calling `toggle_mcp_http`.

MCP bin CLI (`crates/mcp/src/main.rs`):

```sh
cargo run -p agent-control-mcp -- --stdio                 # stdio (harness spawns this)
cargo run -p agent-control-mcp -- --bind 127.0.0.1:9870   # HTTP (default bind)
cargo run -p agent-control-mcp -- --daemon                # detached background daemon, then exit
cargo run -p agent-control-mcp -- --doctor                # read-only diagnostics JSON for cwd, then exit
cargo run -p agent-control-mcp -- --data-dir <path>       # override data root
```

**Single-instance:** the TCP bind is the lock — a second HTTP launch on a busy address logs
"already serving" and exits cleanly (`AddrInUse` guard), so daemon spawns are idempotent.

**29 tools:** `get_applicable_rules`, `explain_rule`, `check_conflicts`, `create_session`,
`record_evidence`, `acknowledge_rules`, `get_session_context`, `update_work_state`,
`set_session_status`, `export_handoff`,
`import_handoff`, `verify_evidence`, `list_rules`, `save_rule`, `set_rule_enabled`, `suggest_rule`,
`list_suggestions`, `export_native`, `check_export_drift`, `revoke_native`, `refresh_snapshot`, `verify_constraints`,
`check_tool_call`, `register_hooks`, `revoke_hooks`, `detect_hooks`, `doctor`, `search`, `setup` (plus resources & prompts).
`save_rule` is the active rule-authoring path: an agent records a discovered project contract
(`scope=project`, `kind=contract`, `project_path`), a personal preference (`scope=user`), or a
machine/project environment note (`scope=environment`, `project_path`, written to git-ignored
`local/`) as a rule file via `Store::upsert_rule`; reusing an id updates + bumps version. Use it for
durable project knowledge; `record_evidence` remains for session-scoped actions (fixed `EvidenceKind`
enum).
`set_rule_enabled` pauses/resumes a rule without touching its content (`id`, `enabled`, `scope`,
`project_path` for project scope): disabled rules are excluded from resolution and stop binding
sessions, enabling restores them. It is the only tool that flips `enabled` — `save_rule` content
updates preserve the existing state.
`suggest_rule` is the human-in-the-loop alternative for inferred/uncertain rules: it queues a
`Pending` draft via `Store::submit_suggestion` (with a `rationale`) that does **not** bind sessions
until a user approves it in the Suggestions view. `scope=project` (default) requires `project_path`
and approval writes a project contract; `scope=environment` also requires `project_path` and approval
writes a git-ignored `local/` environment note (the right home for machine/project facts like WSL);
`scope=user` needs no `project_path` and approval writes a personal preference that follows the user
across projects.
`list_suggestions` reads the queue (`pending|approved|rejected`) plus a pending count. Prefer
`suggest_rule` when a human should sign off first, `save_rule` when the rule is confident knowledge.
`update_work_state` (`session_id`, optional `goal`, optional `work_items[]` of `{title, status}` with
`status` = `done|doing|todo`, default `todo`, optional `expected_version`) records the session's
resumable **work state** via `Store::set_session_work_state` — the `/goal` objective plus
done/doing/todo progress. It **replaces** the session's goal + items and returns the updated session
(including `work_state_version`). CONC1: pass `expected_version` (the version you read) for
optimistic concurrency — a stale value is a conflict error carrying the current version, so
concurrent planners/implementers retry instead of clobbering; omitted keeps unconditional replace.
Telemetry sessions (task `hook`, UX11) reject work state outright. `export_handoff` carries them into the
bundle and `handoff/latest.md` (a **Goal** + **Work state** section), and `import_handoff` /
`get_session_context` surface them, so a new harness inherits progress instead of relying on its own
todo list. Call it as work completes/starts and before handing off.
`export_native` (`project_path`, optional `targets`, optional `include_user`, optional `inline`)
writes a managed pointer block into the selected native harness entry points via
`Store::export_native`. `targets` is a list of
`agents|claude|gemini|cursor` (omitted → `AGENTS.md` only); the block points at the live rules in
`.agent-control/rules/` and does **not** copy rule text, so a rule edit never makes it stale and
re-export is byte-idempotent. `inline: true` (default false) additionally embeds the enabled project
contract/constraint bodies plus a ruleset hash (mode marker `inline`), trading staleness-on-edit for
agents that only read the file; `check_export_drift` then flags ruleset drift until re-export.
Everything outside the block is preserved; it returns an `ExportReport`
with per-target `created|updated|unchanged`. The guidance leads with read-and-summarize discovery of
the project norms, excludes environment notes (git-ignored `local/`, briefing-only), and ends with an
optional `### Continuity` section (read `.agent-control/handoff/latest.md`, refresh via
`export_handoff`). Use it so agents that
read those files learn where the contracts live without an MCP call. `revoke_native` (same
`project_path`/`targets`/`include_user`) is the explicit undo: it strips the block, keeps surrounding
prose (`removed`), deletes block-only files (`deleted`), and is a safe no-op on block-free targets
(`unchanged`). The harness hooks (`scripts/agent-control-hook.mjs`, wired by `register_hooks` for nine
vendors) are the push/enforce counterparts. The **start** event calls `get_applicable_rules` over MCP
HTTP and injects a briefing into session context where the vendor supports it (reach); the **gate**
event resolves a session and calls `check_tool_call`, blocking the tool in the vendor's deny shape
(claude-shaped exit 2 + stderr, Cursor `{permission:deny}`, Antigravity `{decision:deny}`) when a hard
rule denies (enforce) — and passing `session_id` records the `Enforced` tier. Both are read-only and
fail-open (the vendor's allow output) when the daemon is down, unless `AGENT_CONTROL_HOOK_FAIL_CLOSED`
opts the gate into fail-closed. Export/hook *deliver* rules; `code_checks` + the evidence chain
*verify*; hard enforcement exists only where the gate mediates the call (see **Assurance tiers** in [architecture.md](architecture.md)).
`check_export_drift` (`project_path`, optional `targets`, optional `include_user`) returns a
`DriftReport` flagging targets missing the current-format block (embedded block-format version ≠
`BLOCK_FORMAT`, e.g. a legacy v1 full-text block) or holding an outdated **inline** snapshot
(`inline && exported_ruleset != current_ruleset`); read-only, prompts re-export, never rewrites. A
pointer block never goes stale on a rule edit; an inline block does (by design).
`verify_constraints` (`session_id`, `agent_id`) runs the frozen snapshot's `code_checks` against the
repo, records achieved tiers + evidence, and returns a `ConstraintVerificationReport`
(`verified`/`advisory`/`enforced` counts). `save_rule` / `suggest_rule` accept `enforcement`
(`advisory|verify|hard`); `save_rule` also accepts `code_checks` and `tool_guards`.
`check_tool_call` (`project_path`, `tool_name`, optional `tool_input`, `agent_id`, optional
`session_id`) is the deny-capable **PreToolUse chokepoint** — the only path that produces
`AssuranceTier::Enforced`. It loads the enabled hard rules and returns a `ToolGateReport`
(`allowed`, `denials`, `enforced_rules`, `hard_rules_checked`): a hard rule's `code_checks` gate repo
state (deny while a check fails), and its `tool_guards` (tool-name glob + input regex + reason) deny
matching calls. No hard rules → allowed (fail-open). The hook blocks on `allowed: false` with exit
code 2 + the joined denial reason on stderr, and records a `ToolGate` evidence entry.
`register_hooks` (`project_path`, optional `targets`, optional `command`) writes the managed start +
pre-tool hook group into each selected harness's own config, in its own schema, preserving all other
config and user hooks; idempotent, atomic. `targets` is any of the **nine** `HookTarget` keys (`claude`
| `codebuddy` | `workbuddy` | `qoder` | `qoder-cn` | `codex` | `cursor` | `antigravity` | `kimi`;
omitted → all nine). Each target selects a `ConfigCodec` (nested JSON for the Claude family + Codex +
Qoder CN, flat JSON for Cursor, named bundle for Antigravity, a TOML marker block for Kimi), its event
names (`SessionStart`/`PreToolUse`, Cursor `sessionStart`/`preToolUse`, Antigravity
`PreInvocation`/`PreToolUse`) and its `HookScope` (project for seven; **user** for Kimi
`~/.kimi-code/config.toml` and Antigravity `~/.gemini/config/hooks.json` — a user-level hook still gates
per-project by the call's `cwd`). The registered command bakes in the vendor id + event so the shared
Node script picks the right stdin/output shape.
`revoke_hooks` (same `project_path`/`targets`) removes only the managed group (identified by the
`agent-control-hook.mjs` command marker), pruning empty events/hooks and deleting block-only files; the
Kimi TOML codec strips only its own `[[hooks]]` block so user comments survive.
`detect_hooks` (`project_path`) reports per-target `HookStatus` (`target`, `label`, `scope`, installed
harness, `settings_path`, whether the managed group is present) for all nine without writing — the
read-only counterpart that backs the Projects-view Hooks UI.
`doctor` (`project_path`, optional `daemon_bind`) is the read-only aggregate diagnostic
(`core/src/doctor.rs`, `Store::doctor`): hook registration across the nine harnesses + export drift
(all four targets) + evidence-chain integrity for **every** session + applied-rule/conflict counts +
a loopback TCP probe of the daemon. `DoctorReport.healthy` is false when any chain is broken, any
export target is stale or any conflict exists; it performs no writes. Backs the MCP `doctor` tool,
the mcp-bin `--doctor` flag (JSON to stdout, exit 1 when unhealthy) and the Settings-view
**Diagnostics** card (`Console::run_doctor`, `doctor_report`).
`search` (`query`, optional `kind` = `rules|sessions|evidence`, optional `project_path`, optional
`limit`) is full-text search over three FTS5 shadow tables (`search_rules`, `search_sessions`,
`search_evidence`, `core/src/search.rs` + `store/db.rs`). unicode61 treats a CJK run as one token,
so text is pre-segmented by a hybrid segmenter: CJK runs → character bigrams (zero dictionary),
other scripts → lowercased words; queries use the same transform, making two-character Chinese
queries match. Index rows store space-joined tokens; ranking is bm25 with `snippet()` excerpts.
Shadow rows sync at the write chokepoints (`save_rule`/`delete_rule`, `db.upsert_session`,
`db.append_evidence`, `db.delete_session`) and rebuild from source on every `Store::open` (rules
from disk via the project registry, sessions/evidence from the DB). Backs the MCP `search` tool
and the Dashboard search card (`Console::run_search`, `search_input`/`search_hits`).
`setup` (`project_path`, optional `targets`, optional `export_targets`, optional `include_user`) is
the one-command harness onboarding: a thin `Store::setup` composition of `export_native` +
`register_hooks` with `hooks::default_hook_command()` — no new business logic. Defaults: all nine
hook targets, `AGENTS.md` export only. Returns a `SetupReport { project_path, command, export,
hooks }`; re-running is idempotent (`unchanged` outcomes). Because `register_hooks` dedupes the two
Qoder builds by config path, seven project-scope targets yield six hook outcomes. Undo with
`revoke_native` / `revoke_hooks`. Backs the MCP `setup` tool and the Projects-view **Setup** button
(`Console::setup_for_selected_project`, all nine hook targets + default export, project-only).

Recommended harness call order:

1. `get_applicable_rules` — resolve binding rules + provenance + conflicts before working.
2. `create_session` — freeze the constraint snapshot (get-or-create: an active session with the same
   project + agent + task is reused, `reused: true`, instead of piling up duplicates; the check and
   insert are one IMMEDIATE transaction and the project path is canonicalized, so concurrent
   creators and backslash/forward-slash spellings converge on one session).
3. `acknowledge_rules` — prove you received exactly the snapshot's rules.
4. `record_evidence` — append tool calls / file changes / test results to the hash chain.
5. `verify_constraints` — run the snapshot's `code_checks` against the repo, record achieved tiers.
6. `update_work_state` — record the `/goal` objective + done/doing/todo work items as progress is made
   and before handing off, so continuity carries resumable state (not just rules).
7. `export_handoff` → `import_handoff` + `acknowledge_rules` — move between harnesses (re-verify tiers).
   `export_handoff` carries the goal + work items and also writes
   `<project>/.agent-control/handoff/latest.md` (a readable, **git-ignored local** continuity brief with **Goal** / **Work
   state** sections) beside the JSON bundle, so a harness that never calls MCP can inherit the handoff
   by reading that file (the exported `AGENTS.md` block points at it).
8. `verify_evidence` — recompute the chain, detect tampering.
9. `set_session_status` — `completed`/`failed` when the unit of work ends (appends `session_end`
   evidence, keeps the chain auditable, leaves the active list); `active` reopens. `handed_off` is
   set only by `export_handoff`. `non_owner: true` (CONC1) refreshes bundle + brief without flipping
   status or superseding the owner's pending bundle; telemetry sessions reject export (UX11).
   `refresh_snapshot` (SNAP1) re-freezes a long-lived session from the live rule set.

When adding a tool: implement the handler in `crates/mcp/src/lib.rs` with the rmcp `#[tool]`
macro, keep it a thin adapter over `Store`/`RuleEngine` (no business logic in the transport
layer), and extend `scripts/mcp-e2e.mjs` to cover it.

End-to-end checks (Node.js, no deps):

```sh
node scripts/mcp-e2e.mjs        # stdio: rules, conflicts, sessions, evidence, handoff
node scripts/mcp-http-e2e.mjs   # Streamable HTTP transport
```

