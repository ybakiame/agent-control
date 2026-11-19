# Agent Control

[English](README.md) | **简体中文**

**以开发者为中心、以工程协议为基础、通过 MCP 连接不同 Agent 并经其原生 hooks 强制契约的本地优先指挥台。**

它不只是存储用户喜欢什么，还能回答多个 agent 同时改动一个代码库时真正要紧的问题：

- 这个 Agent 为什么使用了这条规则？
- 规则来自**用户**（个人偏好）还是**项目**（契约）？
- 它是否与当前代码或另一条规则**冲突**？
- 把任务交给**另一个 harness** 后，是否仍然遵守同样的约束？

**与 AGENTS.md / Agent Skills / 各家 memory 的区别？** 那些是**交付格式**——agent 愿意读才读。
Agent Control 是同一规则库之上的**解析层**，会**编译进**这些格式并补足它们缺失的能力：按上下文
（文件路径、语言、任务标签）判定**哪些**规则绑定**这个** agent，记录每条规则为何胜出 / 被取代 /
被过滤 / 被禁用（溯源），对照**真实仓库状态**与彼此检测冲突，并把生效集冻结成**跨 harness** 仍然
绑定的快照。以 MCP 为解析主干；解析后的契约再经 agent 本就在用的渠道触达——以**指针块对齐**进项目的
`AGENTS.md` / `CLAUDE.md` / `GEMINI.md` / `.cursor/rules`（引导 agent 去读活规则，保留手写内容）、在 harness
会话开始时经 SessionStart hook **主动推送**、或经 MCP **查询**；PreToolUse hook 再对每次工具调用把关，
**强制** hard 规则。当 harness 在上下文压缩后重新进入会话（`SessionStart` 且 `source: "compact"`）时，
同一 hook 会重新注入连续性简报（`.agent-control/handoff/latest.md`）与实时目标/工作态；简报先从磁盘读取，
因此即使守护进程不可达也能恢复连续性。卖点是**可追溯溯源 + 冲突检测 + 跨 harness 冻结快照**，并交付进 agent 本就会读取的工具。

---

## 四层架构

| # | 层 | 机制 |
|---|----|------|
| 1 | 个人偏好 | 跟随你跨项目的用户级规则 |
| 2 | 项目契约 | 绑定仓库内每个 agent 的项目级规则 |
| 3 | 跨 harness 连续性 | 冻结约束快照 + 可恢复工作态（goal/done/doing/todo）+ 哈希封存的交接 bundle |
| 4 | 可验证证据 | SHA-256 哈希链；篡改与缺口可检测 |

> **三种规则作用域，而非两种。** 除**用户**（个人偏好，可跨项目移植）与**项目**（契约，提交并审核）外，
> 还有**环境**作用域（工作环境）：描述*当前用户在当前机器的当前项目*的上下文——例如「agent 运行在 WSL 内；
> 编译验证时可调用 Windows 原生工具」。环境说明**项目本地且被 git 忽略**（`.agent-control/local/`），
> 因此**绝不提交**进 `AGENTS.md`；它们只经 MCP `get_applicable_rules` 简报或 SessionStart hook 触达 agent。
> 它们是非竞争性的（绝不取代契约），且始终为 `advisory`。这让机器特定的事实既不混入你可移植的偏好，
> 也不混入仓库共享的契约。

> **这里「可验证」的含义。** 证据链与交接 bundle 的哈希是 SHA-256 *完整性*封存：它们证明记录写入后未被
> 改动，并检测缺口或乱序。它们**不是密码学签名**——没有签名者身份或密钥，在单写者的单机器上无法证明
> 上报的 agent 说了真话。独立校验来自 `code_checks`：它读取*真实*仓库文件，而非信任 agent 的上报。

## 功能亮点

- **Rust 优先**，edition 2024，三 crate 工作区（`core`、`mcp`、`app`）。
- **原生 GUI**，基于 [`gpui-kit`](https://nsh.dev/docs/)（GPUI），品牌化暗色设计系统（紫色主色、卡片表面、
  分组图标侧栏、状态栏）。自渲染标题栏（无 OS 边框），八个视图：仪表盘、**项目**、规则与契约、**建议**、
  溯源浏览器、可验证证据、会话与连续性、设置。
- **项目注册表 + 引导式接入。** 项目是一等公民：注册一次仓库（浏览选择目录、命名），指挥台即在
  `<project>/.agent-control/` 生成双语契约指南。**树视图**组织每个项目的契约、会话与冲突；顶栏芯片显示
  用户级与项目级动作所绑定的**当前项目**。
- **上下文感知的规则解析。** 引擎按 `applies_to`（agent、文件路径、语言、任务标签）过滤，按
  作用域 → 优先级 → 时间消解竞争，并标记规则间与**规则对代码**的冲突：`code_checks` 独立读取真实配置文件
  （JSON/TOML/YAML/INI/.editorconfig）并比对期望值与实际值。规则可用**启用/禁用**开关暂停，而非删除。
- **清晰的作用域分离。** 作用域由**侧栏导航**驱动，而非视图内的标签切换：**个人偏好**显示跟随你跨项目的
  用户级规则，**项目契约**显示写入*当前*项目仓库的项目级规则（无需手填路径），意在提交并审核，
  **工作环境**显示被 git 忽略、仅经简报注入的机器/项目本地说明。
- **代理提议，人工审核。** agent 可用 `suggest_rule` 提议规则而非直接写入。草稿进入 SQLite 支撑的审核队列
  （**建议**视图，侧栏带待审计数徽标）。用户可编辑、**批准**（草稿按其提议作用域成为绑定规则）或**拒绝**——
  让推断出的契约始终有人在环。
- **会话生命周期。** `create_session` 按 (项目, agent, 任务) get-or-create——重复调用复用活动会话
  （`reused: true`），而非堆积重复项。`set_session_status` 关闭（`completed`/`failed`）或重开（`active`）会话，
  并追加 `session_end` 证据条目，让已完成的工作离开活动列表却仍可审计。会话视图显示可恢复的**工作态**
  （goal + done/doing/todo）、已达保证分级，以及级联删除会话证据 + 溯源的删除动作。
- **双语 UI（English / 简体中文）**，设置内有运行时切换器；选择持久化到 `<数据根>/config.toml`。
- **MCP 服务端**（rmcp），支持 **stdio** 与 **Streamable HTTP** 两种模式，暴露 28 个工具及资源与提示。
  守护进程的回环**端口可在设置中配置**（默认 `9870`）并存于 `config.toml`，因此便携/dev 实例可绑定不同端口、
  与生产隔离；侧栏图标按钮可一眼启停守护进程。
- **Hooks 强制**，支持**九家编码 agent**——Claude Code、CodeBuddy、WorkBuddy、Qoder、Qoder CN、Codex、
  Cursor、Antigravity、Kimi Code：一键 `register_hooks`（或 Projects → Hooks 面板）按各 harness 自己的
  配置 schema（嵌套 JSON、Cursor 扁平 JSON、Antigravity 命名 bundle、Kimi TOML）写入 **start** hook
  （注入绑定规则简报）与 **pre-tool** hook（调用 `check_tool_call` 拒绝违反 `hard` 规则的工具调用）。
  这是 **Enforced** 级别背后的可拒绝卡点；`detect_hooks` / `revoke_hooks` 检测与撤回。七家写入项目级配置；
  Kimi 与 Antigravity 为用户级，但仍按调用的工作目录逐项目网关。
- **混合存储**：git 友好的 Markdown/YAML 规则文件 + SQLite 存会话、溯源与证据链。
- **无障碍优先的自动化**：GUI 暴露完整的 UI Automation（AccessKit）树，测试按元素名 / `InvokePattern`
  驱动，而非合成鼠标点击。

## 快速开始

项目级依赖由 **mise** 管理（Rust 工具链 + cmake）：

```sh
mise install          # 安装 rust 1.99.0 + cmake
mise run build        # cargo build --workspace
mise run check        # cargo clippy --workspace --all-targets -- -D warnings
mise run test         # cargo test --workspace
mise run run          # 启动桌面指挥台
mise run mcp          # stdio 模式 MCP 服务端（供 agent harness 使用）
mise run fmt          # cargo fmt --all
```

桌面应用自动在 `http://127.0.0.1:9870/mcp` 启动 MCP Streamable HTTP 端点（默认端口；在
**设置 → MCP 服务端**中修改以隔离实例——存于 `config.toml`）。

端到端协议检查（Node.js，无额外依赖）：

```sh
mise run e2e                    # 构建 MCP 二进制，然后运行下面全部三项检查
node scripts/mcp-e2e.mjs        # stdio MCP：规则、冲突、会话、证据、交接、网关、hooks
node scripts/mcp-http-e2e.mjs   # Streamable HTTP MCP 传输
node scripts/hook-e2e.mjs       # 针对活守护进程的 SessionStart/PreToolUse hook 行为
```

hook 检查针对一个临时守护进程运行 `scripts/agent-control-hook.mjs`，断言各厂商的拒绝/放行契约
（claude 式退出 2 / Cursor `{permission}` / Antigravity `{decision}`）、守护进程不可达时 fail-open，
以及可选的 fail-closed 模式。

## 连接代理

> 一个可直接复制的接入技能位于 [`skills/agent-control/`](skills/agent-control/SKILL.md)（MCP 工作流 + 强制 hooks）。
> 把它放进你 agent 的技能目录即可获得引导式接入；它讲的是*使用*运行中的指挥台，而非开发本仓库。

把任何支持 MCP 的 harness 指向服务端：

- **stdio**：`cargo run -p agent-control-mcp -- --stdio`
- **HTTP**：`http://127.0.0.1:9870/mcp`（Streamable HTTP，`mcp-session-id` 头）

harness 推荐调用顺序（**MCP 协作路径**，即 agent 自行调用 MCP；从不调用 MCP 的 agent 则经导出 + hooks 触达）：

1. `get_applicable_rules` — 动手前解析绑定规则 + 溯源 + 冲突。
2. `create_session` — 冻结约束快照。按 (项目, agent, 任务) get-or-create：相同三元组的活动会话原样返回
   （`reused: true`）。
3. `acknowledge_rules` — 确认你收到的正是快照中的规则。
4. `record_evidence` — 向哈希链追加工具调用 / 文件变更 / 测试结果。
5. `verify_constraints` — 对照仓库运行快照的 `code_checks`，记录已达级别。
6. `update_work_state` — 记录 `/goal` 目标 + done/doing/todo 项，让交接携带可恢复进度（而不仅是规则）。
7. `export_handoff` / `import_handoff` + `acknowledge_rules` — 在 harness 间转移（重新校验级别）。
8. `verify_evidence` — 重算哈希链并检测篡改。
9. `set_session_status` — 工作单元结束时关闭（`completed`/`failed`）；用 `active` 重开。`handed_off`
   仅由 `export_handoff` 设置。

### MCP 工具

`get_applicable_rules`、`explain_rule`、`check_conflicts`、`create_session`、`record_evidence`、
`acknowledge_rules`、`get_session_context`、`update_work_state`、`set_session_status`、`export_handoff`、`import_handoff`、
`verify_evidence`、`list_rules`、`save_rule`、`set_rule_enabled`、`suggest_rule`、`list_suggestions`、
`export_native`、`check_export_drift`、`revoke_native`、`verify_constraints`、`check_tool_call`、
`register_hooks`、`revoke_hooks`、`detect_hooks`、`doctor`、`search`、`setup`。

`save_rule` 让 agent **主动记录**发现的项目契约、个人偏好或机器/项目环境说明为规则文件：传入 `id`、`title`、
`content`、`agent_id`，项目契约再加 `scope: "project"` + `project_path`（或 `scope: "environment"` + `project_path`
写入被 git 忽略的 `local/` 环境说明）。复用 `id` 会更新规则并提升版本号。文件落在项目的 `.agent-control/rules/`
（环境则在 `.agent-control/local/`），出现在指挥台并绑定未来会话——用它沉淀持久的项目知识，而 `record_evidence`
仍用于会话动作。

`set_rule_enabled` **暂停或恢复**规则而不改其内容：传入 `id`、`enabled`（`true`/`false`）、`scope`，项目作用域再加
`project_path`。被禁用的规则排除出解析（`Decision::Disabled`）、不再绑定会话；启用即恢复。这是唯一翻转启用态的
工具——`save_rule` 的内容更新会保留既有状态，因此编辑已暂停的规则绝不会悄悄重新激活它。指挥台的规则编辑器以
**启用**开关暴露同一能力。

`suggest_rule` 是**人在环**的替代路径：当规则是推断或不确定的，agent 提议它（附 `rationale`）而非直接写入。
草稿作为 `pending` 建议存于 SQLite，并出现在**建议**视图。在用户审核前它不绑定会话——用户可编辑草稿、**批准**
（像 `save_rule` 一样写入绑定规则）或附备注**拒绝**。`scope: "project"`（默认，需 `project_path`）提议项目契约；
`scope: "environment"`（需 `project_path`）提议写入被 git 忽略的 `.agent-control/local/` 的机器/项目**环境说明**——
适合记录诸如「我在 WSL 中运行 CLI；编译验证时可调用 Windows 原生工具」这类只对当前机器/项目成立、并非可移植
个人习惯的事实；`scope: "user"` 提议跟随你跨项目的**个人偏好**。`list_suggestions` 让 agent 轮询审核队列
（`pending` / `approved` / `rejected`），并在被接受后看到 `approved_rule_id`。

`export_native` **向你选择的 harness 入口文件写入静态指针块**，让从不调用 MCP 的 agent 也能知道契约在哪。
传入 `project_path`、可选 `targets` 列表（`agents` | `claude` | `gemini` | `cursor`；省略 → 仅 `AGENTS.md`），
以及可选 `include_user`。它向选中文件（`AGENTS.md`、`CLAUDE.md`、`GEMINI.md`、`.cursor/rules/agent-control.mdc`）
写入*托管块*——`<!-- agent-control:begin --> … <!-- agent-control:end -->`；`include_user: true` 时还向用户全局
`~/.claude/CLAUDE.md` 写入偏好指针。该块**指向** `.agent-control/rules/` 中的活规则——**不复制**其文本——
因此改规则绝不会让它过期。块**外**的一切都被保留（手写 `AGENTS.md` 安全），重复导出**幂等**（已最新的文件
报告 `unchanged` 且不重写）。目标是可选的，因为有些 harness 只读自己的文件：当 `CLAUDE.md` 与 `AGENTS.md`
并存时 Claude Code 只读 `CLAUDE.md`。项目 guidance 以**发现为先**：开头是「改动前先理解这个项目」，并让 agent
**读取并总结** `.agent-control/rules/` 中的**启用**规则（种类 `contract`/`constraint`，优先级胜出，`applies_to`
收窄），而非以绑定契约标题开头。环境说明被**排除**出提交块（它们被 git 忽略，只经简报/hook 触达）。一段静态
**`### Continuity (optional)`** 置于**最后**，指向 `<project>/.agent-control/handoff/latest.md` 并注明可用
`export_handoff` 刷新。指挥台在 Projects 详情面板以目标多选 + **导出到 harness** 暴露同一动作；`revoke_native`
（**撤回**按钮）再次移除托管块——保留周围正文、删除仅含块的文件。

`save_rule` 与 `suggest_rule` 还接受 **`enforcement`** 意图——`advisory`（默认）、`verify` 或 `hard`——声明规则
是否可机械校验；`save_rule` 另接受 `code_checks`。这是*规则*的属性，而非会话所达成结果的保证（见下文**保证分级**）。

`check_export_drift`（`project_path`、可选 `targets`、可选 `include_user`）报告哪些入口缺少当前格式的指针块。
每个块嵌入**块格式版本**；当目标存在且该版本与当前不同（例如应迁移为指针的旧版 v1 全文块）时即为**过期**。
因为指针是静态的，改规则**不会**让它过期。它只读：提示重新导出且**绝不改写**任何内容。SessionStart hook 调用它
并追加迁移警告。它只检测**文件对存储**的格式漂移——**不**告诉你已运行会话注入的上下文是否过期；那个边界是
按会话且显式的。

`verify_constraints`（`session_id`、`agent_id`）对照**真实仓库文件**评估冻结快照的 `code_checks`，为每条规则计算
**已达保证级别**，并把结果记入证据链。它返回带 `verified` / `advisory` / `enforced` 计数的报告。这区别于
`verify_evidence`（重算哈希链以检测篡改）：`verify_constraints` 问的是*仓库是否真的满足了检查？*，而非*记录是否完好？*。
指挥台的会话面板以**校验约束**暴露它。同一面板显示会话的**工作态**（goal + done/doing/todo 项），带由
`update_work_state` 支撑的内联编辑器，把快照哈希 / 溯源 id 收进折叠的**验证详情**区（完整链留在证据 / 溯源视图），
并提供**删除会话**动作（顶栏按钮或行右键），在确认对话框后级联删除会话的证据链 + 溯源。

`check_tool_call`（`project_path`、`tool_name`、`tool_input`、`agent_id`、可选 `session_id`）是**可拒绝的 PreToolUse
网关**——唯一达成 `Enforced` 级别的路径，且**仅在传入 `session_id` 时**按会话记录该级别（见下文**保证分级**）。
它以两种方式评估项目启用的 `hard` 规则：**仓库网关**（hard 规则的 `code_checks` 失败或不确定时拒绝工具，直到仓库
合规）与**工具守卫**（规则的 `tool_guards`——工具名 glob + 输入正则——拒绝匹配的调用，例如阻断 `rm -rf`）。
pre-tool hook 在每次工具调用前调用它，拒绝时以厂商的拒绝形态阻断该调用（claude 式厂商为退出码 2 + stderr）。
hook 会解析自己的 Agent Control 会话（get-or-create，task `hook`）并传入该 `session_id`，因此即便没有显式 MCP 会话，
被中介的 `hard` 规则也能达成 **Enforced** 并把裁决写入证据链。无 `hard` 规则时 fail-open。`register_hooks` /
`revoke_hooks` / `detect_hooks` 管理各 harness 自己配置中的 hook 接线（见下文**原生导出与 hooks**）；
Projects → Hooks 面板一键完成同样的事。

## 原生导出与 hooks

规则文件（`.agent-control/rules/`）是唯一事实来源，但多数 agent 读取自己的入口文件
（`AGENTS.md`、`CLAUDE.md`、`GEMINI.md`、`.cursor/rules`）而非调用 MCP。Agent Control 用两种交付机制弥合：

- **原生导出（便于拉取）。** `export_native` / **导出到 harness** 按钮向你**勾选**的入口文件写入静态*指针*块
  （默认仅 `AGENTS.md`；`CLAUDE.md`、`GEMINI.md`、Cursor 可选）。该块以**发现为先**：引导 agent 先理解项目、
  **读取并总结** `.agent-control/rules/` 中的**启用**规则——它不复制规则文本，因此改规则绝不会让它过期，也绝不
  提交被 git 忽略的环境说明。块是管理边界：重复导出只替换块，故其周围的 frontmatter 与手写正文得以保留，
  `revoke_native` / **撤回**按钮可再次移除它。Cursor 目标把 `---` frontmatter 保留在块外。这让任何读该文件的
  agent 都能发现契约，无需 MCP 集成。
- **start + pre-tool hook（推送 + 强制）。**
  [`scripts/agent-control-hook.mjs`](scripts/agent-control-hook.mjs) 是一个服务全部九家 agent 的无依赖 Node hook；
  注册命令把厂商 id 与事件名 baked 进 argv，脚本据此选择正确的 stdin 归一与输出形态。厂商分三种 I/O 形态：
  **claude 式**（Claude Code、CodeBuddy、WorkBuddy、Qoder、Qoder CN、Codex、Kimi——退出 2 + stderr 拒绝，简报经
  `hookSpecificOutput.additionalContext`）、**Cursor**（合法 JSON 的 `{permission:"allow"|"deny"}` 裁决，简报经
  `{additional_context}`）、**Antigravity**（`{decision:"ask"|"deny"}`——沉默被视为拒绝，故放行必须输出 `ask`；
  stdin 把调用嵌套在 `toolCall.{name,args}` 下）。在 **start** 事件它冻结约束快照（get-or-create 会话），并在厂商
  支持注入时把简明简报（绑定规则 + 冲突 + 机器/项目**工作环境**）推入上下文。在 **pre-tool** 事件它解析同一会话，
  带待执行的工具名/输入**及该 `session_id`** 调用 `check_tool_call`，因此拒绝时以厂商形态阻断调用，被中介的 `hard`
  规则达成被记录的 **Enforced** 级别（见**保证分级**）。它默认 **fail-open**：守护进程不可达/缓慢时输出厂商的放行形态，
  绝不因基础设施阻塞会话或拒绝工具。设置 `AGENT_CONTROL_HOOK_FAIL_CLOSED` 可启用 **fail-closed** 强制——无法取得裁决的
  *pre-tool* 调用将拒绝（start 事件始终 fail-open，绝不阻塞会话初始化）。用 `register_hooks`（或 Projects → **Hooks**
  面板）注册这两个事件，它向各 harness 自己的配置写入托管组，保留你的其余配置与 hook；`detect_hooks` 检测、
  `revoke_hooks` 撤回。若端点不是默认 `http://127.0.0.1:9870/mcp`，把 `AGENT_CONTROL_MCP_URL` 指向你的端点。

**harness 覆盖范围。** `register_hooks` / `revoke_hooks` / `detect_hooks` 接线**九家**编码 agent，各自经自己的
配置 schema（一个 `ConfigCodec`）、事件名与拒绝形态：

| Agent | 配置文件 | Codec | 作用域 | start / pre-tool 事件 | 拒绝形态 |
|-------|----------|-------|--------|------------------------|----------|
| Claude Code | `.claude/settings.json` | 嵌套 JSON | 项目 | `SessionStart` / `PreToolUse` | 退出 2 + stderr |
| CodeBuddy | `.codebuddy/settings.json` | 嵌套 JSON | 项目 | `SessionStart` / `PreToolUse` | 退出 2 + stderr |
| WorkBuddy | `.workbuddy/settings.json` | 嵌套 JSON | 项目 | `SessionStart` / `PreToolUse` | 退出 2 + stderr |
| Qoder | `.qoder/settings.json` | 嵌套 JSON | 项目 | `SessionStart` / `PreToolUse` | 退出 2 + stderr |
| Qoder CN | `.qoder/settings.json`（与 Qoder 共用） | 嵌套 JSON | 项目 | `SessionStart` / `PreToolUse` | 退出 2 + stderr |
| Codex | `.codex/hooks.json` | 嵌套 JSON | 项目 | `SessionStart` / `PreToolUse` | 退出 2 + stderr |
| Cursor | `.cursor/hooks.json` | 扁平 JSON（`version:1`） | 项目 | `sessionStart` / `preToolUse` | `{permission:"deny",…}` |
| Antigravity | `~/.gemini/config/hooks.json` | 命名 bundle | 用户 | `PreInvocation` / `PreToolUse` | `{decision:"deny",reason}` |
| Kimi Code | `~/.kimi-code/config.toml` | TOML 标记块 | 用户 | `SessionStart` / `PreToolUse` | 退出 2 + stderr |

七家把 hook 写入**项目级**配置（共享、可提交）。**Kimi** 与 **Antigravity** 为**用户级**（用户全局文件），但网关仍按
调用的工作目录解析规则，故用户级 hook 只对匹配的项目强制。Antigravity 取代了原先的 Gemini CLI 目标。每次写入都保留
配置的其余部分与用户自写的 hook，是原子写入，并可经 `revoke_hooks` 撤回；Kimi 的 TOML codec 只编辑自己的 `[[hooks]]`
标记块，故用户注释得以保留。`detect_hooks` 报告全部九家并带 `scope` 字段（`project` | `user`）。不具备可拒绝 hook 的
agent 仍经 `export_native`（`AGENTS.md` / `CLAUDE.md` / `GEMINI.md` / `.cursor/rules`）加 MCP 协作路径触达。

### 保证分级

**触达 ≠ 校验，校验 ≠ 强制。** Agent Control 所做的大部分是*触达*：导出与 SessionStart 简报让规则对 agent 可见，
但可见的规则只是 **Advisory**，直到有机械校验或卡点中介它。Agent Control 把两条正交轴彼此分开，绝不夸大：

- **声明意图——`Rule.enforcement`**（`advisory` | `verify` | `hard`）：*规则*的属性。`advisory` 为语义指引
  （不可机械校验）；`verify` 携带可对照仓库评估的 `code_checks`；`hard` 声称须在可拒绝的卡点中介。声明 `hard`
  本身**并不**强制任何事。
- **已达级别——`AssuranceTier`**（`Advisory` < `Verified` < `Enforced`）：*会话*实际交付的结果，由证据计算，
  记录在冻结快照旁（规则上的 `snapshot_hash` 不变）。

| 级别 | 含义 | 如何赢得 |
|------|------|----------|
| **Advisory** | 已触达，但无机械校验运行、无物拦截动作。 | 导出 / SessionStart 简报 / 查询。语义规则永远停在此级。 |
| **Verified** | 规则的 `code_checks` 已对照真实仓库运行并通过；结果入链。 | `verify_constraints`（事后）。 |
| **Enforced** | 动作在可拒绝的卡点被中介，且裁决已按会话记录。 | 带 `session_id` 的 `check_tool_call`（事前拒绝）。 |

**强制是行为；`Enforced` *级别*是按会话的记录。** pre-tool 网关（`check_tool_call`）是唯一达成 `Enforced` 的路径，
且**仅在传入 `session_id` 时**写入该级别（外加一条 `ToolGate` 证据）。随附 hook 会解析自己的 Agent Control 会话——
`create_session` 是按（项目、agent、task `hook`）的 get-or-create，故同一项目+agent 的每次 hook 调用都落在同一会话
UUID 上——并把该 id 传给 `check_tool_call`。因此仅部署 hook 时既*强制了动作*又把 `Enforced` *记录*进该会话的证据链；
harness 经 stdin 传入的 session id 从不被使用（它并非 Agent Control 的 UUID）。MCP 协作路径则记录进 agent 的显式会话。
没有注册 hook 中介调用时，`hard` 规则最高只到 **Advisory/Verified**，且无法强迫运行在卡点之外的不合作 agent。
用 `register_hooks` 注册 hook 以启用行为强制。

**交接携带基线，而非保证。** `export_handoff` 带上源会话的已达级别**与其工作态**（经 `update_work_state` 记录的
`goal` + done/doing/todo `work_items`）；`import_handoff` 展示它们并**警告**新 harness 须重新校验（警告而非阻断）。
冻结的*规则*可转移；其背后的*保证*须在新运行时重新赢得——级别下降会被标记，绝不被悄悄假定。工作态原样转移，
让下一个 harness 继承可恢复进度，而非依赖它自己的 todo 列。`export_handoff` 还在 JSON bundle 旁写出可读的连续性
简报 `<project>/.agent-control/handoff/latest.md`（任务、来源 harness、证据链头、冻结约束、级别，外加 **Goal** 与
**Work state** 两节）——这是导出 `AGENTS.md` 块所指向的**文件级触达**对应物，从不调用 MCP 的 harness 也能靠读文件
继承连续性。再次 `export_handoff` 会刷新 `latest.md`；`import_handoff` 再次把它刷新为*已恢复*状态（命名新 harness
并携带重新校验警告），因此文件级触达路径在恢复后绝不过期。每次导出还会把此前所有*未确认*的交接记录标记为
**已取代**（`superseded`）——只有最新一条交接仍是待确认事项；已确认的记录绝不会被取代。

## 项目接入

项目级规则以**项目**为单位组织，而非手填路径。流程：

1. 打开 **Projects** 视图，点击 **Add project**。
2. **浏览**选择仓库目录（原生目录选择器）并给它一个显示名。
3. 接入时在 SQLite 注册项目，并在 `<project>/.agent-control/` 生成脚手架——`rules/` 目录、含可复制规则模板的
   `examples/` 目录，以及双语 `README.md` 契约指南。已有文件被保留（脚手架幂等）。示例**不**生效；复制一个进
   `rules/` 即激活。
4. 该项目成为**当前项目**（顶栏芯片显示）。会话、项目级规则编辑、冲突消解都自动绑定到它。

左侧**树**列出每个注册项目；展开一个可见其**契约**（项目级规则）、**会话**与**冲突**。点击节点跳转到对应视图
并激活该项目。右侧面板显示项目元数据（路径、创建 / 最近打开、规则与会话计数）与动作：**设为当前**、
**打开契约目录**、**导出到 harness**（带各 harness 目标多选：默认 AGENTS.md，CLAUDE.md / GEMINI.md / Cursor 可选）、
**撤回**（移除导出块）、**重命名**、**移除**。移除项目仅注销它——绝不删除仓库文件。

**作用域分离。** 同一个规则编辑器由侧栏导航定作用域：**个人偏好**编辑用户级规则（存于 `~/.agent-control/rules/user/`，
跟随你到各处），**项目契约**编辑项目级规则（存于当前项目仓库，意在提交），**工作环境**编辑环境级说明（存于当前
项目被 git 忽略的 `.agent-control/local/`，仅经简报注入，始终 advisory）。无当前项目时，项目契约与工作环境显示
指引与前往 Projects 的快捷入口，而非编辑器。

## 建议审核

代理通过 `suggest_rule` 提议规则，由人审核。**Suggestions** 视图（侧栏带待审计数徽标、仪表盘有入口横幅）即审核队列：

1. 按 **Pending / Approved / Rejected / All** 过滤，选中一条建议即可见代理草稿、其 `rationale`、提议者，
   以及**作用域标签**（user / project / environment）。
2. **编辑**规则 id、标题、正文并补充审核备注——人可在决定前补充或纠正代理草稿。
3. **批准**即按其提议作用域写入绑定规则——**project** 建议落在当前项目 `.agent-control/rules/`，**user** 建议
   （可移植个人偏好）落在 `~/.agent-control/rules/user/` 并跨所有项目生效，**environment** 建议落在当前项目被
   git 忽略的 `.agent-control/local/`（仅经简报注入）——都与 `save_rule` 同路径；或**拒绝**（带确认对话框）
   丢弃它而不写任何规则。

建议存于 SQLite（`rule_suggestions`），故队列重启后保留且可审计：批准条目记录生成的 `approved_rule_id`，
拒绝条目保留审核备注。

## 存储布局

默认数据根：`~/.agent-control`（用 `--data-dir` 覆盖）。

**便携模式。** 在可执行文件旁放置 `portable.marker`（或设 `AGENT_CONTROL_PORTABLE=1`），全部数据改存于程序旁
而非全局家目录：数据根 `<exe目录>/data`，数据库在 `<exe目录>/data/db/control.db`，规则 / `config.toml` / bundles
都在 `<exe目录>/data/` 下。不再读写 `~/.agent-control`，因此便携副本完全隔离——适合测试。删除标记即恢复全局存储。
Release 压缩包默认附带该标记，解压即便携运行。

```
~/.agent-control/
├── control.db          # SQLite：会话、溯源、证据链、项目、规则建议
├── config.toml         # 应用偏好（TOML）：locale + MCP 端口（默认 9870）
├── agents.yaml         # 注册的 harness 命令
├── rules/user/         # 用户级规则文件（Markdown + YAML frontmatter）
└── bundles/            # 导出的交接 bundle
<project>/.agent-control/
├── rules/              # 项目级契约规则（绑定）
├── local/              # 环境级说明（经 local/.gitignore 被 git 忽略；仅简报，绝不提交）
├── examples/           # 可复制规则模板（不生效；复制进 rules/ 即激活）
├── handoff/latest.md   # export_handoff 写出的连续性简报（AGENTS.md 块指向这里）
├── README.md           # 双语契约指南（接入时生成）
└── .scaffold.json      # 模板版本 + 各文件 sha256，驱动编辑安全的脚手架迁移
```

规则文件是带 YAML frontmatter 的 Markdown（`id`、`title`、`kind`、`scope`、`source`、可选 `code_checks`、过滤器、
优先级）。用户级与项目级规则意在提交并审核；**`local/` 中的环境级说明是例外**——被 git 忽略、机器/项目特定，
只经 MCP/hook 简报交付。

## 无障碍树驱动

因为窗口自渲染标题栏并暴露 AccessKit 节点，优先用 UI Automation 而非鼠标合成（免受用户操作与 DPI 缩放影响）：

```powershell
scripts/uia-dump.ps1   -Hwnd <hwnd>          # 遍历控件视图树
scripts/uia-invoke.ps1 -Hwnd <hwnd> -Name "Settings"   # 按名调用
scripts/uia-shot.ps1   -Hwnd <hwnd> -Out out.png       # 截取窗口矩形
```

## 打包发布

`mise run release` 构建优化二进制（`cargo build --release`）并经 `scripts/package-release.mjs` 组装
`dist/agent-control-v<版本>-windows-x64.zip`。压缩包内含控制台与 MCP 可执行文件、两个 README（English + 简体中文）、
品牌图标、hook 脚本与 `portable.marker`，解压即便携运行（存储位于 exe 旁）。Windows 可执行文件通过
`crates/app/build.rs`（winresource）嵌入多尺寸 `crates/app/assets/icon.ico` 与版本信息，资源管理器 / 任务栏显示
Agent Control 图标。

## 许可证

Apache-2.0