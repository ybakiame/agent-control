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

---

## The four layers / 四层架构

| # | Layer | 层 | Mechanism |
|---|-------|----|-----------|
| 1 | Personal preferences | 个人偏好 | User-scope rules that follow you across projects |
| 2 | Project contracts | 项目契约 | Project-scope rules that bind every agent working in a repo |
| 3 | Cross-harness continuity | 跨 harness 连续性 | Frozen constraint snapshots + signed handoff bundles |
| 4 | Verifiable evidence | 可验证证据 | SHA-256 hash chain; tampering and gaps are detectable |

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
- **Clear user / project separation.** The Rules view has **Personal** and **Project** tabs.
  Personal rules follow you across projects; project contracts are written into the *current*
  project's repo (no free-text path juggling) and are meant to be committed and reviewed.
- **Agent suggestions, human review.** Agents can propose rules with `suggest_rule` instead of
  writing them directly. Drafts land in a SQLite-backed review queue (the **Suggestions** view,
  with a pending-count badge in the sidebar). A user edits, **approves** (the draft becomes a
  binding project rule) or **rejects** it — keeping humans in the loop for inferred contracts.
- **Bilingual UI (English / 简体中文)** with a runtime switcher in Settings; the choice is
  persisted to `<data root>/ui.json`.
- **MCP server** (rmcp) in both **stdio** and **Streamable HTTP** modes, exposing 15 tools plus
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
`verify_evidence`, `list_rules`, `save_rule`, `set_rule_enabled`, `suggest_rule`, `list_suggestions`.

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
**rename**, and **remove**. Removing a project only unregisters it — it never deletes repo files.

项目级规则以**项目**为单位组织，而非手填路径。在 **Projects** 视图点击 **Add project**，浏览选择
仓库目录并命名；接入时会注册项目并在 `<project>/.agent-control/` 生成 `rules/` 目录、`examples/`
模板目录（可复制的规则示例，不生效，复制进 `rules/` 即启用）与中英双语 `README.md` 契约指南
（幂等，保留已有文件）。该项目随即成为**当前项目**（顶栏芯片显示），会话、
项目级规则编辑、冲突消解都自动绑定到它。左侧**树**展开每个项目的契约 / 会话 / 冲突，点击节点跳转
到对应视图；右侧提供设为当前、打开契约目录、重命名、移除（仅注销，不删除仓库文件）。

**User vs project scope.** The Rules view splits into **Personal** (user-scope, stored in
`~/.agent-control/rules/user/`, follows you everywhere) and **Project** (project-scope, stored in
the current project's repo). With no current project, the Project tab shows guidance and a
shortcut to Projects instead of an editor.

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