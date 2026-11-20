# Architecture & storage

> Part of the Agent Control engineering conventions. Golden rules, toolchain commands and
> the must-know-before-edit list live in [AGENTS.md](../AGENTS.md); this file holds the detail.

## Workspace map / 工作区结构

Three crates under `crates/`. Dependency direction: `app → {mcp, core}`, `mcp → core`,
`core → (nothing internal)`.

| Crate | Package | Kind | Responsibility | Key files |
|-------|---------|------|----------------|-----------|
| core | `agent-control-core` | lib | Rule engine, provenance, conflict detection, hybrid storage, evidence hash chain, handoff bundles, project registry, native pointer export + drift + revoke, assurance tiers, tool-call gate, hooks config | `engine.rs` (`RuleEngine`), `model.rs` (`Rule`, `Scope`, `Enforcement`, `AssuranceTier`, `ConstraintAssurance`, `ToolGuard`, `ToolGateReport`, `Project`, `Session`, `EvidenceEntry`, `canonical_hash`, …), `conflict.rs` (`evaluate_code_checks`), `gate.rs` (`check_tool_call`, repo gate + tool guards), `hooks.rs` (`HookTarget`, `ConfigCodec`, `HookScope`, `HookEvents`, `register`/`revoke`/`detect`), `export.rs` (`PROJECT_GUIDANCE`, `render_constraint_briefing`, `handoff_tier_warning`, `render_handoff_brief`, `wrap_block`, `upsert_managed_block`, `remove_managed_block`, `ExportTarget`, `ExportReport`, `RevokeReport`, `DriftReport`), `config.rs` (`DataLayout`), `store/{mod,db,files}.rs` (`Store`, `Db`) |
| mcp | `agent-control-mcp` | lib + bin | MCP server (rmcp): 28 tools + resources + prompts; stdio & Streamable HTTP | `lib.rs` (`ControlServer`, `serve_http`), `main.rs` (CLI) |
| app | `agent-control-app` | bin | GPUI desktop console (gpui-kit), six view modules + per-session Sheet, branded design system, bilingual UI, self-rendered title bar, project tree + onboarding, suggestion review, native export + hooks registration | `main.rs`, `console.rs` (`Console` entity), `i18n.rs`, `theme.rs` (brand tokens), `ui.rs` (shared view helpers), `views/*.rs` |

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
wrapper). Session work state is written via
`set_session_work_state(id, goal, work_items, acting_agent_id) -> Session` (replaces the goal +
done/doing/todo list; backs MCP `update_work_state` and the Sessions-view work editor). Every session
mutation appends evidence carrying the **acting** agent (caller-supplied), not `session.agent_id`:
`WorkStateUpdate` on work-state edits, `StatusChange` on every status transition (including reopen),
`SessionRename` on title edits (UX7: `Session.title`, serde default `''`, display falls back to
`task`; `set_session_title(id, title, acting_agent_id)` touches **only** the title so the machine
identity key behind get-or-create / hooks / bundles stays stable; the GUI passes `console` as the
acting identity, and the handoff brief gains a human-readable **Title** line while briefings stay
agent-neutral),
`OwnershipChange` on an `import_handoff` agent flip. `delete_session(id)` removes a session and,
**cascade**, its evidence chain + provenance record (backs the Sessions-view Delete button / row menu,
behind a confirm dialog).
`create_session(task, project_path, agent_id, task_tags, harness_session_id) -> (Session, bool)` is
**get-or-create**: an **active** session with the same project + agent + task is returned unchanged
(`reused = true`), so repeated calls from one harness conversation never pile up duplicate sessions; a
new task or a closed source session freezes a fresh snapshot. When the source session was **closed**
with the same (project, agent, task), the new session records it as `predecessor` (serde default,
guarded `ALTER` column) so chain splits read as one continuity thread instead of duplicates (UX1).
`harness_session_id` (optional, serde
default, guarded `ALTER` column) records the harness-native conversation id to distinguish several
conversations of one harness. `set_session_status(id, status, acting_agent_id) -> Session` closes
(`Completed`/`Failed`, appending `status_change` + `session_end`, then **auto-exporting** the handoff
bundle + brief via `record_handoff` with the closed status preserved — UX6, so a manual export is
redundant) or reopens (`Active`, appending
`status_change`) a session; `HandedOff` is set only by `export_handoff` (whose `to_agent` is an optional
receiver hint — the real receiver is chosen at `import_handoff`).
`ownership_timeline(id) -> Vec<OwnershipSpan>` **derives** the ownership history from the evidence
chain (SessionStart agent + HandoffIn/OwnershipChange flips); `Session.agent_id` stays the current
owner. Each `export_handoff` marks every prior **unacknowledged**
`HandoffRecord` as `superseded` (a `#[serde(default)]` bool, so old session JSON needs no migration):
only the latest handoff can be acknowledged, and superseded records no longer read as pending action.
`acknowledge_rules` still marks only the last record acknowledged; acknowledged records are never
superseded.

**Native export** (`core/src/export.rs` + `Store::export_native`): writes a *static pointer block*
into the selected harness entry points so agents that never call MCP still learn where the contracts
live. `export_native(project_path, targets: &[ExportTarget], include_user, inline) -> ExportReport`
loads the **enabled** rules (to report counts) and writes a managed block — `<!-- agent-control:begin --> …
<!-- agent-control:end -->` — into each selected target. With `inline = false` (default) the block body
is the constant
`PROJECT_GUIDANCE`: it leads with **discovery** — "start by understanding this project before changing
it" — and tells the agent to **read and summarize** the enabled `contract`/`constraint` rules in
`.agent-control/rules/` (priority wins, `applies_to` narrows, skip `enabled: false`), then personal
preferences in `~/.agent-control/rules/user/`, and to prefer MCP `get_applicable_rules` for precise
resolution. It notes that machine/project-local environment notes live in git-ignored
`.agent-control/local/` and reach the agent only through the briefing/hook — **never** this committed
file. It does **not** copy rule text, so the body never varies with the rule set: editing a rule never
makes the block stale and re-export is byte-idempotent. With `inline = true` (opt-in, UX9) the block
additionally carries a `<!-- agent-control:mode:inline -->` marker, a
`<!-- agent-control:ruleset:<sha256> -->` line (`Store::inline_ruleset_hash`, a `canonical_hash` over
the enabled project contract/constraint rules' `{id,title,content}`) and the rule bodies rendered by
`render_inline_contracts` — so a rule edit **does** dirty the block until re-export (the deliberate
v1-style tradeoff, default off). `wrap_block_mode(body, inline, ruleset_hash)` writes the markers;
`read_exported_inline` / `read_exported_ruleset` parse them; pointer blocks carry **no** mode line, so
they stay byte-identical to v2. `export_native` loads only the enabled
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
(`.agent-control/handoff/latest.md`) if it exists — a **local git-ignored mirror**, absent on a fresh
clone — and that it may refresh it via `export_handoff`. The block carries
**no trailing newline**; the file's terminating newline is added on append so it survives as the suffix
after the end marker — this keeps re-export **byte-idempotent** (`TargetOutcome.action` = `created` |
`updated` | `unchanged`; unchanged files are not rewritten). Backs the MCP `export_native` tool and the
Projects-view **Integration** section (UX9): an `AGENTS.md pointer` Switch
(`integration-pointer`, on = `export_native([Agents])`, off = `revoke_native([Agents])`), an opt-in
`Inline-list contracts in AGENTS.md` Switch (`integration-inline`, default off,
`Console::export_inline`) and an `Enforcement hooks` Switch (`integration-hooks`, on =
`register_hooks(HookTarget::ALL)`, off = confirm → `revoke_hooks`), plus the primary **Setup** button.
The GUI writes only `AGENTS.md`; the other three targets remain MCP-only. Handlers:
`Console::set_integration_pointer` / `set_integration_inline` / `set_integration_hooks`. The companion
`scripts/agent-control-hook.mjs` is a dependency-free Claude Code **SessionStart** hook that calls
`get_applicable_rules` over MCP HTTP and injects a briefing via `hookSpecificOutput.additionalContext`;
it is read-only and exits `0` silently when the daemon is unreachable. The hook also calls
`check_export_drift` and appends a warning when a target is missing the current-format block (it prompts
a re-export, never rewrites). Reach ≠ verify ≠ enforce — see **Assurance tiers** below.

**Export drift & revoke** (`core/src/export.rs` + `Store::export_drift` / `Store::revoke_native`): the
managed block embeds a *block-format version* (`FORMAT_PREFIX = "<!-- agent-control:block-format:"`,
`BLOCK_FORMAT = 2`); `read_exported_format` extracts it (a legacy v1 full-text block carried a ruleset
hash instead, so it reads as `None`). `export_drift(project_path, targets, include_user) -> DriftReport`
reports `TargetDrift { path, exists, exported_format: Option<u32>, current_format, inline,
exported_ruleset: Option<String>, current_ruleset: Option<String>, stale }` per target;
`stale = exists && (exported_format != Some(BLOCK_FORMAT) || (inline && exported_ruleset !=
current_ruleset))` — a never-exported file is **not** stale, and a rule edit does **not** make a current
*pointer* stale (the body is static), but **does** make an *inline* block stale (ruleset-hash check).
Drift therefore means "missing block, an older format, or an outdated inline snapshot", detecting
**file-vs-store** drift (layer 1) only, **not** whether a running session's injected context is stale
(layer 2).
`revoke_native(project_path, targets, include_user) -> RevokeReport` is the explicit undo: it strips the
block via `remove_managed_block`, preserving surrounding prose (`action = removed`), deletes a file that
held only the block or only the generated Cursor frontmatter (`deleted`), and leaves a block-free target
`unchanged`. Backs the MCP `check_export_drift` / `revoke_native` tools, the hook warning, the
Projects-view **export stale** badge (`Console::export_stale` + `export_pointer_on`, refreshed in
`refresh_data`/`select_project`/export — never at render time); revoking rides the pointer Switch off
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
   project-local `.agent-control/handoff/latest.md` continuity brief (file-based reach on this
   machine; a git-ignored runtime mirror that may be absent on a fresh clone — cross-machine
   continuity rides MCP or the integrity-verified bundle, not git).
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
├── handoff/            # git-ignored runtime mirror (self-ignoring .gitignore, like local/)
│   └── latest.md       # continuity brief written by export_handoff; local-only, never committed
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

## GUI / 自渲染标题栏

Built on **gpui-kit 0.9** (`gpui-pre` + `gpui-component` + `gpui-base`; docs at
<https://nsh.dev/docs/>). Branded dark theme applied in `main.rs` via `theme::apply_brand`
(`Theme::update`): violet primary `#7c6af0`, background `#0e1116`, semantic tokens
(`sidebar`, `status_bar`, `group_box`, `radius`/`radius_lg`). Six view modules under
`crates/app/src/views/`: `dashboard`, `projects`, `rules` (rendered at three scopes by the nav),
`suggestions`, `sessions`, `settings`, plus `session_sheet` (the per-session detail Sheet).
`Console` (`console.rs`) is the root entity: grouped icon sidebar
(Workspace / Contracts / System) + header (title, project switcher, refresh) + content + status bar,
and owns the `Store`. Nav groups (UX4): **Workspace** = Dashboard, Projects, Sessions & Handoff
(the core destination, kept primary-tinted even when unselected), Suggestions; **Contracts** =
Project Contracts, Personal Preferences, Work Environment; **System** = Settings.

**Per-session Sheet (UX2/UX8)** (`views/session_sheet.rs`): clicking a session card (or its row menu)
opens a gpui-kit `Sheet` (`window.open_sheet`, 620px) carrying **all** session detail and actions —
overview (task, status + agent, derived ownership chain, project, timestamps, snapshot), an action
row (**Mark completed** / **Reopen**, **Verify constraints**, danger **Delete session**; rename sits
in the Sheet header), binding constraints, work state (goal + done/doing/todo + editor disclosure),
achieved assurance, **timeline** (UX3:
day-grouped vertical spine over the evidence chain, per-kind icon + tint via `ui::kind_visual`,
ownership-flip markers on `ownership_change`/`handoff_in`, click a node to expand its payload JSON;
default milestone-only, the **Show telemetry** Switch admits `tool_gate` noise), handoffs (export
button + bundle/brief paths, latest highlighted, superseded collapsed), provenance
(`Store::provenance_for_session`: linked rows plus legacy
NULL-session rows matched by agent + time window, tagged `legacy`), the evidence chain
(integrity tag + entry count + rows) and a verification-details disclosure. `SheetData` is loaded
once on open and on `refresh_data`, never
at render time; the former Provenance/Evidence views and the Verify nav group are gone —
cross-session chain integrity lives in the Dashboard **Chain integrity** stat card
(`Console::chain_stats`) and `doctor`.

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

**Sessions lifecycle, work state & deletion** (`views/sessions.rs`): the list is a **full-width
compact card list** (UX8) **grouped by project** (UX1) with a per-group count; each card shows only
key info — title (UX7), status tag, kind tag (`telemetry` for task `hook`, `continuity` for handoffs /
predecessor / handed off, else `work`), and a meta line (ownership chain or current owner · project ·
rule count · updated-at) plus a `continues <id>` line when `predecessor` links a chain split.
Clicking a card opens the per-session Sheet, which carries **all** detail and actions; there is no
persistent right detail pane. The list header keeps the **Show completed** Switch
(`Console.session_show_completed`, default off) and a **kind filter**
(`Console.session_kind_filter`, `ui::SessionKind` All/Work/Continuity/Telemetry) that hides
Completed/Failed rows so finished work leaves the active list. There is **no manual create panel**
(UX6): sessions arrive from agent activity via MCP/hooks, and humans label them with rename (UX7).
The handoff cluster lives in the per-session **Sheet** (UX6): an **Export bundle** button
(`Console::export_handoff(id, cx)`, `to_agent = None`) plus
mono rows showing the deterministic bundle path (`<root>/bundles/<session_id>.json`) and the
project-local `handoff/latest.md` brief path; only the **latest** handoff record is highlighted
(primary border), other non-superseded records render plainly, and superseded records collapse into
a `sess-handoff-history` disclosure (UX1). Handoff rows render an unspecified receiver as
`sess.unspecified` and tag each record acknowledged (`sess.ack`) / superseded (`sess.superseded`,
a newer export replaced it) / pending ack (`sess.pending_ack`).
A **Work state** section shows a locale-formatted last-updated line (`sess.work_updated`,
`session.updated_at` in local time) above the `goal` plus read-only done/doing/todo rows (`work.*` tags); an
`ui::disclosure` ("sess-work-edit") holds the editor — two `TextareaState`s (`session_goal_input`,
`session_work_input`) + a format hint + **Save** (`save_session_work_state` → `Store::set_session_work_state`).
Work items are edited as one `status: title` line each (`parse_work_items` / `format_work_items`; an
unknown/missing prefix defaults to `todo`). The snapshot hash + provenance id moved out of the header
into a collapsed **Verification details** disclosure ("sess-verification") — the full chain stays in
the per-session Sheet. A danger **Delete session** button (header) and a row right-click menu
call `confirm_delete_session` → `delete_selected_session` → `Store::delete_session` (cascade evidence +
provenance), behind an AlertDialog whose copy states the cascade. `select_session(id, window, cx)`
moves the highlight, refreshes evidence and reloads the work inputs; `load_session_work_inputs` mirrors
the selected session into the textareas (empty when none). Disclosure open/closed state lives in
`Console.open_sections: HashSet<String>` toggled by `toggle_section(key, cx)` (shared with the
Projects export/hooks help disclosures).

**Projects detail & Integration** (`views/projects.rs`): the detail pane groups project actions and a
single **Integration** section (UX9) separated by a divider. Integration leads with the primary
**Setup** button (`project-setup`, `Console::setup_for_selected_project` → `Store::setup`: one click
wires the default export + all nine hook targets, `log.setup_done` / `log.setup_failed` toasts), then
three Switches — `integration-pointer` (AGENTS.md pointer on/off), `integration-inline` (opt-in inline
contract snapshot, default off, with a tradeoff hint line) and `integration-hooks` (register/revoke
all nine harnesses; revoke goes through the confirm dialog). The header carries the **export stale**
badge and the hooks summary tag (`proj.hooks_summary`, registered/total). Verbose guidance and the
read-only per-target registered/installed/scope tags from `detect_hooks` live in one
`ui::disclosure` ("proj-integration-detail"). Register/revoke emit success **and** failure toasts
(`proj.hooks_register_toast` / `hooks_revoke_toast` / `hooks_failed_toast` / `hooks_no_script_toast`).

