# AGENTS.md

Engineering conventions for agents (and humans) working on **Agent Control** — a Rust-first,
local-first command console that connects different Agents over **MCP / ACP** and makes rule
provenance, conflicts, cross-harness continuity and evidence verifiable.

以开发者为中心、以工程协议为基础的本地优先指挥台。本文件是仓库的工程约定，动手前请先读。

> Read this file before editing. It defines the commands, the crate boundaries, and the
> non-negotiable conventions (i18n, self-rendered title bar, accessibility-first verification).

---

## Golden rules / 铁律

1. **Rust-first.** All product logic lives in the four Rust crates. Node.js appears only in
   `scripts/*.mjs` as dependency-free protocol/mock harnesses for end-to-end checks.
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
7. **Keep `core` free of GUI/transport dependencies.** `core` is pure logic + storage; `mcp`,
   `acp`, `app` depend on it, never the reverse.

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

Four crates under `crates/`. Dependency direction: `app → {mcp, acp, core}`, `mcp → core`,
`acp → core`, `core → (nothing internal)`.

| Crate | Package | Kind | Responsibility | Key files |
|-------|---------|------|----------------|-----------|
| core | `agent-control-core` | lib | Rule engine, provenance, conflict detection, hybrid storage, evidence hash chain, handoff bundles, project registry, native export | `engine.rs` (`RuleEngine`), `model.rs` (`Rule`, `Scope`, `Project`, `Session`, `EvidenceEntry`, `canonical_hash`, …), `conflict.rs`, `export.rs` (`render_body`, `upsert_managed_block`, `ExportReport`), `config.rs` (`DataLayout`), `store/{mod,db,files}.rs` (`Store`, `Db`) |
| mcp | `agent-control-mcp` | lib + bin | MCP server (rmcp): 16 tools + resources + prompts; stdio & Streamable HTTP | `lib.rs` (`ControlServer`, `serve_http`), `main.rs` (CLI) |
| acp | `agent-control-acp` | lib | ACP client / harness manager: launches agent CLIs, injects constraint briefing, streams events, permission round-trips, file-change evidence | `lib.rs` (`HarnessManager`), `connection.rs` (`run_harness`, `HarnessConfig`, `constraint_briefing`), `events.rs`, `fs.rs`, `terminal.rs` |
| app | `agent-control-app` | bin | GPUI desktop console (gpui-kit), eight views, branded design system, bilingual UI, self-rendered title bar, project tree + onboarding, suggestion review | `main.rs`, `console.rs` (`Console` entity), `i18n.rs`, `theme.rs` (brand tokens), `ui.rs` (shared view helpers), `views/*.rs` |

Public surface of `core` (re-exported from `lib.rs`): `DataLayout`, `RuleEngine`, `Store`,
`RuleExplanation`, `AcknowledgmentReport`, `ExportReport`, `TargetOutcome`, and all of `model::*`.

`Store` entry points: `Store::open(layout)`, `Store::open_default()` (`~/.agent-control`),
`Store::open_with_root(path)`. Rules load via `load_rules(project_path: Option<&Path>)`
(user scope always; project scope when a path is given). Write rules via `save_rule(rule,
project_path)` (overwrite) or `upsert_rule(rule, project_path) -> (Rule, PathBuf)` (create-or-update
by id: bumps version, preserves original `created_at` **and `enabled`**; backs the MCP `save_rule`
tool). Toggle enablement with `set_rule_enabled(rule_id, scope, project_path, enabled) -> Rule` —
the only write path for `enabled` (bumps version, preserves content; backs the MCP
`set_rule_enabled` tool and the Rules-view Switch). Disabled rules are excluded by `RuleEngine`
(`Decision::Disabled`) and never bind sessions; a content update via `save_rule`/`upsert_rule`
never silently re-enables them. Build fresh rules with
`store::new_rule(id, title, content, kind, scope, author)` (`new_user_rule` is a preference/user
wrapper).

**Native export** (`core/src/export.rs` + `Store::export_native`): compiles the binding set into each
harness's native entry point so agents that never call MCP still inherit the contracts.
`export_native(project_path, include_user) -> ExportReport` loads **enabled** project rules (and user
rules when `include_user`) and writes a *managed block* — `<!-- agent-control:begin --> …
<!-- agent-control:end -->` — into `DataLayout::project_agents_md` / `project_claude_md` /
`project_cursor_rule` (the Cursor file gets `CURSOR_FRONTMATTER` *outside* the block); `include_user`
also writes personal preferences to `DataLayout::claude_user_md()` (`~/.claude/CLAUDE.md`). The pure
helpers `render_body(rules, heading)` (priority desc then id), `wrap_block(body)` and
`upsert_managed_block(existing, block)` preserve everything outside the markers, append when absent,
and error on a lone marker. The block carries **no trailing newline**; the file's terminating newline
is added on append so it survives as the suffix after the end marker — this keeps re-export
**byte-idempotent** (`TargetOutcome.action` = `created` | `updated` | `unchanged`; unchanged files are
not rewritten). Backs the MCP `export_native` tool and the Projects-view **Export to harnesses**
button (`Console::export_native_for_selected_project`, project-only). The companion
`scripts/agent-control-hook.mjs` is a dependency-free Claude Code **SessionStart** hook that calls
`get_applicable_rules` over MCP HTTP and injects a briefing via `hookSpecificOutput.additionalContext`;
it is read-only and exits `0` silently when the daemon is unreachable. Reach ≠ enforcement: export /
hook deliver rules, `code_checks` + the evidence chain verify them, and hard enforcement only exists
at the ACP permission round-trip.

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
`project_path` (normalized on submit) and approval writes `<project>/.agent-control/rules/`; `User`
clears `project_path` to empty and approval writes `~/.agent-control/rules/user/` (a personal
preference that follows the user across projects); `Session` is rejected. The `rule_suggestions`
table has a `scope TEXT NOT NULL DEFAULT 'project'` column, added by a guarded `ALTER` migration for
older DBs. Project-filtered `list_suggestions`/`count_suggestions` exclude user-scope rows (empty
path). **Environment notes** (e.g. "agents run inside WSL; Windows-native tools are callable for
compile/verify") belong in a **user-scope `constraint` rule** — propose via `suggest_rule scope=user`
or record via `save_rule scope=user`; user rules always load, freeze into every session snapshot, and
are injected proactively via the ACP `constraint_briefing` / `get_applicable_rules`.

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
   `create_session`, `HandoffBundle` export/import, `acknowledge_rules` compliance report.
4. **Verifiable evidence / 可验证证据** — SHA-256 hash chain (`canonical_hash`, `EvidenceEntry`),
   `verify_evidence` recomputes and flags tampering/gaps.

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
├── ui.json             # UI prefs: {"locale":"en"|"zh"}
├── agents.yaml         # registered harness commands
├── logs/               # app.log + mcp.log (tracing-appender, non-rolling; visible with no console)
├── rules/user/         # user-scope rule files (committed/reviewed)
└── bundles/            # exported handoff bundles
<project>/.agent-control/
├── rules/              # project-scope contract rules (binding)
├── examples/           # copy-ready rule templates (NOT binding; copy into rules/ to activate)
├── README.md           # bilingual contract guide (scaffolded by init_project_contracts)
└── .scaffold.json      # template version + per-file sha256, drives edit-safe scaffold migration
```

`DataLayout` (`core/src/config.rs`) owns every path — add new paths there, don't scatter
`join()` literals (`project_rules_dir`, `project_examples_dir`, `project_config_dir`,
`project_scaffold_marker`, `logs_dir`, and the native-export targets `project_agents_md`,
`project_claude_md`, `project_cursor_rule`, `claude_user_md`). The export targets live at the project
root (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules/agent-control.mdc`) and user-global `~/.claude/CLAUDE.md`,
**not** under `.agent-control/`; `export_native` only ever touches the managed block inside them. Rule files
carry frontmatter: `id`, `title`, `kind`, `scope`, `source`, optional `code_checks`, filters,
priority. `init_project_contracts` scaffolds `rules/`, `examples/` (contract + constraint
templates), and the guide, then **migrates** previously scaffolded files to the current templates
(`TEMPLATE_VERSION` in `store/mod.rs`). Migration is hash-guarded by `.scaffold.json`: a generated
file is refreshed only while its on-disk sha256 still equals the hash recorded when we wrote it
(user never edited it); user-modified files are always preserved, and legacy scaffolds with no
marker are adopted only when their content already matches the current template. The app calls it
on startup and whenever a project becomes current, so upgrades reach existing projects. Examples
live outside `rules/` so they never load as binding contracts.

---

## MCP protocol / MCP 协议

Transports (rmcp 3.9): **stdio** and **Streamable HTTP** (axum, route `/mcp`, `mcp-session-id`
header). The desktop app runs MCP as a **background daemon** at `http://127.0.0.1:9870/mcp`:
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

MCP bin CLI (`crates/mcp/src/main.rs`):

```sh
cargo run -p agent-control-mcp -- --stdio                 # stdio (harness spawns this)
cargo run -p agent-control-mcp -- --bind 127.0.0.1:9870   # HTTP (default bind)
cargo run -p agent-control-mcp -- --daemon                # detached background daemon, then exit
cargo run -p agent-control-mcp -- --data-dir <path>       # override data root
```

**Single-instance:** the TCP bind is the lock — a second HTTP launch on a busy address logs
"already serving" and exits cleanly (`AddrInUse` guard), so daemon spawns are idempotent.

**16 tools:** `get_applicable_rules`, `explain_rule`, `check_conflicts`, `create_session`,
`record_evidence`, `acknowledge_rules`, `get_session_context`, `export_handoff`,
`import_handoff`, `verify_evidence`, `list_rules`, `save_rule`, `set_rule_enabled`, `suggest_rule`,
`list_suggestions`, `export_native` (plus resources & prompts).
`save_rule` is the active rule-authoring path: an agent records a discovered project contract
(`scope=project`, `kind=contract`, `project_path`) or personal preference (`scope=user`) as a rule
file via `Store::upsert_rule`; reusing an id updates + bumps version. Use it for durable project
knowledge; `record_evidence` remains for session-scoped actions (fixed `EvidenceKind` enum).
`set_rule_enabled` pauses/resumes a rule without touching its content (`id`, `enabled`, `scope`,
`project_path` for project scope): disabled rules are excluded from resolution and stop binding
sessions, enabling restores them. It is the only tool that flips `enabled` — `save_rule` content
updates preserve the existing state.
`suggest_rule` is the human-in-the-loop alternative for inferred/uncertain rules: it queues a
`Pending` draft via `Store::submit_suggestion` (with a `rationale`) that does **not** bind sessions
until a user approves it in the Suggestions view. `scope=project` (default) requires `project_path`
and approval writes a project contract; `scope=user` needs no `project_path` and approval writes a
personal preference that follows the user across projects (the right home for environment notes).
`list_suggestions` reads the queue (`pending|approved|rejected`) plus a pending count. Prefer
`suggest_rule` when a human should sign off first, `save_rule` when the rule is confident knowledge.
`export_native` (`project_path`, optional `include_user`) compiles the enabled binding set into native
harness entry points via `Store::export_native`: a managed block in the project's `AGENTS.md`,
`CLAUDE.md` and `.cursor/rules/agent-control.mdc` (plus `~/.claude/CLAUDE.md` when `include_user`).
Everything outside the block is preserved and re-export is idempotent; it returns an `ExportReport`
with per-target `created|updated|unchanged`. Use it so agents that read those files inherit the
contracts without an MCP call. The Claude Code **SessionStart** hook
(`scripts/agent-control-hook.mjs`) is the push counterpart: it calls `get_applicable_rules` over MCP
HTTP and injects a briefing into session context, read-only and graceful (exit 0) when the daemon is
down. Neither *forces* compliance — they deliver rules; `code_checks` + the evidence chain verify,
and hard enforcement is limited to the ACP permission round-trip.

Recommended harness call order:

1. `get_applicable_rules` — resolve binding rules + provenance + conflicts before working.
2. `create_session` — freeze the constraint snapshot.
3. `acknowledge_rules` — prove you received exactly the snapshot's rules.
4. `record_evidence` — append tool calls / file changes / test results to the hash chain.
5. `export_handoff` → `import_handoff` + `acknowledge_rules` — move between harnesses.
6. `verify_evidence` — recompute the chain, detect tampering.

When adding a tool: implement the handler in `crates/mcp/src/lib.rs` with the rmcp `#[tool]`
macro, keep it a thin adapter over `Store`/`RuleEngine` (no business logic in the transport
layer), and extend `scripts/mcp-e2e.mjs` to cover it.

End-to-end checks (Node.js, no deps):

```sh
node scripts/mcp-e2e.mjs        # stdio: rules, conflicts, sessions, evidence, handoff
node scripts/mcp-http-e2e.mjs   # Streamable HTTP transport
```

---

## ACP harness / ACP 代理

`agent-client-protocol` 5.0. `HarnessManager` (`crates/acp/src/lib.rs`) owns a small tokio
runtime and spawns `run_harness`, which: launches the agent CLI, injects `constraint_briefing`
as system prompt, wires the MCP endpoint, streams `HarnessEvent`s, handles permission requests
(`PermissionPolicy` / `PermissionDecision`), and records file-change evidence.

Tested by `crates/acp/tests/harness.rs` against the mock agent `scripts/mock-acp-agent.mjs`
(covers a permission round-trip and file-change evidence). Run with `mise run test`. When you
change the ACP flow, update the mock + the test together.

---

## i18n / 多语言

Bilingual **English / 简体中文**, runtime-switchable in Settings, persisted to `ui.json`.

- All strings live in the static table `STRINGS: &[(&str, &str, &str)]` in
  `crates/app/src/i18n.rs`, as `(key, English, 简体中文)` rows. Keys are namespaced
  (`nav.*`, `header.*`, `dash.*`, `proj.*`, `rules.*`, `scope.*`, `prov.*`, `decision.*`,
  `reason.*`, `ev.*`, `sess.*`, `sugg.*`, `desc.*`, `kind.*`, `set.*`, `ph.*`, `log.*`, `lang.*`,
  `common.*`, `mcp.*`, `footer.*`).
- Lookup: `i18n::t(locale, key) -> &'static str` (unknown key falls back to the key itself).
  Templates: `i18n::tr(locale, key, &[args]) -> String` replaces each `{}` **in order**.
- In views/console use the entity helpers **`self.t(key)`** and **`self.tr(key, &[args])`**.
  `Console` holds `pub locale: Locale`; `set_locale(locale, window, cx)` saves `ui.json`,
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
and owns `Store` + `HarnessManager`.

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
(exes + README + icon + `portable.marker`, portable by default).

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
switcher — `Nav::Preferences` (a top-level "Personal Preferences" menu) renders the rules view with
`Scope::User`, and `Nav::Rules` ("Project Contracts") renders it with `Scope::Project`
(`let tab = if self.nav == Nav::Preferences { Scope::User } else { Scope::Project }`). The list
filters `rule.scope == tab` (original indices preserved for editing); the editor's scope is derived
from the nav — there is **no free-text project path input** and no in-view tab toggle. With no
current project, the Project view shows guidance + a `rules-go-projects` shortcut instead of the
form. `save_rule_from_inputs` / `delete_selected_rule` resolve the project path from
`current_project_path()`. When a rule is selected the editor shows an **enable/disable Switch**
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
(`scope.user.tag` info / `scope.project.tag` primary) beside the status + kind tags; the approve
button label, edit hint and success toast are scope-aware (`sugg.approve_user` /
`sugg.edit_hint_user` / `sugg.approved_toast_user` for user scope). Reviewed suggestions render
read-only with status, review note and `approved_rule_id`. `refresh_data` reloads the queue + pending
count and repairs a dangling selection.

---

## GUI verification / 无障碍树驱动

The window exposes a full **UI Automation (AccessKit)** tree. Drive it by element name +
`InvokePattern`, never by synthetic mouse clicks (immune to user interference and DPI scaling;
works even when the Windows session is locked).

PowerShell helpers in `scripts/` (use the window handle `HWND`):

```powershell
scripts/uia-dump.ps1   -Hwnd <hwnd>                 # walk the control-view tree (names/roles)
scripts/uia-invoke.ps1 -Hwnd <hwnd> -Name "Settings" # invoke a button by its accessible name
scripts/uia-shot.ps1   -Hwnd <hwnd> -Out out.png     # capture the window rect (GetWindowRect)
```

Typical loop: launch with `mise run run` → find the HWND → `uia-dump` to confirm names →
`uia-invoke` each nav item / the language buttons → `uia-dump` again to assert the localized
names changed → `uia-shot` for pixels. Verify locale persistence by reading `ui.json`.

**Windows gotchas:**

- **Stop the GUI before rebuilding.** A running `agent-control-app.exe` locks the binary;
  `cargo build` then fails with `failed to remove … (os error 5 / 拒绝访问)`. Find and kill it:
  `tasklist | grep agent-control-app` then `taskkill //PID <pid> //F` (Git Bash needs `//`).
- **Locked session ⇒ screenshots show the lock screen / black.** That is not an app bug. The
  UIA tree still returns correct elements, so verify functionally (names, geometry, `ui.json`)
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
  on fallible I/O in `core`/`mcp`/`acp`. `expect()` is acceptable only for truly-unreachable
  invariant violations (e.g. window creation in `app/main.rs`).
- Business logic belongs in `core`; `mcp`/`acp` are thin protocol adapters; `app` is presentation.
- Add paths to `DataLayout`, strings to `STRINGS`, tools to `ControlServer` — extend the existing
  owner rather than duplicating logic.
- Tests: unit/integration in `crates/core/tests/core_tests.rs` and `crates/acp/tests/harness.rs`;
  protocol e2e in `scripts/*.mjs`. Cover new behavior before declaring done.
- Don't add speculative abstractions, backward-compat shims, or noise comments. Comment only a
  non-obvious *why* (hidden constraint, invariant, Windows/GPUI workaround).

## License

Apache-2.0.