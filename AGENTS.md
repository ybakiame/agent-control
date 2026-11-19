# AGENTS.md

Engineering conventions for agents (and humans) working on **Agent Control** — a Rust-first,
local-first command console that connects different Agents over **MCP** and enforces contracts
through their native **hooks**, making rule provenance, conflicts, cross-harness continuity and
evidence verifiable.

以开发者为中心、以工程协议为基础的本地优先指挥台。本文件是仓库的工程约定，动手前请先读。

> Read this file before editing. It defines the commands, the crate boundaries, and the
> non-negotiable conventions (i18n, self-rendered title bar, accessibility-first verification).

---

## Golden rules / 铁律

1. **Rust-first.** All product logic lives in the three Rust crates. Node.js appears only in
   `scripts/*.mjs` as dependency-free protocol hooks and end-to-end checks.
2. **Use mise for everything.** Never call a globally-installed `cargo`/`rustc` directly; the
   toolchain is pinned. Use `mise run <task>` or `mise exec -- cargo ...`.
3. **Never hardcode user-facing UI strings.** Every label goes through the i18n table
   (`crates/app/src/i18n.rs`) and `self.t(...)` / `self.tr(...)`. See [i18n](#i18n--多语言).
4. **The window renders its own title bar.** Do not reintroduce native OS chrome. Use
   `TitleBar` from `gpui_kit::component`. See [GUI](#gui--自渲染标题栏).
5. **Drive the GUI through the accessibility tree, not the mouse.** Use the UIA scripts and
   `InvokePattern`; synthetic clicks are forbidden (DPI artifacts + user interference).
   See [Verification](#gui-verification--无障碍树驱动).
6. **Stop the running GUI before rebuilding on Windows**, or the build fails with a file-lock
   `os error 5`. See [Verification](#gui-verification--无障碍树驱动).
7. **Keep `core` free of GUI/transport dependencies.** `core` is pure logic + storage; `mcp` and
   `app` depend on it, never the reverse.

---

## Toolchain & commands / 工具链与命令

Pinned in `mise.toml`: **rust 1.99.0**, **cmake latest**. Workspace is **edition 2024**,
**MSRV 1.95** (`Cargo.toml`). `resolver = "3"`.

```sh
mise install          # one-time: install the pinned rust + cmake
mise run build        # cargo build --workspace
mise run check        # cargo clippy --workspace --all-targets -- -D warnings
mise run test         # cargo test --workspace
mise run run          # cargo run -p agent-control-app   (desktop console)
mise run mcp          # cargo run -p agent-control-mcp -- --stdio
mise run fmt          # cargo fmt --all
mise run release      # cargo build --release + package dist/ portable zip
```

Ad-hoc cargo (still through mise): `mise exec -- cargo <args>`.

**Definition of done for any code change:** `mise run fmt` → `mise run check` (zero warnings,
`-D warnings`) → `mise run test` all green. GUI changes additionally require the
accessibility-tree verification below.

---

## Workspace map / 工作区结构

Three crates under `crates/`. Dependency direction: `app → {mcp, core}`, `mcp → core`,
`core → (nothing internal)`.

| Crate | Package | Kind | Responsibility | Key files |
|-------|---------|------|----------------|-----------|
| core | `agent-control-core` | lib | Rule engine, provenance, conflict detection, hybrid storage, evidence hash chain, handoff bundles, project registry, native pointer export + drift + revoke, assurance tiers, tool-call gate, hooks config | `engine.rs` (`RuleEngine`), `model.rs` (`Rule`, `Scope`, `Enforcement`, `AssuranceTier`, `ConstraintAssurance`, `ToolGuard`, `ToolGateReport`, `Project`, `Session`, `EvidenceEntry`, `canonical_hash`, …), `conflict.rs` (`evaluate_code_checks`), `gate.rs` (`check_tool_call`, repo gate + tool guards), `hooks.rs` (`HookTarget`, `ConfigCodec`, `HookScope`, `HookEvents`, `register`/`revoke`/`detect`), `export.rs` (`PROJECT_GUIDANCE`, `render_constraint_briefing`, `handoff_tier_warning`, `render_handoff_brief`, `wrap_block`, `upsert_managed_block`, `remove_managed_block`, `ExportTarget`, `ExportReport`, `RevokeReport`, `DriftReport`), `config.rs` (`DataLayout`), `store/{mod,db,files}.rs` (`Store`, `Db`) |
| mcp | `agent-control-mcp` | lib + bin | MCP server (rmcp): 28 tools + resources + prompts; stdio & Streamable HTTP | `lib.rs` (`ControlServer`, `serve_http`), `main.rs` (CLI) |
| app | `agent-control-app` | bin | GPUI desktop console (gpui-kit), eight views, branded design system, bilingual UI, self-rendered title bar, project tree + onboarding, suggestion review, native export + hooks registration | `main.rs`, `console.rs` (`Console` entity), `i18n.rs`, `theme.rs` (brand tokens), `ui.rs` (shared view helpers), `views/*.rs` |

Public surface of `core` (re-exported from `lib.rs`): `DataLayout`, `RuleEngine`, `Store`,
`RuleExplanation`, `AcknowledgmentReport`, `ConstraintVerificationReport`, `ExportReport`,
`ExportTarget`, `RevokeReport`, `TargetOutcome`, `DriftReport`, `TargetDrift`, `HookReport`,
`HookScope`, `HookStatus`, `HookTarget`, `render_constraint_briefing`, `handoff_tier_warning`, and all of
`model::*`.

`Store` entry points: `Store::open(layout)`, `Store::open_default()` (`~/.agent-control`),
`Store::open_with_root(path)`. Rules load via `load_rules(project_path: Option<&Path>)`
(user scope always; project **and environment** scope when a path is given). Write rules via `save_rule(rule,
project_path)` (overwrite) or `upsert_rule(rule, project_path) -> (Rule, PathBuf)` (create-or-update
by id: bumps version, preserves original `created_at` **and `enabled`**; backs the MCP `save_rule`
tool). Toggle enablement with `set_rule_enabled(rule_id, scope, project_path, enabled) -> Rule` —
the only write path for `enabled` (bumps version, preserves content; backs the MCP
`set_rule_enabled` tool and the Rules-view Switch). Disabled rules are excluded by `RuleEngine`
(`Decision::Disabled`) and never bind sessions; a content update via `save_rule`/`upsert_rule`
never silently re-enables them. Build fresh rules with
`store::new_rule(id, title, content, kind, scope, author)` (`new_user_rule` is a preference/user
wrapper). Session work state is written via `set_session_work_state(id, goal, work_items) -> Session`
(replaces the goal + done/doing/todo list; backs MCP `update_work_state` and the Sessions-view work
editor). `delete_session(id)` removes a session and, **cascade**, its evidence chain + provenance
record (backs the Sessions-view Delete button / row menu, behind a confirm dialog).
`create_session(task, project_path, agent_id, task_tags) -> (Session, bool)` is **get-or-create**: an
**active** session with the same project + agent + task is returned unchanged (`reused = true`), so
repeated calls from one harness conversation never pile up duplicate sessions; a new task or a closed
source session freezes a fresh snapshot. `set_session_status(id, status) -> Session` closes
(`Completed`/`Failed`, appending a `session_end` evidence entry) or reopens (`Active`) a session;
`HandedOff` is set only by `export_handoff` (whose `to_agent` is an optional receiver hint — the real
receiver is chosen at `import_handoff`). Each `export_handoff` marks every prior **unacknowledged**
`HandoffRecord` as `superseded` (a `#[serde(default)]` bool, so old session JSON needs no migration):
only the latest handoff can be acknowledged, and superseded records no longer read as pending action.
`acknowledge_rules` still marks only the last record acknowledged; acknowledged records are never
superseded.

**Native export** (`core/src/export.rs` + `Store::export_native`): writes a *static pointer block*
into the selected harness entry points so agents that never call MCP still learn where the contracts
live. `export_native(project_path, targets: &[ExportTarget], include_user) -> ExportReport` loads the
**enabled** rules (to report counts) and writes a managed block — `<!-- agent-control:begin --> …
<!-- agent-control:end -->` — into each selected target. The block body is the constant
`PROJECT_GUIDANCE`: it leads with **discovery** — "start by understanding this project before changing
it" — and tells the agent to **read and summarize** the enabled `contract`/`constraint` rules in
`.agent-control/rules/` (priority wins, `applies_to` narrows, skip `enabled: false`), then personal
preferences in `~/.agent-control/rules/user/`, and to prefer MCP `get_applicable_rules` for precise
resolution. It notes that machine/project-local environment notes live in git-ignored
`.agent-control/local/` and reach the agent only through the briefing/hook — **never** this committed
file. It does **not** copy rule text, so the body never varies with the rule set: editing a rule never
makes the block stale and re-export is byte-idempotent. `export_native` loads only the enabled
`Project`/`User` rules for its counts; `Environment` rules are deliberately excluded from every
committed target. `ExportTarget` (`Agents` | `Claude` | `Gemini` | `Cursor`,
serde lowercase) selects the files via `DataLayout::project_agents_md` / `project_claude_md` /
`project_gemini_md` / `project_cursor_rule` (the Cursor file gets `CURSOR_FRONTMATTER` *outside* the
block); `default_targets()` is `[Agents]` only — writing every harness at once is opt-in, because Claude
Code reads only `CLAUDE.md` when both it and `AGENTS.md` exist. `include_user` also writes the constant
`USER_GUIDANCE` pointer to `DataLayout::claude_user_md()` (`~/.claude/CLAUDE.md`). The pure helpers
`wrap_block(body)` (embeds `GENERATED_NOTE` + the `FORMAT_PREFIX` version line),
`upsert_managed_block(existing, block)` and `remove_managed_block(existing)` preserve everything outside
the markers, append when absent, and error on a lone marker. The project guidance ends with a static
`### Continuity (optional)` section (deliberately last, after the read-and-summarize steps and
`### Assurance`) telling the agent to read `DataLayout::project_handoff_latest`
(`.agent-control/handoff/latest.md`) if it exists and that it may refresh it via `export_handoff`. The block carries
**no trailing newline**; the file's terminating newline is added on append so it survives as the suffix
after the end marker — this keeps re-export **byte-idempotent** (`TargetOutcome.action` = `created` |
`updated` | `unchanged`; unchanged files are not rewritten). Backs the MCP `export_native` tool and the
Projects-view target checkboxes + **Export to harnesses** button
(`Console::export_native_for_selected_project`, project-only, `Console::export_targets`). The companion
`scripts/agent-control-hook.mjs` is a dependency-free Claude Code **SessionStart** hook that calls
`get_applicable_rules` over MCP HTTP and injects a briefing via `hookSpecificOutput.additionalContext`;
it is read-only and exits `0` silently when the daemon is unreachable. The hook also calls
`check_export_drift` and appends a warning when a target is missing the current-format block (it prompts
a re-export, never rewrites). Reach ≠ verify ≠ enforce — see **Assurance tiers** below.

**Export drift & revoke** (`core/src/export.rs` + `Store::export_drift` / `Store::revoke_native`): the
managed block embeds a *block-format version* (`FORMAT_PREFIX = "<!-- agent-control:block-format:"`,
`BLOCK_FORMAT = 2`); `read_exported_format` extracts it (a legacy v1 full-text block carried a ruleset
hash instead, so it reads as `None`). `export_drift(project_path, targets, include_user) -> DriftReport`
reports `TargetDrift { path, exists, exported_format: Option<u32>, current_format, stale }` per target;
`stale = exists && exported_format != Some(BLOCK_FORMAT)` — a never-exported file is **not** stale, and a
rule edit does **not** make a current pointer stale (the body is static). Drift therefore means "missing
block or an older format" (e.g. a v1 block to migrate), detecting **file-vs-store format** drift (layer
1) only, **not** whether a running session's injected context is stale (layer 2).
`revoke_native(project_path, targets, include_user) -> RevokeReport` is the explicit undo: it strips the
block via `remove_managed_block`, preserving surrounding prose (`action = removed`), deletes a file that
held only the block or only the generated Cursor frontmatter (`deleted`), and leaves a block-free target
`unchanged`. Backs the MCP `check_export_drift` / `revoke_native` tools, the hook warning, the
Projects-view **export stale** badge (`Console::export_stale`, refreshed in
`refresh_data`/`select_project`/export — never at render time) and the **Revoke** button
(`Console::confirm_revoke_native` → `revoke_native_for_selected_project`).

**Assurance tiers** (`core/src/model.rs` + `Store::verify_constraints`): two orthogonal axes.
`Rule.enforcement: Enforcement` (`Advisory` | `Verify` | `Hard`, serde lowercase, `#[serde(default)]`
→ `Advisory`) is the *declared intent* — whether a rule is mechanically checkable; it round-trips
through frontmatter and is accepted by `save_rule` / `suggest_rule`. `AssuranceTier` (`Advisory` <
`Verified` < `Enforced`) is the *achieved* tier, computed from evidence. `ConstraintAssurance {
rule_id, declared, achieved, basis }` is recorded per session in `Session.assurance:
Vec<ConstraintAssurance>` **beside** the frozen snapshot (the `snapshot_hash` over rules is unchanged)
and carried in `HandoffBundle.assurance`. `verify_constraints(session_id, agent_id) ->
ConstraintVerificationReport { session_id, agent_id, assurances, verified, advisory, enforced }`
evaluates the snapshot's `code_checks` via `conflict::evaluate_code_checks` and sets each rule's
achieved tier: no checks → `Advisory` (semantic), all pass → `Verified`, any fail/inconclusive →
`Advisory`. It appends a `TestResult` evidence (checks + assurance) and a `ConstraintViolation`
evidence when any check failed. **`Enforced` is never assigned here** — it requires a deny-capable
chokepoint (the PreToolUse gate, `Store::check_tool_call`), so declaring `Hard` does not enforce;
without a registered hook it caps at `Advisory`/`Verified`. `import_handoff`
surfaces the carried tiers and **warns** on a drop (warn, not block) — the new harness must re-verify.
DB: guarded `ALTER` adds `sessions.assurance_json` (DEFAULT `'[]'`) and `rule_suggestions.enforcement`
(DEFAULT `'advisory'`). Backs the MCP `verify_constraints` tool, the Rules-view enforcement selector +
declared tag, and the Sessions-view achieved tiers + **Verify constraints** button.

**Rule suggestions** (`Store`, backed by the SQLite `rule_suggestions` table): the human-in-the-loop
authoring path. `submit_suggestion(draft)` queues a `Pending` draft (build it with
`store::new_suggestion(project_path, agent_id, id, title, content, kind, scope)`); `list_suggestions(
project_path?, status?)` and `count_suggestions(project_path?, status?)` read the queue;
`get_suggestion(id)` fetches one. `approve_suggestion(id, SuggestionEdits, note?) -> (RuleSuggestion,
Rule, PathBuf)` applies optional human edits (`SuggestionEdits` fields default to the agent draft),
writes a binding rule via the same `upsert_rule` path as `save_rule`, and marks the suggestion
`Approved` with `approved_rule_id`. `reject_suggestion(id, note?)` marks it `Rejected` and writes
**no** rule. `RuleSuggestion` / `SuggestionStatus` (`Pending`/`Approved`/`Rejected`) / `SuggestionEdits`
live in `model.rs`. Backs the MCP `suggest_rule` / `list_suggestions` tools and the app Suggestions view.

A suggestion carries a `scope` (`Scope`, serde-default `project`): `Project` requires a non-empty
`project_path` (normalized on submit) and approval writes `<project>/.agent-control/rules/`;
`Environment` also requires a non-empty `project_path` and approval writes the git-ignored
`<project>/.agent-control/local/`; `User` clears `project_path` to empty and approval writes
`~/.agent-control/rules/user/` (a personal preference that follows the user across projects);
`Session` is rejected. The `rule_suggestions` table stores `scope` as its TEXT label
(`scope TEXT NOT NULL DEFAULT 'project'`, added by a guarded `ALTER` migration for older DBs), so
adding `Environment` needed **no** migration. Project-filtered `list_suggestions`/`count_suggestions`
exclude user-scope rows (empty path) but include environment rows (they carry the project path).
**Environment notes** (e.g. "agents run inside WSL; Windows-native tools are callable for
compile/verify") belong in an **environment-scope rule**, NOT a user preference — they describe THIS
machine/project, not a portable personal habit. Propose via `suggest_rule scope=environment` or record
via `save_rule scope=environment` (both need `project_path`); see **Environment scope** below.

**Project registry** (`Store`, backed by the SQLite `projects` table): `register_project(name,
path, description)` (canonicalizes + dedups by path, defaults the name to the folder name),
`list_projects()` (most-recently-opened first), `get_project(id)`, `project_by_path(path)`,
`touch_project(id)`, `rename_project(id, name)`, `remove_project(id)` (**unregisters only — never
deletes repo files**), and `init_project_contracts(path)` (idempotent scaffold of
`<project>/.agent-control/rules/` + `examples/` templates + a bilingual `README.md` guide; preserves
existing files).
`Project` (`model.rs`) = `{ id, name, path, description?, created_at, last_opened_at }`.

### The four layers → where they live

1. **Personal preferences / 个人偏好** — user-scope `Rule`s in `~/.agent-control/rules/user/`.
2. **Project contracts / 项目契约** — project-scope `Rule`s in `<project>/.agent-control/rules/`.
3. **Cross-harness continuity / 跨 harness 连续性** — `ConstraintSnapshot` frozen at
   `create_session`, the session's resumable **work state** (`goal` + `work_items` done/doing/todo,
   written by `update_work_state`), `HandoffBundle` export/import (carries rules + tiers + work state),
   `acknowledge_rules` compliance report, and the
   project-local `.agent-control/handoff/latest.md` continuity brief (file-based reach).
4. **Verifiable evidence / 可验证证据** — SHA-256 hash chain (`canonical_hash`, `EvidenceEntry`),
   `verify_evidence` recomputes and flags tampering/gaps.

### Environment scope / 工作环境作用域

`Scope::Environment` (serde `environment`, rank 3) is a fourth rule scope beside user/project/session:
**descriptive context about this user's current project on this machine** — e.g. "agents run inside
WSL; the repo lives in WSL but Windows-native tools are callable for compile/verify". It is the right
home for machine/project facts that are *not* portable personal preferences (a user preference follows
the user across every project; an environment note is true only here).

- **Non-competitive.** `RuleEngine::resolve` partitions environment rules out of the competition
  grouping: an enabled environment rule whose `applies_to` matches is **always applied** as context —
  it never supersedes or is superseded by a contract/preference, and `rank()` is never used for it.
- **Project-local + git-ignored.** Stored in `<project>/.agent-control/local/`
  (`DataLayout::project_environment_dir`); `Store::ensure_environment_dir` writes a `.gitignore` of
  `*\n!.gitignore` so the notes never get committed. `save_rule`/`set_rule_enabled`/`delete_rule` all
  resolve this dir for `Environment`; `load_rules(Some(project))` loads it alongside project rules.
- **Briefing-only delivery.** `render_constraint_briefing` renders a dedicated `## Work environment
  (this machine/project)` section, so the SessionStart hook + `get_applicable_rules` inject it
  proactively. `export_native` **excludes** environment rules from every committed harness entry point
  (it loads only enabled `Project`/`User` rules) — a machine fact must never leak into a shared
  `AGENTS.md`/`CLAUDE.md`.
- **Snapshot + handoff.** Environment rules freeze into the session snapshot like any applied rule
  (valid for a same-machine handoff); the `import_handoff` re-verify warning covers cross-machine drift.
- **Authoring.** `save_rule scope=environment` (confident) or `suggest_rule scope=environment` (human
  review); both require `project_path`. `init_project_contracts` scaffolds `local/` and an
  `examples/environment-example.md` template (`TEMPLATE_VERSION` 2). The app exposes a **Work
  Environment** nav (`Nav::Environment`, `IconName::Cpu`) rendering the rules view at `Scope::Environment`;
  its editor hides the enforcement selector (environment notes are descriptive, always `Advisory`).

---

## Storage / 存储

Hybrid by design: **git-friendly rule files** (Markdown + YAML frontmatter) + **SQLite** for
sessions, provenance, the evidence chain, projects and rule suggestions. Default root
`~/.agent-control`; override with `--data-dir` (MCP bin) or `Store::open_with_root`.

**Portable mode** (`DataLayout`, `core/src/config.rs`): a `portable.marker` file next to the
executable, or `AGENT_CONTROL_PORTABLE=1`, makes `default_layout()` return a portable layout
rooted at `<exe dir>/data` with the DB at `<root>/db/control.db` (`portable: bool` on
`DataLayout`); `ensure_dirs` creates the `db` parent. `Store::open_default()` (app + MCP bin)
picks this up automatically, so a portable copy never touches `~/.agent-control`. Use it to
isolate test runs; `open_with_root` / `--data-dir` still override everything.

```
~/.agent-control/
├── control.db          # SQLite (rusqlite, bundled): sessions, provenance, evidence chain, projects, rule_suggestions
├── config.toml         # app prefs (TOML): locale ("en"|"zh") + mcp_port (default 9870); migrated once from legacy ui.json
├── agents.yaml         # registered harness commands
├── logs/               # app.log + mcp.log (tracing-appender, non-rolling; visible with no console)
├── rules/user/         # user-scope rule files (committed/reviewed)
└── bundles/            # exported handoff bundles
<project>/.agent-control/
├── rules/              # project-scope contract rules (binding)
├── local/              # environment-scope notes (git-ignored via local/.gitignore; briefing-only, never committed)
├── examples/           # copy-ready rule templates (NOT binding; copy into rules/ to activate)
├── handoff/latest.md   # continuity brief written by export_handoff (the AGENTS.md managed block points here)
├── README.md           # bilingual contract guide (scaffolded by init_project_contracts)
└── .scaffold.json      # template version + per-file sha256, drives edit-safe scaffold migration
```

`DataLayout` (`core/src/config.rs`) owns every path — add new paths there, don't scatter
`join()` literals (`project_rules_dir`, `project_environment_dir`, `project_examples_dir`,
`project_config_dir`, `project_scaffold_marker`, `project_handoff_dir`, `project_handoff_latest`,
`logs_dir`, and the native-export targets `project_agents_md`, `project_claude_md`, `project_gemini_md`,
`project_cursor_rule`, `claude_user_md`). The export targets live at the project
root (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursor/rules/agent-control.mdc`) and user-global
`~/.claude/CLAUDE.md`, **not** under `.agent-control/`; `export_native` only ever touches the managed
pointer block inside them (and `revoke_native` only removes it). Rule files
carry frontmatter: `id`, `title`, `kind`, `scope`, `source`, optional `code_checks`, filters,
priority. `init_project_contracts` scaffolds `rules/`, `local/` (git-ignored environment dir),
`examples/` (contract + constraint + environment templates), and the guide, then **migrates**
previously scaffolded files to the current templates
(`TEMPLATE_VERSION` in `store/mod.rs`). Migration is hash-guarded by `.scaffold.json`: a generated
file is refreshed only while its on-disk sha256 still equals the hash recorded when we wrote it
(user never edited it); user-modified files are always preserved, and legacy scaffolds with no
marker are adopted only when their content already matches the current template. The app calls it
on startup and whenever a project becomes current, so upgrades reach existing projects. Examples
live outside `rules/` so they never load as binding contracts.

---

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

**28 tools:** `get_applicable_rules`, `explain_rule`, `check_conflicts`, `create_session`,
`record_evidence`, `acknowledge_rules`, `get_session_context`, `update_work_state`,
`set_session_status`, `export_handoff`,
`import_handoff`, `verify_evidence`, `list_rules`, `save_rule`, `set_rule_enabled`, `suggest_rule`,
`list_suggestions`, `export_native`, `check_export_drift`, `revoke_native`, `verify_constraints`,
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
`status` = `done|doing|todo`, default `todo`) records the session's resumable **work state** via
`Store::set_session_work_state` — the `/goal` objective plus done/doing/todo progress. It **replaces**
the session's goal + items and returns the updated session. `export_handoff` carries them into the
bundle and `handoff/latest.md` (a **Goal** + **Work state** section), and `import_handoff` /
`get_session_context` surface them, so a new harness inherits progress instead of relying on its own
todo list. Call it as work completes/starts and before handing off.
`export_native` (`project_path`, optional `targets`, optional `include_user`) writes a static pointer
block into the selected native harness entry points via `Store::export_native`. `targets` is a list of
`agents|claude|gemini|cursor` (omitted → `AGENTS.md` only); the block points at the live rules in
`.agent-control/rules/` and does **not** copy rule text, so a rule edit never makes it stale and
re-export is byte-idempotent. Everything outside the block is preserved; it returns an `ExportReport`
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
*verify*; hard enforcement exists only where the gate mediates the call (see **Assurance tiers** above).
`check_export_drift` (`project_path`, optional `targets`, optional `include_user`) returns a
`DriftReport` flagging targets missing the current-format block (embedded block-format version ≠
`BLOCK_FORMAT`, e.g. a legacy v1 full-text block); read-only, prompts re-export, never rewrites, and
never goes stale on a rule edit.
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
   project + agent + task is reused, `reused: true`, instead of piling up duplicates).
3. `acknowledge_rules` — prove you received exactly the snapshot's rules.
4. `record_evidence` — append tool calls / file changes / test results to the hash chain.
5. `verify_constraints` — run the snapshot's `code_checks` against the repo, record achieved tiers.
6. `update_work_state` — record the `/goal` objective + done/doing/todo work items as progress is made
   and before handing off, so continuity carries resumable state (not just rules).
7. `export_handoff` → `import_handoff` + `acknowledge_rules` — move between harnesses (re-verify tiers).
   `export_handoff` carries the goal + work items and also writes
   `<project>/.agent-control/handoff/latest.md` (a readable continuity brief with **Goal** / **Work
   state** sections) beside the JSON bundle, so a harness that never calls MCP can inherit the handoff
   by reading that file (the exported `AGENTS.md` block points at it).
8. `verify_evidence` — recompute the chain, detect tampering.
9. `set_session_status` — `completed`/`failed` when the unit of work ends (appends `session_end`
   evidence, keeps the chain auditable, leaves the active list); `active` reopens. `handed_off` is
   set only by `export_handoff`.

When adding a tool: implement the handler in `crates/mcp/src/lib.rs` with the rmcp `#[tool]`
macro, keep it a thin adapter over `Store`/`RuleEngine` (no business logic in the transport
layer), and extend `scripts/mcp-e2e.mjs` to cover it.

End-to-end checks (Node.js, no deps):

```sh
node scripts/mcp-e2e.mjs        # stdio: rules, conflicts, sessions, evidence, handoff
node scripts/mcp-http-e2e.mjs   # Streamable HTTP transport
```

---

## Enforcement hooks / 强制 hooks

Layer 2 enforcement rides the harness's **own** hooks — no separate runtime host. `core/src/hooks.rs`
is the config counterpart of `export.rs`'s managed block: it upserts/removes only *our* hook groups in
each harness's own config, preserving every other key and every user-authored hook. A group is "ours"
when a handler command contains `HOOK_MARKER` (`agent-control-hook.mjs`), so no extra marker field is
needed that a harness might reject. Nine vendors differ in config schema, so each `HookTarget` maps to
a `ConfigCodec` that owns the upsert/remove/registered text manipulation:

- `NestedJson` — `{"hooks":{Event:[{matcher?,hooks:[{type,command,timeout}]}]}}` (Claude family, Codex, Qoder CN).
- `FlatJson` — `{"version":1,"hooks":{event:[{command,timeout}]}}` (Cursor; command on the definition).
- `NamedBundleJson` — `{"<bundle>":{Event:[…]}}` (Antigravity).
- `Toml` — `[[hooks]]` tables inside a user TOML config, owned via a marker block edited as **text** so user comments survive (Kimi).

Writes are atomic (temp file + rename) and reversible. `HookScope` records where the config lives:
`Project` (in the repo, shared/committed) for seven vendors, `User` (a user-global file) for Kimi and
Antigravity. A user-level hook still only enforces for the matching project because the gate resolves
rules by the call's `cwd`.

- **Targets** (`HookTarget`, serde lowercase; `HookTarget::ALL` is the nine-element list):
  `Claude` (`.claude/settings.json`), `CodeBuddy` (`.codebuddy/settings.json`), `WorkBuddy`
  (`.workbuddy/settings.json` — a separate product shipping the same engine rebranded), `Qoder`
  (`.qoder/settings.json`), `QoderCn` (`.qoder/settings.json` — the China build **shares the
  international build's project file**; only the user home differs, `~/.qoder-cn` vs `~/.qoder`),
  `Codex` (`.codex/hooks.json`), `Cursor` (`.cursor/hooks.json`), `Antigravity`
  (`~/.gemini/config/hooks.json`, user), `Kimi` (`~/.kimi-code/config.toml`, user). Antigravity replaces
  the former Gemini CLI target. `register_hooks` / `revoke_hooks` / `detect_hooks` are thin `Store`
  adapters over `hooks::{upsert, remove, registered}`. **Verified against the Qoder CN SDK** (brand
  map `{user: ".qoder-cn", project: ".qoder"}`): the project config dir is `.qoder`, not `.qoder-cn`.
  Because the two Qoder builds share one project file, `Store::register_hooks` / `revoke_hooks`
  dedupe targets by config path (first target in request order owns the shared entry; one report
  outcome per file), and `detect_hooks` reports both targets against that same file.
- **Two events written together** (`HookTarget::events() -> HookEvents { start, gate }`, default
  `CLAUDE_EVENTS`): the **start** event (`SessionStart`, Cursor `sessionStart`, Antigravity
  `PreInvocation`; no matcher) injects the binding-rule briefing via `get_applicable_rules` (reach);
  the **gate** event (`PreToolUse`, Cursor `preToolUse`; empty matcher = every tool) runs the gate via
  `check_tool_call` (enforce). Registering is idempotent (`created|updated|unchanged`); revoking prunes
  the events/hooks we emptied and deletes block-only files (`removed|deleted|unchanged`).
- **The gate** (`core/src/gate.rs`, `evaluate_gate`): pure evaluation over the enabled `Hard` rules.
  A hard rule denies when its **repo gate** (`code_checks` against real files) finds the repo
  non-compliant/inconclusive, OR a **tool guard** (tool-name glob + input regex + reason) matches the
  call. Otherwise it mediated the call and counts toward `Enforced`. `Store::check_tool_call` wraps it,
  records a `ToolGate` evidence entry + achieved tiers, and returns `ToolGateReport`. No hard rules →
  allowed (fail-open). This is the **only** path that produces `AssuranceTier::Enforced`.
- **The hook script** (`scripts/agent-control-hook.mjs`, dependency-free Node) serves all nine vendors;
  the registered command bakes in `vendor` + `event` as argv (authoritative even when a harness omits
  `hook_event_name` in stdin). It selects one of three I/O shapes: **claude** (Claude family, Codex,
  Qoder CN, Kimi — exit **2** + stderr deny, briefing via `hookSpecificOutput.additionalContext`),
  **cursor** (always valid JSON: `{permission:"allow"|"deny",user_message,agent_message}`, briefing via
  `{additional_context}`), **antigravity** (`{decision:"ask"|"deny",reason}` — silence/`{}` is denial
  since agy 1.4.x, so allow/fail-open must emit `ask`; stdin nests the call under `toolCall.{name,args}`).
  The **start** event resolves a session (get-or-create, task `hook`) to freeze the snapshot, then
  injects the `get_applicable_rules` briefing + a best-effort `check_export_drift` warning where the
  vendor supports injection (Antigravity's `PreInvocation` has no channel → reach is via the exported
  `GEMINI.md` pointer). The **gate** event resolves the same session and calls `check_tool_call` with
  the pending tool name/input **and that `session_id`**, so a mediated hard rule earns `Enforced` and
  the verdict lands in the evidence chain; on `allowed: false` it blocks in the vendor's deny shape.
  **Fail-open by default** (unreachable/slow daemon → the vendor's allow output, never a spurious
  block); set `AGENT_CONTROL_HOOK_FAIL_CLOSED` to opt into **fail-closed**, where a *gate* invocation
  that cannot reach a verdict denies (the start event always stays fail-open). A hard guard timer
  (`AGENT_CONTROL_HOOK_TIMEOUT_MS`, default 4000) emits the verdict-shaped output on timeout.

Tested by `scripts/hook-e2e.mjs` (start briefing + gate deny/allow in each vendor's shape, hook-only
`Enforced` recording, fail-open and opt-in fail-closed against a throwaway repo) and the gate/hooks
codec unit tests in `crates/core` (per-codec upsert/remove/registered text manipulation, no disk writes
to the real user home). Run with `mise run e2e`. When you change the hook flow, update the script + the
e2e together.

---

## i18n / 多语言

Bilingual **English / 简体中文**, runtime-switchable in Settings, persisted to `config.toml` (`locale`).

- All strings live in the static table `STRINGS: &[(&str, &str, &str)]` in
  `crates/app/src/i18n.rs`, as `(key, English, 简体中文)` rows. Keys are namespaced
  (`nav.*`, `header.*`, `dash.*`, `proj.*`, `rules.*`, `scope.*`, `prov.*`, `decision.*`,
  `reason.*`, `ev.*`, `sess.*`, `sugg.*`, `desc.*`, `kind.*`, `set.*`, `ph.*`, `log.*`, `lang.*`,
  `common.*`, `mcp.*`, `footer.*`).
- Lookup: `i18n::t(locale, key) -> &'static str` (unknown key falls back to the key itself).
  Templates: `i18n::tr(locale, key, &[args]) -> String` replaces each `{}` **in order**.
- In views/console use the entity helpers **`self.t(key)`** and **`self.tr(key, &[args])`**.
  `Console` holds `pub locale: Locale`; `set_locale(locale, window, cx)` saves `config.toml`,
  re-localizes every `InputState` placeholder, and calls `cx.notify()`.
- `Locale` is `En` (default) / `Zh`; `code()` → `"en"`/`"zh"`, `from_code()` parses.

**To add a UI string:** add a row to `STRINGS` with both languages, then reference it via
`self.t`/`self.tr`. Never inline an English or Chinese literal in a view. Placeholders use
`ph.*` keys and are set through `InputState::set_placeholder` (runtime-localized on switch).

---

## GUI / 自渲染标题栏

Built on **gpui-kit 0.9** (`gpui-pre` + `gpui-component` + `gpui-base`; docs at
<https://nsh.dev/docs/>). Branded dark theme applied in `main.rs` via `theme::apply_brand`
(`Theme::update`): violet primary `#7c6af0`, background `#0e1116`, semantic tokens
(`sidebar`, `status_bar`, `group_box`, `radius`/`radius_lg`). Eight views under
`crates/app/src/views/`: `dashboard`, `projects`, `rules`, `suggestions`, `provenance`, `evidence`,
`sessions`, `settings`. `Console` (`console.rs`) is the root entity: grouped icon sidebar
(Workspace / Verify / System) + header (title, project switcher, refresh) + content + status bar,
and owns the `Store`.

**Shared view helpers** (`crates/app/src/ui.rs`): reuse these instead of re-rolling surfaces —
`card(cx)` (standard bordered `group_box` panel), `section_label(cx, text)` (heading with a brand
accent bar), `empty_state(cx, IconName, title, desc)`, `stat_card(cx, icon, label, value, tint)`,
`kv(cx, key, value)`, `dot(color)`, `mono(cx, text)` (monospace hash/id). Icons come from
`gpui_kit::assets::IconName` (only the runtime-served default set renders). Icon tiles/circles must
be **flex containers** (`h_flex()`), not plain `div()` — a block `div` ignores
`items_center`/`justify_center`, leaving the icon pinned top-left. Destructive actions
(rule delete, project remove, suggestion reject) go through `window.open_alert_dialog` confirmations.

**App icon & release:** the brand mark lives at `crates/app/assets/icon.{png,ico}` (violet tile,
`>` chevron, inverted-C canopy over an "A" on a baseline). `crates/app/build.rs` embeds the `.ico`
+ version info into the Windows exe via a windows-only `winresource` build-dependency.
`mise run release` → `scripts/package-release.mjs` assembles `dist/agent-control-v<ver>-windows-x64.zip`
(exes + both READMEs (EN `README.md` + zh-CN `README.zh-CN.md`) + icon + hook script +
`portable.marker`, portable by default).

**Self-rendered title bar (no native OS chrome):**

- Import `TitleBar` from the **component root**: `use gpui_kit::component::TitleBar;`
  (the `title_bar` module is private — `component::title_bar::TitleBar` will not compile).
- `main.rs` builds `WindowOptions` from `TitleBar::window_options()` (sets
  `appears_transparent: true` + `app_owns_titlebar_drag: true`) and
  `TitleBar::title_bar_options()`, with `title: Some("Agent Control".into())`.
- On Windows `appears_transparent` removes the native caption (WM_NCCALCSIZE); `TitleBar`
  renders the drag region, its children, and the min/max/close buttons.
- Render it as `TitleBar::new().child(<h_flex …>)` at the top of `Console::render`.

Give interactive elements **stable ids and accessible names** (e.g. nav buttons use
`(Nav::Dashboard, "nav.dashboard", "nav-dashboard")` + `self.t(key)`) so the UIA tree is
deterministic.

### Projects, scope & the tree / 项目、作用域与树

`Console` tracks two project pointers: **`current_project`** (the project that sessions,
project-scope rule edits and conflict resolution bind to; defaults to the most-recently-opened)
and **`selected_project`** (the tree highlight / detail pane; defaults to current). Helpers:
`current_project()`, `current_project_path()`. The header chip reflects `current_project` and
clicking it opens the Projects view. `refresh_data(cx)` reloads projects, repairs dangling
pointers, reloads rules for the current project path, and rebuilds the tree.

The Projects view (`views/projects.rs`) renders a **gpui-kit tree**
(`gpui_kit::component::tree::{tree, TreeState, TreeItem}`; `Console` owns
`project_tree: Entity<TreeState>`). `rebuild_project_tree(cx)` builds one root per project
(id `project:<uuid>`) with **Contracts** (`contracts:<uuid>`, leaves `rule:<uuid>:<rule_id>`),
**Sessions** (`sessions:<uuid>`) and **Conflicts** (`conflicts:<uuid>`) children. The `tree(...)`
render closure returns a `ListItem` with `.accessibility_label(...)` and an `.on_click` that
captures a `WeakEntity<Console>` + the node id and calls `handle_tree_click(node_id, window, cx)`
(prefix-dispatch → select / activate project + jump to the matching view). Per-project counts are
precomputed into `project_stats: HashMap<Uuid,(usize,usize)>` during rebuild — **no disk I/O at
render time**. `select_project` (root-row click) only moves the highlight via `set_selected_index`;
it must **not** call `rebuild_project_tree`, because `set_items` resets expand/collapse state and
the row's own click toggle would then flip it again (a root could never stay open).

**Onboarding** (`show_onboarding`): browse via `browse_project_folder` (native
`cx.prompt_for_paths(PathPromptOptions{ directories: true, .. })`, awaited in a detached
`cx.spawn_in` task that writes the picker result back into the input), then `onboard_project`
registers + scaffolds contracts + sets current/selected. Guidance copy lives under `proj.*` i18n
keys.

**Rules scope separation** (`views/rules.rs`): scope is driven by the **sidebar nav**, not a tab
switcher — `Nav::Preferences` ("Personal Preferences") renders the rules view with `Scope::User`,
`Nav::Rules` ("Project Contracts") with `Scope::Project`, and `Nav::Environment` ("Work Environment",
`IconName::Cpu`) with `Scope::Environment`
(`let tab = match self.nav { Nav::Preferences => Scope::User, Nav::Environment => Scope::Environment, _ => Scope::Project }`).
The list filters `rule.scope == tab` (original indices preserved for editing); the editor's scope is
derived from the nav — there is **no free-text project path input** and no in-view tab toggle. With no
current project, the Project **and Environment** views show guidance + a `rules-go-projects` shortcut
instead of the form (`project_blocked` matches both scopes). `save_rule_from_inputs` /
`delete_selected_rule` / `set_selected_rule_enabled` resolve the project path from
`current_project_path()` for `Project` **and** `Environment`. The Environment editor hides the
enforcement selector and forces `Advisory` on save (environment notes are descriptive context, never
enforced). When a rule is selected the editor shows an **enable/disable Switch**
(`rule-enabled-switch`, `gpui_kit::component::switch::Switch`, controlled by `rule.enabled`); its
`on_click` calls `set_selected_rule_enabled(next, cx)` → `Store::set_rule_enabled`, then refreshes and
toasts. The list row's on/off `Tag` is a read-only status indicator. Enablement is independent of
content edits: saving a rule preserves its `enabled` state.

**Suggestions review** (`views/suggestions.rs`): the human-in-the-loop queue for agent-proposed
rules. `Console` holds `suggestions: Vec<RuleSuggestion>`, `pending_suggestions: usize` (sidebar
Badge + Dashboard banner), `selected_suggestion: Option<Uuid>`, `suggestion_filter:
Option<SuggestionStatus>` (Pending/Approved/Rejected/All tabs), and draft inputs
(`sugg_rule_id_input`, `sugg_title_input`, `sugg_content_input`, `sugg_note_input`).
`select_suggestion` loads the draft into the editor; `approve_selected_suggestion` reads the inputs
into `SuggestionEdits` + note and calls `Store::approve_suggestion` (writes a binding rule at the
suggestion's scope); `confirm_reject_suggestion` opens an AlertDialog then calls
`Store::reject_suggestion` (writes no rule). Each row and the detail header show a **scope tag**
(`scope.user.tag` info / `scope.project.tag` primary / `scope.environment.tag` warning) beside the
status + kind tags; the approve button label, edit hint and success toast are scope-aware
(`sugg.approve_user` / `sugg.edit_hint_user` / `sugg.approved_toast_user` for user scope, and the
`*_environment` variants for environment scope). Reviewed suggestions render
read-only with status, review note and `approved_rule_id`. `refresh_data` reloads the queue + pending
count and repairs a dangling selection.

**Sessions lifecycle, work state & deletion** (`views/sessions.rs`): the selected-session detail keeps the
snapshot summary, binding constraints, achieved assurance and handoffs in the open, and collapses the rest.
The header carries a **Mark completed** / **Reopen** button (`set_selected_session_status` →
`Store::set_session_status`, toast feedback) beside the danger **Delete session** button; the list
header has a **Show completed** Switch (`Console.session_show_completed`, default off) that hides
Completed/Failed rows so finished work leaves the active list. The **Handoff bundle** panel has no
target input: a hint + **Export bundle** button (`Console::export_handoff`, `to_agent = None`) plus
mono rows showing the deterministic bundle path (`<root>/bundles/<session_id>.json`) and the
project-local `handoff/latest.md` brief path; handoff history rows render an unspecified receiver as
`sess.unspecified` and tag each record acknowledged (`sess.ack`) / superseded (`sess.superseded`,
a newer export replaced it) / pending ack (`sess.pending_ack`).
A **Work state** section shows a locale-formatted last-updated line (`sess.work_updated`,
`session.updated_at` in local time) above the `goal` plus read-only done/doing/todo rows (`work.*` tags); an
`ui::disclosure` ("sess-work-edit") holds the editor — two `TextareaState`s (`session_goal_input`,
`session_work_input`) + a format hint + **Save** (`save_session_work_state` → `Store::set_session_work_state`).
Work items are edited as one `status: title` line each (`parse_work_items` / `format_work_items`; an
unknown/missing prefix defaults to `todo`). The snapshot hash + provenance id moved out of the header
into a collapsed **Verification details** disclosure ("sess-verification") — the full chain stays in
the Evidence/Provenance views. A danger **Delete session** button (header) and a row right-click menu
call `confirm_delete_session` → `delete_selected_session` → `Store::delete_session` (cascade evidence +
provenance), behind an AlertDialog whose copy states the cascade. `select_session(id, window, cx)`
moves the highlight, refreshes evidence and reloads the work inputs; `load_session_work_inputs` mirrors
the selected session into the textareas (empty when none). Disclosure open/closed state lives in
`Console.open_sections: HashSet<String>` toggled by `toggle_section(key, cx)` (shared with the
Projects export/hooks help disclosures).

**Projects detail & hooks feedback** (`views/projects.rs`): the detail pane groups actions (project
actions / export cluster / hooks cluster) separated by dividers, and tucks verbose guidance into
`ui::disclosure` panels ("proj-export-help", "proj-hooks-help"). The Hooks cluster shows a summary tag
(`proj.hooks_summary`, registered/total) plus per-target registered/installed tags from `detect_hooks`;
**Register** / **Revoke** emit success **and** failure toasts (`proj.hooks_register_toast` /
`hooks_revoke_toast` / `hooks_failed_toast` / `hooks_no_script_toast`) so the outcome is always visible.
The export cluster leads with a primary **Setup** button (`project-setup`,
`Console::setup_for_selected_project` → `Store::setup`): one click wires the default export + all
nine hook targets, with `log.setup_done` / `log.setup_failed` toasts.

---

## GUI verification / 无障碍树驱动

The window exposes a full **UI Automation (AccessKit)** tree. Drive it by element name +
`InvokePattern`, never by synthetic mouse clicks (immune to user interference and DPI scaling;
works even when the Windows session is locked).

PowerShell helpers in `scripts/` (use the window handle `HWND`):

```powershell
scripts/uia-dump.ps1   -Hwnd <hwnd>                 # walk the control-view tree (names/roles/patterns)
scripts/uia-invoke.ps1 -Hwnd <hwnd> -Name "Settings" # InvokePattern: press a button (-Last = last match, e.g. a confirm dialog's OK)
scripts/uia-toggle.ps1 -Hwnd <hwnd> -Name "AGENTS.md" # TogglePattern: flip a Checkbox/Switch (Invoke cannot)
scripts/uia-setvalue.ps1 -Hwnd <hwnd> -Name "C:/path/to/repo" -Value "C:\repo" # ValuePattern: set an Input's text
scripts/uia-shot.ps1   -Hwnd <hwnd> -Out out.png     # capture the window rect (GetWindowRect)
```

`invoke`/`toggle`/`setvalue` match by accessible **Name or AutomationId**; gpui surfaces a control's
label/placeholder as its Name (the internal `.id(...)` is **not** the UIA AutomationId). **Always pass
`-Hwnd`** for a dev window — with no HWND the helpers fall back to the first `agent-control-app`
process, which may be a production install. Run the scripts under the existing `RemoteSigned` policy
(local scripts are trusted); never bypass execution policy.

Typical loop: launch with `mise run run` → find the HWND → `uia-dump` to confirm names →
`uia-invoke` each nav item / the language buttons → `uia-dump` again to assert the localized
names changed → `uia-shot` for pixels. Verify locale persistence by reading `config.toml`.

**Windows gotchas:**

- **Stop the GUI before rebuilding.** A running `agent-control-app.exe` locks the binary;
  `cargo build` then fails with `failed to remove … (os error 5 / 拒绝访问)`. Find and kill it:
  `tasklist | grep agent-control-app` then `taskkill //PID <pid> //F` (Git Bash needs `//`).
- **The exe reserves an 8MB main-thread stack** (`crates/app/build.rs`, Windows
  `/STACK:8388608` link arg). The Projects view's element tree overflows the default 1MB stack
  during gpui layout/paint (silent `thread 'main' has overflowed its stack` exit when opening the
  view); keep the reserve when touching the link args.
- **Locked session ⇒ screenshots show the lock screen / black.** That is not an app bug. The
  UIA tree still returns correct elements, so verify functionally (names, geometry, `config.toml`)
  and note that pixel confirmation is blocked until the session is unlocked.
- **Clickable list rows must be named to be UIA-drivable.** A raw `div().id(...).on_click(...)`
  is pruned from the control view (generic role, no name). Add `.role(Role::Button)` +
  `.aria_label(...)` (see the suggestion/session/rule/provenance rows) so it exposes an Invoke
  node; `Role` comes from `gpui_kit::*`.
- **gpui's AccessKit `Click` is coordinate-based** (it synthesizes a mouse down/up at the node's
  bounds center). Controls scrolled out of view therefore do NOT respond to `InvokePattern`.
  Bring them on-screen first — e.g. resize/move the window via the root `TransformPattern`
  (`Resize`/`Move`) on a tall monitor — then invoke. Prefer this over synthetic mouse clicks.

---

## Coding conventions / 编码约定

- Edition 2024; format with `mise run fmt` (rustfmt defaults). Keep `clippy -D warnings` clean.
- Library crates return `anyhow::Result` / typed errors (`thiserror`); avoid `unwrap()`/`expect()`
  on fallible I/O in `core`/`mcp`. `expect()` is acceptable only for truly-unreachable
  invariant violations (e.g. window creation in `app/main.rs`).
- Business logic belongs in `core`; `mcp` is a thin protocol adapter; `app` is presentation.
- Add paths to `DataLayout`, strings to `STRINGS`, tools to `ControlServer` — extend the existing
  owner rather than duplicating logic.
- Tests: unit/integration in `crates/core/tests/core_tests.rs`; protocol + hook e2e in
  `scripts/*.mjs`. Cover new behavior before declaring done.
- Don't add speculative abstractions, backward-compat shims, or noise comments. Comment only a
  non-obvious *why* (hidden constraint, invariant, Windows/GPUI workaround).

## License

Apache-2.0.