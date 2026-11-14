# Agent Control

**English** | [简体中文](README.zh-CN.md)

**A developer-centric, engineering-protocol-based, local-first command console that connects
different Agents over MCP and enforces contracts through their native hooks.**

It does not merely store what you like. It answers the questions that matter when multiple
agents touch one codebase:

- Why did this agent apply this rule?
- Does the rule come from the **user** (personal preference) or the **project** (contract)?
- Does it **conflict** with the current code or with another rule?
- After handing the task to **another harness**, are the same constraints still binding?

**Why not just AGENTS.md, Agent Skills, or per-agent memory?** Those are *delivery formats* — an
agent reads them if it chooses. Agent Control is a *resolution layer* over one rule store that
**compiles into** those formats and adds what they lack: it decides **which** rules bind **this**
agent in **this** context (file paths, languages, task tags), records **why** each rule won, was
superseded, filtered out, or disabled (provenance), checks rules against **real repo state** and
against each other (conflict detection), and freezes the binding set into a snapshot that stays
binding **across harnesses**. The resolved contracts reach agents three ways: **exported** as a
pointer block into the project's `AGENTS.md` / `CLAUDE.md` / `GEMINI.md` / `.cursor/rules` (directing
agents to the live rules, so hand-written prose is preserved), **pushed** into a harness session at
start via a SessionStart hook, or **queried** over MCP. A PreToolUse hook then **enforces** the hard
rules by gating each tool call. The differentiator is **traceable provenance + conflict detection + a
frozen cross-harness snapshot**, delivered into the tools agents already read.

---

## The four layers

| # | Layer | Mechanism |
|---|-------|-----------|
| 1 | Personal preferences | User-scope rules that follow you across projects |
| 2 | Project contracts | Project-scope rules that bind every agent working in a repo |
| 3 | Cross-harness continuity | Frozen constraint snapshots + resumable work state (goal/done/doing/todo) + hash-sealed handoff bundles |
| 4 | Verifiable evidence | SHA-256 hash chain; tampering and gaps are detectable |

> **Three rule scopes, not two.** Alongside **user** (personal preferences, portable across
> projects) and **project** (contracts, committed and reviewed), there is an **environment** scope
> (工作环境): descriptive context for *this user on this machine in this project* — e.g. "agents run
> inside WSL; Windows-native tools are callable for compile/verify". Environment notes are
> **project-local and git-ignored** (`.agent-control/local/`), so they are **never committed** into
> `AGENTS.md`; they reach the agent only through the MCP `get_applicable_rules` briefing or the
> SessionStart hook. They are non-competitive (they never supersede a contract) and are always
> `advisory`. This keeps machine-specific facts out of both your portable preferences and the repo's
> shared contracts.

> **What "verifiable" means here.** The evidence chain and handoff-bundle hashes are SHA-256
> *integrity* seals: they prove a record was not altered after it was written, and detect gaps or
> reordering. They are **not cryptographic signatures** — there is no signer identity or key, and on
> a single machine with one writer they cannot prove the reporting agent was truthful. Independent
> verification comes from `code_checks`, which read the *actual* repo files instead of trusting the
> agent's report.

## Highlights

- **Rust-first**, edition 2024, workspace of three crates (`core`, `mcp`, `app`).
- **Native GUI** built on [`gpui-kit`](https://nsh.dev/docs/) (GPUI) with a branded dark design
  system (violet accent, card surfaces, grouped icon sidebar, status bar). Self-rendered title bar
  (no OS chrome), eight views: Dashboard, **Projects**, Rules & Contracts, **Suggestions**,
  Provenance Explorer, Verifiable Evidence, Sessions & Continuity, Settings.
- **Project registry + guided onboarding.** Projects are first-class: register a repo once
  (browse for its folder, give it a name), and the console scaffolds a bilingual contract guide
  under `<project>/.agent-control/`. A **tree view** organizes each project's contracts, sessions
  and conflicts; the header chip shows the **current project** that user- and project-scope
  actions bind to.
- **Context-aware rule resolution.** The engine filters by `applies_to` (agents, file paths,
  languages, task tags), resolves competition by scope → priority → recency, and flags conflicts
  both rule-vs-rule and **rule-vs-code**: `code_checks` independently read real config files
  (JSON/TOML/YAML/INI/.editorconfig) and compare expected vs actual values. A rule can be paused with
  an **enable/disable** toggle instead of being deleted.
- **Clear scope separation.** Scope is driven by the **sidebar nav**, not an in-view tab
  toggle: **Preferences** (个人偏好) shows user-scope rules that follow you across projects,
  **Project Contracts** (项目契约) shows project-scope rules written into the *current* project's repo
  (no free-text path juggling), meant to be committed and reviewed, and **Work Environment** (工作环境)
  shows machine/project-local notes that are git-ignored and injected only via the briefing.
- **Agent suggestions, human review.** Agents can propose rules with `suggest_rule` instead of
  writing them directly. Drafts land in a SQLite-backed review queue (the **Suggestions** view,
  with a pending-count badge in the sidebar). A user edits, **approves** (the draft becomes a
  binding rule at its proposed scope) or **rejects** it — keeping humans in the loop for inferred
  contracts.
- **Session lifecycle.** `create_session` is get-or-create per (project, agent, task) — repeated
  calls reuse the active session (`reused: true`) instead of piling up duplicates. `set_session_status`
  closes (`completed`/`failed`) or reopens (`active`) a session, appending a `session_end` evidence
  entry so finished work leaves the active list while staying auditable. The Sessions view shows the
  resumable **work state** (goal + done/doing/todo), achieved assurance tiers, and a delete action
  that cascades to the session's evidence + provenance.
- **Bilingual UI (English / 简体中文)** with a runtime switcher in Settings; the choice is
  persisted to `<data root>/config.toml`.
- **MCP server** (rmcp) in both **stdio** and **Streamable HTTP** modes, exposing 25 tools plus
  resources and prompts. The daemon's loopback **port is configurable** in Settings (default
  `9870`) and stored in `config.toml`, so a portable/dev instance can bind a different port and
  stay isolated from production; a sidebar icon button starts/stops the daemon at a glance.
- **Hooks enforcement** for Claude Code / CodeBuddy / WorkBuddy / Qoder: a one-click
  `register_hooks` (or the Projects → Hooks panel) writes a **SessionStart** hook (injects the
  binding-rule briefing) and a **PreToolUse** hook (calls `check_tool_call` to deny tool calls that
  violate `hard` rules) into each harness's `settings.json`. This is the deny-capable chokepoint that
  earns the **Enforced** tier; `detect_hooks` / `revoke_hooks` inspect and undo it.
- **Hybrid storage**: git-friendly Markdown/YAML rule files + SQLite for sessions, provenance
  and the evidence chain.
- **Accessibility-first automation**: the GUI exposes a full UI Automation (AccessKit) tree, so
  tests drive it by element name / `InvokePattern` instead of synthetic mouse clicks.

## Quick start

Project-level dependencies are managed by **mise** (Rust toolchain + cmake):

```sh
mise install          # install rust 1.99.0 + cmake
mise run build        # cargo build --workspace
mise run check        # cargo clippy --workspace --all-targets -- -D warnings
mise run test         # cargo test --workspace
mise run run          # launch the desktop console
mise run mcp          # MCP server in stdio mode (for agent harnesses)
mise run fmt          # cargo fmt --all
```

The desktop app auto-starts the MCP Streamable HTTP endpoint at `http://127.0.0.1:9870/mcp`
(default port; change it in **Settings → MCP server** to isolate instances — stored in `config.toml`).

End-to-end protocol checks (Node.js, no extra deps):

```sh
mise run e2e                    # build the MCP bin, then run all three checks below
node scripts/mcp-e2e.mjs        # stdio MCP: rules, conflicts, sessions, evidence, handoff, gate, hooks
node scripts/mcp-http-e2e.mjs   # Streamable HTTP MCP transport
node scripts/hook-e2e.mjs       # SessionStart/PreToolUse hook behavior against a live daemon
```

The hook check spawns `scripts/agent-control-hook.mjs` against a throwaway daemon and asserts the
deny/allow exit codes (PreToolUse deny → exit 2, allow → exit 0, fail-open on a dead daemon).

## Connecting an agent

> A ready-to-copy integrator skill lives at [`skills/agent-control/`](skills/agent-control/SKILL.md)
> (MCP workflow + enforcement hooks). Drop it into your agent's skill directory to get guided
> onboarding; it documents *using* a running console, not developing this repo.

Point any MCP-capable harness at the server:

- **stdio**: `cargo run -p agent-control-mcp -- --stdio`
- **HTTP**: `http://127.0.0.1:9870/mcp` (Streamable HTTP, `mcp-session-id` header)

Recommended call order for a harness:

1. `get_applicable_rules` — resolve binding rules + provenance + conflicts before working.
2. `create_session` — freeze a constraint snapshot. Get-or-create per (project, agent, task): an
   active session with the same triple is returned unchanged (`reused: true`).
3. `acknowledge_rules` — confirm you received exactly the snapshot's rules.
4. `record_evidence` — append tool calls / file changes / test results to the hash chain.
5. `verify_constraints` — run the snapshot's `code_checks` against the repo, record achieved tiers.
6. `update_work_state` — record the `/goal` objective + done/doing/todo items so a handoff carries
   resumable progress (not just rules).
7. `export_handoff` / `import_handoff` + `acknowledge_rules` — move between harnesses (re-verify tiers).
8. `verify_evidence` — recompute the chain and detect tampering.
9. `set_session_status` — close out (`completed`/`failed`) when the unit of work ends; reopen with
   `active`. `handed_off` is set only by `export_handoff`.

### MCP tools

`get_applicable_rules`, `explain_rule`, `check_conflicts`, `create_session`, `record_evidence`,
`acknowledge_rules`, `get_session_context`, `update_work_state`, `set_session_status`, `export_handoff`, `import_handoff`,
`verify_evidence`, `list_rules`, `save_rule`, `set_rule_enabled`, `suggest_rule`, `list_suggestions`,
`export_native`, `check_export_drift`, `revoke_native`, `verify_constraints`, `check_tool_call`,
`register_hooks`, `revoke_hooks`, `detect_hooks`.

`save_rule` lets an agent **actively record** a discovered project contract, personal preference, or
machine/project environment note as a rule file: pass `id`, `title`, `content`, `agent_id`, and for
project contracts `scope: "project"` + `project_path` (or `scope: "environment"` + `project_path` for a
git-ignored `local/` environment note). Reusing an `id` updates the rule and bumps its version. The
file lands in the project's `.agent-control/rules/` (or `.agent-control/local/` for environment),
appears in the console, and binds future sessions — use it for durable project knowledge, while
`record_evidence` stays for session actions.

`set_rule_enabled` **pauses or resumes** a rule without changing its content: pass `id`, `enabled`
(`true`/`false`), `scope`, and `project_path` for project scope. A disabled rule is excluded from
resolution (`Decision::Disabled`) and stops binding sessions; enabling restores it. This is the only
tool that flips enablement — a `save_rule` content update preserves the existing state, so editing a
paused rule never silently re-activates it. The console's Rules editor exposes the same toggle as an
**Enabled** switch.

`suggest_rule` is the **human-in-the-loop** alternative: when a rule is inferred or uncertain,
the agent proposes it (with a `rationale`) rather than writing it directly. The draft is stored in
SQLite as a `pending` suggestion and surfaces in the **Suggestions** view. It does not bind sessions
until a user reviews it — the user can edit the draft, **approve** it (writing a binding rule just
like `save_rule`), or **reject** it with a note. `scope: "project"` (default, needs `project_path`)
proposes a project contract; `scope: "environment"` (needs `project_path`) proposes a machine/project
**environment note** written to the git-ignored `.agent-control/local/` — the right home for facts
such as "I run my CLIs inside WSL; Windows-native tools are callable for compile/verify", which are
true only on this machine/project, not portable personal habits; `scope: "user"` proposes a **personal
preference** that follows you across projects. `list_suggestions` lets agents poll the review
queue (`pending` / `approved` / `rejected`) and see the `approved_rule_id` once accepted.

`export_native` **writes a static pointer block into the harness entry points you choose** so agents
that never call MCP still learn where the contracts live. Pass `project_path`, an optional `targets`
list (`agents` | `claude` | `gemini` | `cursor`; omitted → `AGENTS.md` only), and optionally
`include_user`. It writes a *managed block* — `<!-- agent-control:begin --> … <!-- agent-control:end -->`
— into the selected files (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursor/rules/agent-control.mdc`);
with `include_user: true` it also writes a preferences pointer to the user-global `~/.claude/CLAUDE.md`.
The block **points at** the live rules in `.agent-control/rules/` — it does **not** copy their text —
so editing a rule never makes it stale. Everything **outside** the block is preserved (a hand-written
`AGENTS.md` is safe), and re-export is **idempotent** (an up-to-date file is reported `unchanged` and
not rewritten). Targets are opt-in because some harnesses read only their own file: Claude Code reads
`CLAUDE.md` and ignores `AGENTS.md` when both exist. The project guidance is **discovery-first**: it
opens with "understand this project before changing it" and tells the agent to **read and summarize the
enabled rules** in `.agent-control/rules/` (kinds `contract`/`constraint`, priority wins, `applies_to`
narrows) rather than leading with a binding-contract headline. Environment notes are **excluded** from
the committed block (they are git-ignored and reach the agent only via the briefing/hook). A static
**`### Continuity (optional)`** section comes **last**, pointing at
`<project>/.agent-control/handoff/latest.md` and noting it may be refreshed via `export_handoff`. The
console exposes the same action as target checkboxes + **Export to harnesses** in the Projects detail
pane; `revoke_native` (the **Revoke** button) removes the block again — keeping surrounding prose and
deleting files that held only the block.

`save_rule` and `suggest_rule` also accept an **`enforcement`** intent — `advisory` (default),
`verify`, or `hard` — declaring whether a rule is mechanically checkable; `save_rule` additionally
accepts `code_checks`. This is a property of the *rule*, not a guarantee of what a session achieves
(see **Assurance tiers** below).

`check_export_drift` (`project_path`, optional `targets`, optional `include_user`) reports which
entry points are missing the current-format pointer block. Each block embeds a **block-format
version**; a target is **stale** when it exists and that version differs from the current one (e.g. a
legacy v1 full-text block that should be migrated to the pointer). Because the pointer is static, a
rule edit does **not** make it stale. It is read-only: it prompts a re-export and **never rewrites**
anything. The SessionStart hook calls it and appends a migration warning. It detects **file-vs-store
format** drift only — it does **not** tell you whether an already-running session's injected context
is stale; that boundary is per-session and explicit.

`verify_constraints` (`session_id`, `agent_id`) evaluates the frozen snapshot's `code_checks` against
the **real repo files**, computes the **achieved assurance tier** for each rule, and records the
result in the evidence chain. It returns a report with `verified` / `advisory` / `enforced` counts.
This is distinct from `verify_evidence` (which recomputes the hash chain to detect tampering):
`verify_constraints` asks *did the repo actually satisfy the checks?*, not *is the record intact?*.
The console's Sessions pane exposes it as **Verify constraints**. The same pane shows the session's
**work state** (goal + done/doing/todo items) with an inline editor backed by `update_work_state`,
tucks the snapshot hash / provenance id into a collapsed **Verification details** disclosure (the full
chain stays in the Evidence / Provenance views), and offers a **Delete session** action (header button
or row right-click) that cascades to the session's evidence chain + provenance behind a confirm dialog.

`check_tool_call` (`project_path`, `tool_name`, `tool_input`, `agent_id`, optional `session_id`) is the
**deny-capable PreToolUse gate** — the only path that earns the `Enforced` tier. It evaluates the
project's enabled `hard` rules two ways: a **repo gate** (a hard rule's `code_checks` failing or
inconclusive denies tools until the repo complies) and **tool guards** (a rule's `tool_guards` —
tool-name glob + input regex — deny matching calls, e.g. block `rm -rf`). The PreToolUse hook calls it
before each tool call and exits `2` with the reason on stderr to block a denied call. With no `hard`
rules it is fail-open. `register_hooks` / `revoke_hooks` / `detect_hooks` manage the hook wiring in
each harness's `settings.json` (see **Native export & hooks** below); the Projects → Hooks panel does
the same with one click.

## Native export & hooks

Rule files in `.agent-control/rules/` are the source of truth, but most agents read their own
entry points (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursor/rules`) rather than calling MCP. Agent
Control bridges that gap with two delivery mechanisms:

- **Native export (pull-friendly).** `export_native` / the **Export to harnesses** button writes a
  static *pointer* block into the entry points you select (`AGENTS.md` by default; `CLAUDE.md`,
  `GEMINI.md`, Cursor opt-in). The block is **discovery-first**: it tells the agent to understand the
  project and **read and summarize the enabled rules** in `.agent-control/rules/` — it does not
  duplicate their text, so a rule edit never makes it stale, and it never commits the git-ignored
  environment notes. The block is the management boundary: re-export replaces only the block, so
  frontmatter and hand-written prose around it survive, and `revoke_native` / the **Revoke** button
  removes it again. The Cursor target keeps its `---` frontmatter outside the block. This makes the
  contracts discoverable by any agent that reads the file, with no MCP integration required.
- **SessionStart + PreToolUse hooks (push + enforce).**
  [`scripts/agent-control-hook.mjs`](scripts/agent-control-hook.mjs) is a dependency-free Node hook
  for Claude Code / CodeBuddy / WorkBuddy / Qoder. At **SessionStart** it calls `get_applicable_rules`
  over the local MCP HTTP endpoint and injects a concise briefing (binding rules + conflicts + the
  machine/project **work environment**) into the session context via
  `hookSpecificOutput.additionalContext`. At **PreToolUse** it calls
  `check_tool_call` with the pending tool name/input and, on deny, **blocks the call** by exiting `2`
  with the reason on stderr — the chokepoint that turns a `hard` rule into an `Enforced` one. It is
  **fail-open**: an unreachable/slow daemon exits `0` silently, never blocking a session or denying a
  tool by infrastructure. Register both events with `register_hooks` (or the Projects → **Hooks**
  panel), which writes a managed group into each harness's `settings.json`, preserving your other
  config and hooks; `detect_hooks` inspects and `revoke_hooks` undoes it. Point `AGENT_CONTROL_MCP_URL`
  at your endpoint if it is not the default `http://127.0.0.1:9870/mcp`.

### Assurance tiers

**Reach is not verification, and verification is not enforcement.** Agent Control keeps two
orthogonal axes apart so it never overclaims:

- **Declared intent — `Rule.enforcement`** (`advisory` | `verify` | `hard`): a property of the *rule*.
  `advisory` is semantic guidance (not mechanically checkable); `verify` carries `code_checks`
  evaluable against the repo; `hard` claims the rule must be mediated at a chokepoint that can deny.
  Declaring `hard` does **not** by itself enforce anything.
- **Achieved tier — `AssuranceTier`** (`Advisory` < `Verified` < `Enforced`): what a *session* actually
  delivered, computed from evidence and recorded beside the frozen snapshot (the `snapshot_hash` over
  rules stays intact).

| Tier | Meaning | How it is earned |
|------|---------|------------------|
| **Advisory** | Delivered, but no mechanical check ran and nothing intercepted the action. | Export / hook / query. Semantic rules stay here forever. |
| **Verified** | The rule's `code_checks` ran against the real repo and passed; the result is in the chain. | `verify_constraints` (post-hoc). |
| **Enforced** | The action was mediated at a chokepoint that can deny it. | PreToolUse hook → `check_tool_call` (pre-emptive). |

Without a chokepoint a `hard` rule still caps at **Advisory/Verified** — there is no pre-emptive deny.
Export and the SessionStart hook only make rules *reach* the agent; `code_checks` + the hash chain
*verify* after the fact; hard enforcement exists only where the **PreToolUse hook** mediates the action
itself (`check_tool_call` denying the call), and cannot compel an uncooperative agent running outside
that mediation. Register the hook with `register_hooks` to enable it.

**Handoffs carry the baseline, not a guarantee.** `export_handoff` includes the source session's
achieved tiers **and its work state** (the `goal` + done/doing/todo `work_items` recorded via
`update_work_state`); `import_handoff` surfaces them and **warns** that the new harness must re-verify
(warn, not block). The frozen *rules* transfer; the *assurance* behind them must be re-earned in the
new runtime — a tier drop is flagged, never silently assumed. The work state transfers as-is so the
next harness inherits resumable progress instead of relying on its own todo list. `export_handoff` also
writes a readable
continuity brief to `<project>/.agent-control/handoff/latest.md` (task, from-harness, evidence head,
frozen constraints, tiers, plus **Goal** and **Work state** sections) beside the JSON bundle — the
**file-based reach** counterpart the exported
`AGENTS.md` block points at, so a harness that never calls MCP can inherit continuity by reading a
file. Re-running `export_handoff` refreshes `latest.md`; `import_handoff` refreshes it again into the
*resumed* state (naming the new harness and carrying the re-verify warning), so the file-based reach
path never goes stale after a resume.

## Projects & onboarding

Project-level rules are organized **per project**, not by a free-text path. The workflow:

1. Open the **Projects** view and click **Add project**.
2. **Browse** for the repository folder (native directory picker) and give it a display name.
3. Onboarding registers the project in SQLite and scaffolds `<project>/.agent-control/` —
   a `rules/` directory, an `examples/` directory with copy-ready rule templates, and a bilingual
   `README.md` contract guide. Existing files are preserved (the scaffold is idempotent). Examples
   are **not** binding; copy one into `rules/` to activate it.
4. The project becomes the **current project** (shown as a chip in the header). Sessions,
   project-scope rule edits, and conflict resolution all bind to it automatically.

The left **tree** lists every registered project; expanding one reveals its **Contracts**
(project-scope rules), **Sessions**, and **Conflicts**. Clicking a node jumps to the matching
view with that project activated. The right pane shows project metadata (path, created /
last-opened, rule and session counts) and actions: **set current**, **reveal contracts folder**,
**export to harnesses** (with per-harness target checkboxes: AGENTS.md by default, CLAUDE.md /
GEMINI.md / Cursor opt-in), **revoke** (remove the exported block), **rename**, and **remove**.
Removing a project only unregisters it — it never deletes repo files.

**Scope separation.** The same Rules editor is scoped by the sidebar nav: **Preferences**
(个人偏好) edits user-scope rules (stored in `~/.agent-control/rules/user/`, follow you everywhere),
**Project Contracts** (项目契约) edits project-scope rules (stored in the current project's repo, meant
to be committed), and **Work Environment** (工作环境) edits environment-scope notes (stored in the
current project's git-ignored `.agent-control/local/`, injected only via the briefing, always advisory).
With no current project, Project Contracts and Work Environment show guidance and a shortcut to
Projects instead of an editor.

## Suggestions review

Agents propose rules with `suggest_rule`; humans decide. The **Suggestions** view (with a
pending-count badge in the sidebar and a banner on the Dashboard) is the review queue:

1. Filter by **Pending / Approved / Rejected / All** and select a suggestion to see the agent's
   draft, its `rationale`, who proposed it, and a **scope tag** (user / project / environment).
2. **Edit** the rule id, title, body, and add a review note — the human can supplement or correct
   the agent's draft before deciding.
3. **Approve** to write it as a binding rule at its proposed scope — a **project** suggestion lands
   in the current project's `.agent-control/rules/`, a **user** suggestion (portable personal
   preference) lands in `~/.agent-control/rules/user/` and applies across all projects, and an
   **environment** suggestion lands in the current project's git-ignored `.agent-control/local/`
   (injected only via the briefing) — all the same path as `save_rule`; or **Reject** (with a
   confirmation dialog) to discard it without writing any rule.

Suggestions live in SQLite (`rule_suggestions`), so the queue survives restarts and is auditable:
approved entries record the resulting `approved_rule_id`, rejected entries keep the review note.

## Storage layout

Default data root: `~/.agent-control` (override with `--data-dir`).

**Portable mode.** Place a `portable.marker` file next to the executable (or set
`AGENT_CONTROL_PORTABLE=1`) and the whole store moves beside the binary instead of the global
home directory: data root `<exe dir>/data`, database at `<exe dir>/data/db/control.db`, with
rules / `config.toml` / bundles under `<exe dir>/data/`. Nothing reads or writes `~/.agent-control`,
so a portable copy is fully isolated — ideal for testing. Delete the marker to return to the
global store. Release zips ship the marker so they run portable out of the box.

```
~/.agent-control/
├── control.db          # SQLite: sessions, provenance, evidence chain, projects, rule suggestions
├── config.toml         # app preferences (TOML): locale + MCP port (default 9870)
├── agents.yaml         # registered harness commands
├── rules/user/         # user-scope rule files (Markdown + YAML frontmatter)
└── bundles/            # exported handoff bundles
<project>/.agent-control/
├── rules/              # project-scope contract rules (binding)
├── local/              # environment-scope notes (git-ignored via local/.gitignore; briefing-only, never committed)
├── examples/           # copy-ready rule templates (NOT binding; copy into rules/ to activate)
├── handoff/latest.md   # continuity brief written by export_handoff (the AGENTS.md block points here)
├── README.md           # bilingual contract guide (scaffolded on onboarding)
└── .scaffold.json      # template version + per-file sha256, drives edit-safe scaffold migration
```

Rule files are Markdown with YAML frontmatter (`id`, `title`, `kind`, `scope`, `source`,
optional `code_checks`, filters, priority). User- and project-scope rules are meant to be committed
and reviewed; **environment-scope notes in `local/` are the exception** — git-ignored, machine/project
specific, and delivered only through the MCP/hook briefing.

## GUI automation (accessibility tree)

Because the window renders its own title bar and exposes AccessKit nodes, prefer UI Automation
over mouse synthesis (immune to user interference and DPI scaling):

```powershell
scripts/uia-dump.ps1   -Hwnd <hwnd>          # walk the control-view tree
scripts/uia-invoke.ps1 -Hwnd <hwnd> -Name "Settings"   # invoke by name
scripts/uia-shot.ps1   -Hwnd <hwnd> -Out out.png       # capture the window rect
```

## Release packaging

`mise run release` builds optimized binaries (`cargo build --release`) and assembles
`dist/agent-control-v<version>-windows-x64.zip` via `scripts/package-release.mjs`. The zip bundles
the console + MCP executables, both READMEs (English + 简体中文), the brand icon, the hook script and a
`portable.marker`, so an extracted copy runs fully portable (store beside the exe) out of the box. The
Windows executable embeds `crates/app/assets/icon.ico` (multi-size) plus version info through
`crates/app/build.rs` (winresource), so Explorer / the taskbar show the Agent Control icon.

## License

Apache-2.0