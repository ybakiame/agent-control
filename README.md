# Agent Control

**A developer-centric, engineering-protocol-based, local-first command console that connects
different Agents over MCP / ACP.**

以开发者为中心、以工程协议为基础、通过 MCP / ACP 连接不同 Agent 的本地优先指挥台。

It does not merely store what you like. It answers the questions that matter when multiple
agents touch one codebase:

- Why did this agent apply this rule?
- Does the rule come from the **user** (personal preference) or the **project** (contract)?
- Does it **conflict** with the current code or with another rule?
- After handing the task to **another harness**, are the same constraints still binding?

它不只是存储用户喜欢什么，还能说明：这个 Agent 为什么使用了这条规则？规则来自用户还是项目？
有没有与当前代码冲突？任务交给另一个 Harness 后，是否仍然遵守同样的约束？

**Why not just AGENTS.md, Agent Skills, or per-agent memory?** Those are *delivery formats* — an
agent reads them if it chooses. Agent Control is a *resolution layer* over one rule store that
**compiles into** those formats and adds what they lack: it decides **which** rules bind **this**
agent in **this** context (file paths, languages, task tags), records **why** each rule won, was
superseded, filtered out, or disabled (provenance), checks rules against **real repo state** and
against each other (conflict detection), and freezes the binding set into a snapshot that stays
binding **across harnesses**. The resolved contracts reach agents three ways: **exported** into the
project's `AGENTS.md` / `CLAUDE.md` / `.cursor/rules` (a managed block, so hand-written prose is
preserved), **pushed** into a Claude Code session at start via a SessionStart hook, or **queried**
over MCP / injected over ACP. The differentiator is **traceable provenance + conflict detection + a
frozen cross-harness snapshot**, delivered into the tools agents already read.

与 AGENTS.md / Agent Skills / 各家 memory 的区别：那些是**交付格式**——agent 愿意读才读。
Agent Control 是同一规则库之上的**解析层**，会**编译进**这些格式并补足它们缺失的能力：按上下文判定
哪些规则生效、记录每条规则为何胜出 / 被取代 / 被过滤 / 被禁用（溯源）、对照**真实仓库状态**与彼此
检测冲突，并把生效集冻结成跨 harness 仍然绑定的快照。解析后的契约通过三种方式触达 agent：**导出**进
项目的 `AGENTS.md` / `CLAUDE.md` / `.cursor/rules`（托管块，保留手写内容）、在 Claude Code 会话开始时
经 SessionStart hook **主动推送**、或经 MCP **查询** / ACP 注入。卖点是**可追溯溯源 + 冲突检测 +
跨 harness 冻结快照**，并交付进 agent 本就会读取的工具。

---

## The four layers / 四层架构

| # | Layer | 层 | Mechanism |
|---|-------|----|-----------|
| 1 | Personal preferences | 个人偏好 | User-scope rules that follow you across projects |
| 2 | Project contracts | 项目契约 | Project-scope rules that bind every agent working in a repo |
| 3 | Cross-harness continuity | 跨 harness 连续性 | Frozen constraint snapshots + hash-sealed handoff bundles |
| 4 | Verifiable evidence | 可验证证据 | SHA-256 hash chain; tampering and gaps are detectable |

> **What "verifiable" means here.** The evidence chain and handoff-bundle hashes are SHA-256
> *integrity* seals: they prove a record was not altered after it was written, and detect gaps or
> reordering. They are **not cryptographic signatures** — there is no signer identity or key, and on
> a single machine with one writer they cannot prove the reporting agent was truthful. Independent
> verification comes from `code_checks`, which read the *actual* repo files instead of trusting the
> agent's report.

## Highlights

- **Rust-first**, edition 2024, workspace of four crates (`core`, `mcp`, `acp`, `app`).
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
- **Clear user / project separation.** Scope is driven by the **sidebar nav**, not an in-view tab
  toggle: **Preferences** (个人偏好) shows user-scope rules that follow you across projects, and
  **Project Contracts** (项目契约) shows project-scope rules written into the *current* project's repo
  (no free-text path juggling), meant to be committed and reviewed.
- **Agent suggestions, human review.** Agents can propose rules with `suggest_rule` instead of
  writing them directly. Drafts land in a SQLite-backed review queue (the **Suggestions** view,
  with a pending-count badge in the sidebar). A user edits, **approves** (the draft becomes a
  binding project rule) or **rejects** it — keeping humans in the loop for inferred contracts.
- **Bilingual UI (English / 简体中文)** with a runtime switcher in Settings; the choice is
  persisted to `<data root>/ui.json`.
- **MCP server** (rmcp) in both **stdio** and **Streamable HTTP** modes, exposing 16 tools plus
  resources and prompts.
- **ACP client / harness manager** (agent-client-protocol) that launches real agent CLIs,
  injects the constraint briefing as system prompt, wires the MCP endpoint, streams session
  updates, handles permission requests, and records file-change evidence.
- **Hybrid storage**: git-friendly Markdown/YAML rule files + SQLite for sessions, provenance
  and the evidence chain.
- **Accessibility-first automation**: the GUI exposes a full UI Automation (AccessKit) tree, so
  tests drive it by element name / `InvokePattern` instead of synthetic mouse clicks.

## Quick start / 快速开始

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

The desktop app auto-starts the MCP Streamable HTTP endpoint at `http://127.0.0.1:9870/mcp`.

End-to-end protocol checks (Node.js, no extra deps):

```sh
node scripts/mcp-e2e.mjs        # stdio MCP: rules, conflicts, sessions, evidence, handoff
node scripts/mcp-http-e2e.mjs   # Streamable HTTP MCP transport
```

ACP is exercised by `crates/acp/tests/harness.rs` against `scripts/mock-acp-agent.mjs`
(includes a permission round-trip and file-change evidence).

## Connecting an agent / 连接代理

> A ready-to-copy integrator skill lives at [`skills/agent-control/`](skills/agent-control/SKILL.md)
> (MCP workflow + ACP harness setup). Drop it into your agent's skill directory to get guided
> onboarding; it documents *using* a running console, not developing this repo.

Point any MCP-capable harness at the server:

- **stdio**: `cargo run -p agent-control-mcp -- --stdio`
- **HTTP**: `http://127.0.0.1:9870/mcp` (Streamable HTTP, `mcp-session-id` header)

Recommended call order for a harness:

1. `get_applicable_rules` — resolve binding rules + provenance + conflicts before working.
2. `create_session` — freeze a constraint snapshot.
3. `acknowledge_rules` — confirm you received exactly the snapshot's rules.
4. `record_evidence` — append tool calls / file changes / test results to the hash chain.
5. `export_handoff` / `import_handoff` + `acknowledge_rules` — move between harnesses.
6. `verify_evidence` — recompute the chain and detect tampering.

### MCP tools

`get_applicable_rules`, `explain_rule`, `check_conflicts`, `create_session`, `record_evidence`,
`acknowledge_rules`, `get_session_context`, `export_handoff`, `import_handoff`,
`verify_evidence`, `list_rules`, `save_rule`, `set_rule_enabled`, `suggest_rule`, `list_suggestions`,
`export_native`.

`save_rule` lets an agent **actively record** a discovered project contract (or personal
preference) as a rule file: pass `id`, `title`, `content`, `agent_id`, and for project contracts
`scope: "project"` + `project_path`. Reusing an `id` updates the rule and bumps its version. The
file lands in the project's `.agent-control/rules/`, appears in the console, and binds future
sessions — use it for durable project knowledge, while `record_evidence` stays for session actions.

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
proposes a project contract; `scope: "user"` proposes a **personal preference** that follows you
across projects — the right home for environment notes such as "I run my CLIs inside WSL;
Windows-native tools are callable for compile/verify". `list_suggestions` lets agents poll the review
queue (`pending` / `approved` / `rejected`) and see the `approved_rule_id` once accepted.

`export_native` **compiles the binding set into each harness's native entry point** so agents that
never call MCP still inherit the contracts. Pass `project_path` (and optionally `include_user`).
It writes a *managed block* — `<!-- agent-control:begin --> … <!-- agent-control:end -->` — into the
project's `AGENTS.md`, `CLAUDE.md` and `.cursor/rules/agent-control.mdc`; with `include_user: true` it
also writes personal preferences to the user-global `~/.claude/CLAUDE.md`. Only **enabled** rules are
exported, everything **outside** the block is preserved (a hand-written `AGENTS.md` is safe), and
re-export is **idempotent** (an up-to-date file is reported `unchanged` and not rewritten). The
console exposes the same action as **Export to harnesses** in the Projects detail pane.

## Native export & hooks / 原生导出与 hooks

Rule files in `.agent-control/rules/` are the source of truth, but most agents read their own
entry points (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules`) rather than calling MCP. Agent Control
bridges that gap with two delivery mechanisms:

- **Native export (pull-friendly).** `export_native` / the **Export to harnesses** button renders the
  enabled contracts into a managed block inside those files. The block is the management boundary:
  re-export replaces only the block, so frontmatter and hand-written prose around it survive. The
  Cursor target keeps its `---` frontmatter outside the block. This makes the contracts visible to
  any agent that reads the file, with no MCP integration required.
- **SessionStart hook (push).** [`scripts/agent-control-hook.mjs`](scripts/agent-control-hook.mjs) is
  a dependency-free Node hook for Claude Code. At session start it calls `get_applicable_rules` over
  the local MCP HTTP endpoint and injects a concise briefing (binding rules + conflicts) into the
  session context via `hookSpecificOutput.additionalContext`. It is **read-only** and exits `0`
  silently if the daemon is unreachable, so it never blocks a session. Wire it by adding a
  `SessionStart` command hook to `.claude/settings.json` (the script header carries a copy-ready
  snippet); point `AGENT_CONTROL_MCP_URL` at your endpoint if it is not the default
  `http://127.0.0.1:9870/mcp`.

> **Reach is not enforcement.** Export and the hook make rules *reach* the agent; they do not *force*
> compliance — a cooperative agent still has to follow them. Independent verification comes from
> `code_checks` (which read real repo files) and the evidence hash chain (which detects tampering /
> gaps after the fact). Hard enforcement only exists where Agent Control mediates the action itself,
> i.e. the ACP permission round-trip; it cannot compel an uncooperative agent running outside that
> mediation.

规则文件（`.agent-control/rules/`）是唯一事实来源，但多数 agent 读取自己的入口文件
（`AGENTS.md`、`CLAUDE.md`、`.cursor/rules`）而非调用 MCP。Agent Control 用两种交付机制弥合：
**原生导出**（`export_native` / 「导出到 harness」按钮）把启用的契约渲染进这些文件的**托管块**，
块外内容与 Cursor frontmatter 均保留，重复导出幂等；**SessionStart hook**
（`scripts/agent-control-hook.mjs`，无依赖 Node）在 Claude Code 会话开始时经本地 MCP HTTP 调用
`get_applicable_rules`，把绑定规则与冲突摘要注入会话上下文，只读、守护进程不可达时静默退出 0、绝不阻塞会话。
**触达不等于强制**：导出与 hook 让规则*触达* agent，但不*强制*遵守；独立校验来自 `code_checks`
（读取真实仓库文件）与证据哈希链（事后检测篡改 / 缺口），硬性强制仅存在于 Agent Control 亲自中介动作之处
（ACP 权限往返），无法强迫在该中介之外运行的不合作 agent。

## Projects & onboarding / 项目接入

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
**export to harnesses**, **rename**, and **remove**. Removing a project only unregisters it — it
never deletes repo files.

项目级规则以**项目**为单位组织，而非手填路径。在 **Projects** 视图点击 **Add project**，浏览选择
仓库目录并命名；接入时会注册项目并在 `<project>/.agent-control/` 生成 `rules/` 目录、`examples/`
模板目录（可复制的规则示例，不生效，复制进 `rules/` 即启用）与中英双语 `README.md` 契约指南
（幂等，保留已有文件）。该项目随即成为**当前项目**（顶栏芯片显示），会话、
项目级规则编辑、冲突消解都自动绑定到它。左侧**树**展开每个项目的契约 / 会话 / 冲突，点击节点跳转
到对应视图；右侧提供设为当前、打开契约目录、导出到 harness、重命名、移除（仅注销，不删除仓库文件）。

**User vs project scope.** The same Rules editor is scoped by the sidebar nav: **Preferences**
(个人偏好) edits user-scope rules (stored in `~/.agent-control/rules/user/`, follow you everywhere),
and **Project Contracts** (项目契约) edits project-scope rules (stored in the current project's repo).
With no current project, Project Contracts shows guidance and a shortcut to Projects instead of an
editor.

## Suggestions review / 建议审核

Agents propose rules with `suggest_rule`; humans decide. The **Suggestions** view (with a
pending-count badge in the sidebar and a banner on the Dashboard) is the review queue:

1. Filter by **Pending / Approved / Rejected / All** and select a suggestion to see the agent's
   draft, its `rationale`, who proposed it, and a **scope tag** (user / project).
2. **Edit** the rule id, title, body, and add a review note — the human can supplement or correct
   the agent's draft before deciding.
3. **Approve** to write it as a binding rule at its proposed scope — a **project** suggestion lands
   in the current project's `.agent-control/rules/`, a **user** suggestion (personal preference /
   environment note) lands in `~/.agent-control/rules/user/` and applies across all projects (the
   same path as `save_rule`); or **Reject** (with a confirmation dialog) to discard it without
   writing any rule.

Suggestions live in SQLite (`rule_suggestions`), so the queue survives restarts and is auditable:
approved entries record the resulting `approved_rule_id`, rejected entries keep the review note.

代理通过 `suggest_rule` 提议规则，由人审核。**Suggestions** 视图（侧栏带待审计数徽标、仪表盘有入口
横幅）即审核队列：按 **Pending / Approved / Rejected / All** 过滤，选中后可见代理草稿、`rationale`、
提议者与**作用域标签**（user / project）；用户可**编辑**规则 id/标题/正文并补充审核备注，再**批准**
（按提议作用域写入绑定规则，与 `save_rule` 同路径：**project** 写入当前项目 `.agent-control/rules/`，
**user** 写入 `~/.agent-control/rules/user/` 成为跨项目生效的个人偏好）或**拒绝**（确认对话框，不写任何规则）。
`scope: "user"` 适合记录**工作环境说明**等个人偏好（如「我在 WSL 中运行 CLI，编译验证时可调用 Windows
原生工具」）——用户级规则总会加载、冻结进每个会话快照，并通过 ACP `constraint_briefing` /
`get_applicable_rules` 主动注入，无需任务中途再提醒。建议存于 SQLite（`rule_suggestions`），重启后保留
且可审计：批准记录生成的 `approved_rule_id`，拒绝保留审核备注。

## Storage layout / 存储布局

Default data root: `~/.agent-control` (override with `--data-dir`).

**Portable mode / 便携模式.** Place a `portable.marker` file next to the executable (or set
`AGENT_CONTROL_PORTABLE=1`) and the whole store moves beside the binary instead of the global
home directory: data root `<exe dir>/data`, database at `<exe dir>/data/db/control.db`, with
rules / `ui.json` / bundles under `<exe dir>/data/`. Nothing reads or writes `~/.agent-control`,
so a portable copy is fully isolated — ideal for testing. Delete the marker to return to the
global store. Release zips ship the marker so they run portable out of the box.

便携模式：在可执行文件旁放置 `portable.marker`（或设 `AGENT_CONTROL_PORTABLE=1`），全部数据改存
于程序旁 `<exe目录>/data`（数据库在 `data/db/control.db`），不再读写全局 `~/.agent-control`，
便于测试隔离；删除标记即恢复全局存储。Release 压缩包默认附带该标记。

```
~/.agent-control/
├── control.db          # SQLite: sessions, provenance, evidence chain, projects, rule suggestions
├── ui.json             # UI preferences (locale)
├── agents.yaml         # registered harness commands
├── rules/user/         # user-scope rule files (Markdown + YAML frontmatter)
└── bundles/            # exported handoff bundles
<project>/.agent-control/
├── rules/              # project-scope contract rules (binding)
├── examples/           # copy-ready rule templates (NOT binding; copy into rules/ to activate)
└── README.md           # bilingual contract guide (scaffolded on onboarding)
```

Rule files are Markdown with YAML frontmatter (`id`, `title`, `kind`, `scope`, `source`,
optional `code_checks`, filters, priority). They are meant to be committed and reviewed.

## GUI automation (accessibility tree) / 无障碍树驱动

Because the window renders its own title bar and exposes AccessKit nodes, prefer UI Automation
over mouse synthesis (immune to user interference and DPI scaling):

```powershell
scripts/uia-dump.ps1   -Hwnd <hwnd>          # walk the control-view tree
scripts/uia-invoke.ps1 -Hwnd <hwnd> -Name "Settings"   # invoke by name
scripts/uia-shot.ps1   -Hwnd <hwnd> -Out out.png       # capture the window rect
```

## Release packaging / 打包发布

`mise run release` builds optimized binaries (`cargo build --release`) and assembles
`dist/agent-control-v<version>-windows-x64.zip` via `scripts/package-release.mjs`. The zip bundles
the console + MCP executables, the README, the brand icon and a `portable.marker`, so an extracted
copy runs fully portable (store beside the exe) out of the box. The Windows executable embeds
`crates/app/assets/icon.ico` (multi-size) plus version info through `crates/app/build.rs`
(winresource), so Explorer / the taskbar show the Agent Control icon.

打包：`mise run release` 构建优化二进制并生成 `dist/agent-control-v<版本>-windows-x64.zip`
（含控制台与 MCP 可执行文件、README、品牌图标与 `portable.marker`，解压即便携运行）。
Windows 可执行文件通过 `crates/app/build.rs`（winresource）嵌入多尺寸 `icon.ico` 与版本信息，
资源管理器 / 任务栏显示 Agent Control 图标。

## License

Apache-2.0