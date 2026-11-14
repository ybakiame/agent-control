# Project contracts / 项目契约

This directory (`.agent-control/`) holds this project's engineering contracts.
本目录（`.agent-control/`）保存本项目的工程契约。

- `rules/` — project-scope rule files (Markdown + YAML frontmatter). Every agent working in this
  repo inherits them; commit and review them like code.
  `rules/` — 项目作用域规则文件（Markdown + YAML frontmatter）。仓库内所有代理都会继承，请像代码一样提交与评审。
- `examples/` — copy-ready rule templates (NOT binding). Copy one into `rules/` (or `local/` for an
  environment note) and edit it to activate. See `contract-example.md`, `constraint-example.md`,
  `environment-example.md`.
  `examples/` — 可复制的规则模板（不会生效）。复制到 `rules/`（环境说明复制到 `local/`）并修改即可启用。
- `local/` — environment notes for THIS user × THIS project × THIS machine (e.g. "agents run inside
  WSL"). Git-ignored (a `.gitignore` inside ignores everything but itself), so machine-specific facts
  are never committed. They are descriptive context, not competing contracts: always applied, and
  delivered only via the MCP/hook briefing — never exported into `AGENTS.md`/`CLAUDE.md`.
  `local/` — 当前用户 × 当前项目 × 当前机器的工作环境说明（如「代理运行在 WSL 内」）。已被 git 忽略，
  机器相关事实不会提交。它是描述性上下文而非竞争性契约：始终生效，仅通过 MCP/hook 简报传递，不导出到入口文件。
- Personal preferences are user-scope and live separately in `~/.agent-control/rules/user/`; they
  follow you across projects and are NOT stored here. Do not put machine/project-specific environment
  facts there — those belong in `local/`.
  个人偏好为用户级，单独存于 `~/.agent-control/rules/user/`，跨项目跟随你，不保存在此。
  机器/项目相关的环境事实不要放在那里——它们属于 `local/`。

Agents discover these rules over MCP (`get_applicable_rules`) and freeze them into a session
snapshot (`create_session`) so the same constraints stay binding across harness handoffs.
代理通过 MCP（`get_applicable_rules`）发现这些规则，并在会话快照（`create_session`）中冻结，
确保跨 harness 交接后同一约束仍然生效。

Agents can also record a newly discovered project contract actively via the MCP `save_rule` tool
(id, title, content, kind, scope=project, project_path); it writes a file into `rules/` that shows
up in the console and binds future sessions.
代理也可通过 MCP `save_rule` 工具主动记录新发现的项目契约，写入 `rules/` 后即出现在指挥台并约束后续会话。
