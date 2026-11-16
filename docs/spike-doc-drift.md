# 探索 spike 评估报告：文档腐化检测（issue #1）

日期：2026-11-15 · 范围：仅阶段一（模型草案 + 原型 + 评估），产物是结论而非产品功能。

## 1. 结论先行

**文档声明模型成立，建议进 ACP 正式立项（附条件）。** 5 种 claim 类型 + 行号锚点反向锚定全部跑通；
18 条真实 claim 基线 0 误报；3 组人为腐化注入全部命中；JSON 报告被最小消费方脚本直接解析用于门控。
原型首轮运行即发现一个真实产品 bug（`code_checks` 的 TOML 解析，见 §4），直接证明了方向价值。

条件：稳定锚点替代纯行号、警告型 hook 渠道逐厂商实测、`command_runs` 限定廉价命令、claim 登记需要
工具化辅助以降低维护成本（§6、§7）。

## 2. 待决设计问题的落地选择

| 问题 | 选择 | 理由 |
|------|------|------|
| 声明放哪 | 独立清单 `.agent-control/docs/*.yaml`，每 claim 带 `anchor: {lines, contains}` 反向锚定文档段落 | 沿用 issue 倾向；零侵入文档；`contains` 使清单与文档脱节（二阶腐化）可检测（注入 C 验证） |
| 数据模型归属 | 独立 `DocClaim` 模型，**不**新增 `RuleKind::Doc` | 新 kind 会改变规则 frontmatter 契约（持久结构），超出 issue「不改 MCP 工具面承诺之外的持久结构」约束；spike 语义也更干净 |
| 证据记录 | 仅输出 JSON 报告，**不**新增 `EvidenceKind::DocCheck`、不入链 | 同上约束；入链留给 ACP。报告预留 `evaluated_at`，未来入链只需加证据引用字段 |

原型位置：`crates/core/examples/doc_drift.rs`（example 二进制，零 lib 公共面改动、不加 MCP 工具）。
运行：`mise exec -- cargo run -p agent-control-core --example doc_drift -- --project . [--out report.json]`。
退出码：0 全 fresh / 1 有漂移 / 2 求值错误。

## 3. 原型与输出契约

- 清单格式：`doc: <文档路径>` + `claims[]`（`id`、`type`、`anchor`、类型专属字段）。
- claim 类型：`path_exists`（≈glob）、`config_value`（复用 `evaluate_code_checks` 的
  JSON/TOML/YAML/INI 读取）、`command_runs`（shell 执行，期望退出 0）、`regex_in_file`（≈grep）、
  `version`（`config_value` 的语义糖，期望值=文档声称的版本）。
- 状态语义：`fresh`=求值且一致；`stale`=声明被证伪（路径消失/值不符/正则无匹配/退出非 0）；
  `broken`=求值基础设施失败（文件不可读/命令无法 spawn/文档缺失）。
- 报告 schema `doc-drift-report/1`：文档标识 + manifest 路径 + `doc_exists` + 逐 claim
  `{id, type, status, anchor, anchor_ok, expected, actual, detail}` + 每文档与全局
  `counts{fresh,stale,broken}` + `rot_score=(stale+broken)/total` + `evaluated_at`（RFC3339）。
- 消费方验证：`scripts/doc-drift-gate.mjs <report.json>` 直接解析，模拟索引门控——stale/broken
  claim 的锚点区间 `exclude`，anchor 漂移 `downweight`，文档 rot 超阈值整体 `downweight`。
  基线/注入 A/注入 C 三份报告均无需人工解读即产出 chunk 级门控计划。

## 4. 首轮即命中：一个真实的「文档 vs 代码」腐化

README 与 AGENTS.md 均声称 `code_checks` 支持 TOML，但 `conflict.rs::read_config_value` 实际用
serde_yaml 解析 TOML——任何含 `[table]` 的真实 TOML 都解析失败（`[tools]` 被 YAML 当作流序列）。
spike 的 `config_value` claim 全部 `broken` 暴露了它。已修复（改用 `toml` crate）并加回归测试
`code_check_reads_toml_tables`。**这是本 spike 唯一的 lib 改动，属 bug 修复而非新功能。**
它同时是方向价值的实证：文档的可证伪声明（「支持 TOML」）悄悄失效，正是 AC 已有
`RuleVsCode` 冲突检测的同构问题。

## 5. 覆盖面与误报（实测实例）

样本：README.md 16 条 + AGENTS.md 2 条，共 18 条 claim。基线：**18/18 fresh，0 误报**。

| 类型 | 条数 | 有效实例 | 误报/漏报风险 |
|------|------|----------|----------------|
| path_exists | 9 | 注入 B：重命名 `scripts/mcp-e2e.mjs` → `stale(missing)`，命中 | 不区分代码围栏/正文引用；动态产物路径（dist/）不宜登记 |
| config_value | 6 | TOML bug 修复后全 fresh；键路径 `tasks.build.run` 多级 traversal 正确 | 期望值需与文档文本手工同步（维护负担，§6） |
| command_runs | 1 | `mise --version` 退出 0 → fresh | **重命令不可行**：`mise run build` 分钟级且有副作用；无超时机制；环境抖动（网络/工具链）会产生「非文档腐化」的 stale |
| regex_in_file | 1 | `DEFAULT_MCP_PORT: u16 = 9870` 命中 config.rs:21 | 符号重命名→真阳性；正则过窄→格式变更误报，过宽→漏报 |
| version | 1 | 注入 A：Cargo.toml `rust-version` 1.95→1.96 → `stale(expected 1.95, actual 1.96)`，命中 | 同 config_value |
| anchor 反向锚定 | 18 | 注入 C：README 版本文本 1.99.0→1.98.0 → `anchor_ok=false`（二阶腐化），命中 | **设计缺口**：anchor 漂移不影响退出码与 rot_score，消费方需自行读 `anchor_ok`；建议 ACP 版单列 `anchor_drift` 计数 |

3 组注入全部命中、0 漏报；但样本小（2 文档 18 claim），误报率结论需更大语料与时间维度观察。

## 6. 维护成本观察

1. **行号锚点脆弱**：文档头部插一行，其后所有锚点整体平移。`contains` 能*检测*漂移但不能*自愈*。
   候选改进：稳定锚（标题 slug / 显式 HTML 注释标记），行号仅作提示。
2. **期望值是三份拷贝**：版本号出现在文档、清单、真实来源三处。清单本身是第二份文档——
   二阶腐化可检测（注入 C）但维护成本真实存在。候选改进：从文档自动抽取 claim 候选（正则扫
   路径/版本/命令），人工确认后落清单。
3. **登记粒度**：18 条只覆盖 README 的机械可验证子集；大量内容本质不可机械验证（§7）。
   腐化评分的分母是「已登记 claim」而非「文档全部声明」，报告语义须向消费方明示。

## 7. 永远停留在 Advisory 的内容

语义性描述（设计哲学、架构意图、「为什么」）、行为叙述、跨文件聚合计数（如「25 tools」——
理论上可经 MCP `tools/list` 内省机械验证，但已超出 ripgrep 级原语约束，列为 ACP 候选扩展）。
这些内容无 claim 可登记，腐化检测对它们无覆盖；与 AC 保证分级一致：语义规则永远 Advisory。

## 8. Read 拦截警告渠道：逐厂商可行性（仅评估，未实现）

干预点：PreToolUse 拦截 Read 类调用，被读文档存在 stale/broken claim 时**附加警告而非拒绝**。
与现有 `hard` 规则的 deny 语义不同，需要独立的「警告型输出形态」，只读、fail-open。

| 厂商形态 | 现状 | 警告渠道可行性 |
|----------|------|----------------|
| claude 式（Claude/CodeBuddy/WorkBuddy/Qoder/Qoder CN/Codex/Kimi） | gate 用 exit 2 + stderr 拒绝；exit 0 时 stderr **不**注入上下文 | 潜在可行：`hookSpecificOutput.permissionDecision="allow"` + reason；`additionalContext` 在 PreToolUse 是否被消费**需实测**（SessionStart/UserPromptSubmit 已支持）。依赖厂商版本 |
| Cursor | 必须合法 JSON；deny 带 `user_message`/`agent_message` | 待实测：`{permission:"allow", agent_message}` 的 message 是否在 allow 时送达 agent；若是则天然警告渠道 |
| Antigravity | `{decision:"ask"}` 保留用户权限策略；silence=deny | 待实测：`ask` + reason 的展示路径；PreInvocation 无注入渠道（reach 靠 GEMINI.md 指针） |

共同约束：
- 警告路径必须与 `check_tool_call` 裁决分离——不得把 doc-drift 警告混入 hard deny 语义。
- 逐次 Read 求值必须按文件 mtime/hash 缓存，否则延迟不可接受；原型引擎（配置解析 + regex +
  路径检查）满足「ripgrep 级原语、零检索基础设施」约束。
- 误报干扰：stale 判定直接来自 claim 求值（§5 基线 0 误报），但 command_runs 的环境抖动类 stale
  若进入警告渠道会造成噪音——警告渠道应只消费 path/config/regex/version 四类。

## 9. 明确不做（复核）

无自动修复；无 MCP 工具面/持久结构改动（TOML bug 修复除外）；无证据入链；无 hooks/CI 门禁集成；
仅 Windows；无向量库/图谱集成（只保证输出可被消费，§3 已验证）。

## 10. 对「是否值得进 ACP」的建议

**值得，附条件立项。** 模型与输出契约已验证、消费方解耦成立、首轮即捕获真实 bug。
ACP 立项前置：brief N（契约文件格式）需吸收稳定锚点与 claim 清单格式；brief Q（证据与溯源）
需决定 `DocCheck` 入链形态。阶段二（hooks/CI 门禁）启动前先完成 §8 的逐厂商警告渠道实测。

PR #2 review 补充的硬条件：

- `command_runs` 必须有**超时 + 廉价命令白名单**（挂起命令会无限阻塞求值；环境抖动类 stale
  不得进入警告渠道）。
- claim id 需 **`doc:id` 复合键**命名空间化（当前 `rust-pin` 在两份清单重复；全局入链的前置）。
- anchor 求值需**按文档缓存**（当前逐 claim 重读文档；§8 的 mtime/hash 缓存要求覆盖此点）。
- 清单 schema 的标量 coercion + `deny_unknown_fields` + 格式回归测试——**已在 spike 整改落地**（§11）。

## 11. Review 整改记录（PR #2）

**必须项（已修复）**：清单级求值错误（不可读 / YAML 非法）原先返回全零 counts，全局 drift=0 →
退出码 0「all fresh」，消费方亦无感——检测系统自身故障被静默吞掉，违反 broken ≠ fresh 原则。
整改：`error_report` 计入 `broken: 1`（随 totals 进入退出码 1 与全局 rot_score）；
`doc-drift-gate.mjs` 对 `doc.error` 输出整文档 `exclude` + 显式 stderr 告警，不再静默跳过。

**叠加风险（已修复）**：未加引号的标量（`expect: 9870` / `expect: 1.95`）原会使整份清单
反序列化失败并静默退出 0。整改：`scalar_string` coercion（数字/布尔→字符串，覆盖
expect/path/file/key/pattern/command/contains）；`DocManifest`/`DocClaim`/`Anchor` 均
`deny_unknown_fields`；example 内新增回归测试（`cargo test -p agent-control-core --example doc_drift`：
标量 coercion、未知字段拒绝、清单错误计 broken）。

**琐碎项（已修复）**：新增文件补齐末尾换行。
