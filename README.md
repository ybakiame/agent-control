# Agent Control

**A developer-centric, engineering-protocol-based, local-first command console that connects
different Agents over MCP and enforces contracts through their native hooks.**

以开发者为中心、以工程协议为基础、通过 MCP 连接不同 Agent 并经其原生 hooks 强制契约的本地优先指挥台。

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
binding **across harnesses**. The resolved contracts reach agents three ways: **exported** as a
pointer block into the project's `AGENTS.md` / `CLAUDE.md` / `GEMINI.md` / `.cursor/rules` (directing
agents to the live rules, so hand-written prose is preserved), **pushed** into a harness session at
start via a SessionStart hook, or **queried** over MCP. A PreToolUse hook then **enforces** the hard
rules by gating each tool call. The differentiator is **traceable provenance + conflict detection + a
frozen cross-harness snapshot**, delivered into the tools agents already read.

与 AGENTS.md / Agent Skills / 各家 memory 的区别：那些是**交付格式**——agent 愿意读才读。
Agent Control 是同一规则库之上的**解析层**，会**编译进**这些格式并补足它们缺失的能力：按上下文判定
哪些规则生效、记录每条规则为何胜出 / 被取代 / 被过滤 / 被禁用（溯源）、对照**真实仓库状态**与彼此
检测冲突，并把生效集冻结成跨 harness 仍然绑定的快照。解析后的契约通过三种方式触达 agent：以**指针块导出**进
项目的 `AGENTS.md` / `CLAUDE.md` / `GEMINI.md` / `.cursor/rules`（引导 agent 去读活规则，保留手写内容）、在 harness 会话开始时
经 SessionStart hook **主动推送**、或经 MCP **查询**；PreToolUse hook 再对每次工具调用把关，**强制** hard 规则。卖点是**可追溯溯源 + 冲突检测 +
跨 harness 冻结快照**，并交付进 agent 本就会读取的工具。

---

## The four layers / 四层架构

| # | Layer | 层 | Mechanism |
|---|-------|----|-----------|
| 1 | Personal preferences | 个人偏好 | User-scope rules that follow you across projects |
| 2 | Project contracts | 项目契约 | Project-scope rules that bind every agent working in a repo |
| 3 | Cross-harness continuity | 跨 harness 连续性 | Frozen constraint snapshots + resumable work state (goal/done/doing/todo) + hash-sealed handoff bundles |
| 4 | Verifiable evidence | 可验证证据 | SHA-256 hash chain; tampering and gaps are detectable |

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
  binding project rule) or **rejects** it — keeping humans in the loop for inferred contracts.
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

## Connecting an agent / 连接代理

> A ready-to-copy integrator skill lives at [`skills/agent-control/`](skills/agent-control/SKILL.md)
> (MCP workflow + enforcement hooks). Drop it into your agent's skill directory to get guided
> onboarding; it documents *using* a running console, not developing this repo.

Point any MCP-capable harness at the server:

- **stdio**: `cargo run -p agent-control-mcp -- --stdio`
- **HTTP**: `http://127.0.0.1:9870/mcp` (Streamable HTTP, `mcp-session-id` header)

Recommended call order for a harness:

1. `get_applicable_rules` — resolve binding rules + provenance + conflicts before working.
2. `create_session` — freeze a constraint snapshot.
3. `acknowledge_rules` — confirm you received exactly the snapshot's rules.
4. `record_evidence` — append tool calls / file changes / test results to the hash chain.
5. `verify_constraints` — run the snapshot's `code_checks` against the repo, record achieved tiers.
6. `update_work_state` — record the `/goal` objective + done/doing/todo items so a handoff carries
   resumable progress (not just rules).
7. `export_handoff` / `import_handoff` + `acknowledge_rules` — move between harnesses (re-verify tiers).
8. `verify_evidence` — recompute the chain and detect tampering.

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

## Native export & hooks / 原生导出与 hooks

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

### Assurance tiers / 保证分级

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

规则文件（`.agent-control/rules/`）是唯一事实来源，但多数 agent 读取自己的入口文件
（`AGENTS.md`、`CLAUDE.md`、`GEMINI.md`、`.cursor/rules`）而非调用 MCP。Agent Control 用两种交付机制弥合：
**原生导出**（`export_native` / 「导出到 harness」按钮）向你**勾选**的入口文件写入一段静态**指针托管块**
（默认仅 `AGENTS.md`，`CLAUDE.md`/`GEMINI.md`/Cursor 可选）。该块以**发现为先**：引导 agent 先理解项目、
**读取并总结** `.agent-control/rules/` 中的**启用**规则，而**不复制规则文本**，也**绝不提交**被 git 忽略的环境说明；
块外内容与 Cursor frontmatter 均保留，重复导出幂等，改规则不会让块过期。一段静态 **`### Continuity (optional)`**
置于**最后**，指向 `<project>/.agent-control/handoff/latest.md`（并可用 `export_handoff` 刷新它）。
`revoke_native` / 「撤回」按钮可移除托管块（保留手写正文、删除仅含块的文件）。
**SessionStart + PreToolUse hook**（`scripts/agent-control-hook.mjs`，无依赖 Node，支持 Claude Code / CodeBuddy / WorkBuddy / Qoder）
在会话开始时经本地 MCP HTTP 调用 `get_applicable_rules`，把绑定规则、冲突与本机/项目**工作环境**摘要注入会话上下文；在每次工具调用前调用
`check_tool_call` 把关，拒绝时以退出码 2 + stderr 原因**阻断**该调用——这是把 `hard` 规则提升到 **Enforced** 的卡点。
只读、守护进程不可达时静默退出 0、绝不因基础设施阻塞会话或拒绝工具。用 `register_hooks`（或 Projects → **Hooks** 面板）
一键写入各 harness 的 `settings.json`（保留其余配置与用户 hook），`detect_hooks` 检测、`revoke_hooks` 撤回。

**触达 ≠ 校验 ≠ 强制。** 两条正交轴彼此分开，绝不夸大：
- **声明意图 `Rule.enforcement`**（`advisory` | `verify` | `hard`）是*规则*的属性：`advisory` 为语义指引
  （不可机械校验）、`verify` 携带可对照仓库评估的 `code_checks`、`hard` 声称须在可拒绝的卡点中介。
  声明 `hard` 本身并不等于强制。
- **已达级别 `AssuranceTier`**（`Advisory` < `Verified` < `Enforced`）是*会话*实际交付的结果，由证据计算，
  记录在冻结快照旁（规则上的 `snapshot_hash` 不变）。**Advisory**＝已触达但无机械校验、无拦截
  （导出 / hook / 查询；语义规则永远停在此级）；**Verified**＝`code_checks` 已对照真实仓库运行并通过、
  结果入链（`verify_constraints`，事后）；**Enforced**＝动作在可拒绝的卡点被中介（PreToolUse hook → `check_tool_call`，事前）。
  没有卡点时 `hard` 规则最高只到 **Advisory/Verified**（用 `register_hooks` 注册 hook 以启用强制）。`check_export_drift` 只检测**文件对存储**的格式漂移
  （入口缺少当前格式指针块，或仍是旧版全文块），不判断已运行会话的上下文是否过期；指针是静态的，改规则不会触发漂移；只提示重新导出、绝不改写。
  **交接携带基线而非保证**：`export_handoff` 带上源会话已达级别**与工作态**（经 `update_work_state` 记录的
  `goal` 目标 + done/doing/todo `work_items`），`import_handoff` 展示并**警告**新 harness
  须重新校验（警告而非阻断）——冻结的*规则*可转移，其背后的*保证*须在新运行时重新赢得，级别下降会被标记；
  工作态则原样转移，让下一个 harness 继承可恢复的进度，而不依赖它自己的 todo 列。
  `export_handoff` 还会在 JSON bundle 旁写出可读的连续性简报 `<project>/.agent-control/handoff/latest.md`
  （任务、来源 harness、证据链头、冻结约束、级别，外加 **Goal** 与 **Work state** 两节）——这是导出 `AGENTS.md` 块所指向的**文件级触达**对应物，
  从不调用 MCP 的 harness 也能靠读文件继承连续性；再次 `export_handoff` 会刷新 `latest.md`。

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
**export to harnesses** (with per-harness target checkboxes: AGENTS.md by default, CLAUDE.md /
GEMINI.md / Cursor opt-in), **revoke** (remove the exported block), **rename**, and **remove**.
Removing a project only unregisters it — it never deletes repo files.

项目级规则以**项目**为单位组织，而非手填路径。在 **Projects** 视图点击 **Add project**，浏览选择
仓库目录并命名；接入时会注册项目并在 `<project>/.agent-control/` 生成 `rules/` 目录、`examples/`
模板目录（可复制的规则示例，不生效，复制进 `rules/` 即启用）与中英双语 `README.md` 契约指南
（幂等，保留已有文件）。该项目随即成为**当前项目**（顶栏芯片显示），会话、
项目级规则编辑、冲突消解都自动绑定到它。左侧**树**展开每个项目的契约 / 会话 / 冲突，点击节点跳转
到对应视图；右侧提供设为当前、打开契约目录、导出到 harness、重命名、移除（仅注销，不删除仓库文件）。

**Scope separation.** The same Rules editor is scoped by the sidebar nav: **Preferences**
(个人偏好) edits user-scope rules (stored in `~/.agent-control/rules/user/`, follow you everywhere),
**Project Contracts** (项目契约) edits project-scope rules (stored in the current project's repo, meant
to be committed), and **Work Environment** (工作环境) edits environment-scope notes (stored in the
current project's git-ignored `.agent-control/local/`, injected only via the briefing, always advisory).
With no current project, Project Contracts and Work Environment show guidance and a shortcut to
Projects instead of an editor.

## Suggestions review / 建议审核

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

代理通过 `suggest_rule` 提议规则，由人审核。**Suggestions** 视图（侧栏带待审计数徽标、仪表盘有入口
横幅）即审核队列：按 **Pending / Approved / Rejected / All** 过滤，选中后可见代理草稿、`rationale`、
提议者与**作用域标签**（user / project / environment）；用户可**编辑**规则 id/标题/正文并补充审核备注，再**批准**
（按提议作用域写入绑定规则，与 `save_rule` 同路径：**project** 写入当前项目 `.agent-control/rules/`，
**user** 写入 `~/.agent-control/rules/user/` 成为跨项目生效的个人偏好，**environment** 写入当前项目被 git 忽略的
`.agent-control/local/`、仅经简报注入）或**拒绝**（确认对话框，不写任何规则）。
`scope: "environment"` 适合记录**本机/本项目的工作环境说明**（如「我在 WSL 中运行 CLI，编译验证时可调用 Windows
原生工具」）——这类事实只对当前电脑/项目成立，既不该混入可移植的个人偏好，也不该提交进仓库共享契约；
环境规则总会加载、冻结进每个会话快照，并通过 SessionStart hook 简报 / `get_applicable_rules` 主动注入，
无需任务中途再提醒，但**绝不写入导出的 `AGENTS.md`**。建议存于 SQLite（`rule_suggestions`），重启后保留
且可审计：批准记录生成的 `approved_rule_id`，拒绝保留审核备注。

## Storage layout / 存储布局

Default data root: `~/.agent-control` (override with `--data-dir`).

**Portable mode / 便携模式.** Place a `portable.marker` file next to the executable (or set
`AGENT_CONTROL_PORTABLE=1`) and the whole store moves beside the binary instead of the global
home directory: data root `<exe dir>/data`, database at `<exe dir>/data/db/control.db`, with
rules / `config.toml` / bundles under `<exe dir>/data/`. Nothing reads or writes `~/.agent-control`,
so a portable copy is fully isolated — ideal for testing. Delete the marker to return to the
global store. Release zips ship the marker so they run portable out of the box.

便携模式：在可执行文件旁放置 `portable.marker`（或设 `AGENT_CONTROL_PORTABLE=1`），全部数据改存
于程序旁 `<exe目录>/data`（数据库在 `data/db/control.db`），不再读写全局 `~/.agent-control`，
便于测试隔离；删除标记即恢复全局存储。Release 压缩包默认附带该标记。

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