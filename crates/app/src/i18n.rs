use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    En,
    Zh,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Zh => "zh",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "zh" => Locale::Zh,
            _ => Locale::En,
        }
    }
}

/// Lookup a static UI string. Unknown keys fall back to the key itself.
pub fn t(locale: Locale, key: &'static str) -> &'static str {
    match STRINGS.iter().find(|(k, _, _)| *k == key) {
        Some((_, en, zh)) => match locale {
            Locale::En => en,
            Locale::Zh => zh,
        },
        None => key,
    }
}

/// Format a template by replacing each `{}` placeholder in order with `args`.
pub fn tr(locale: Locale, key: &'static str, args: &[&str]) -> String {
    let template = t(locale, key);
    let mut out = template.to_string();
    for arg in args {
        out = out.replacen("{}", arg, 1);
    }
    out
}

pub fn prefs_path(root: &Path) -> std::path::PathBuf {
    root.join("ui.json")
}

pub fn load(root: &Path) -> Locale {
    let Ok(text) = std::fs::read_to_string(prefs_path(root)) else {
        return Locale::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Locale::default();
    };
    Locale::from_code(value.get("locale").and_then(|v| v.as_str()).unwrap_or("en"))
}

pub fn save(root: &Path, locale: Locale) {
    let _ = std::fs::write(
        prefs_path(root),
        serde_json::json!({ "locale": locale.code() }).to_string(),
    );
}

/// (key, English, Simplified Chinese)
const STRINGS: &[(&str, &str, &str)] = &[
    // Brand / navigation / header
    (
        "app.subtitle",
        "engineering protocol console",
        "工程协议指挥台",
    ),
    ("app.version", "v{}", "v{}"),
    ("nav.dashboard", "Dashboard", "仪表盘"),
    ("nav.preferences", "Preferences", "个人偏好"),
    ("nav.rules", "Project Contracts", "项目契约"),
    ("nav.provenance", "Provenance", "溯源"),
    ("nav.evidence", "Evidence Chain", "证据链"),
    ("nav.sessions", "Sessions & Handoff", "会话与交接"),
    ("nav.settings", "Settings", "设置"),
    ("header.dashboard", "Dashboard", "仪表盘"),
    ("header.preferences", "Personal Preferences", "个人偏好"),
    ("header.rules", "Project Contracts", "项目契约"),
    ("header.provenance", "Provenance Explorer", "溯源浏览器"),
    ("header.evidence", "Verifiable Evidence", "可验证证据"),
    ("header.sessions", "Sessions & Continuity", "会话与连续性"),
    ("header.settings", "Settings", "设置"),
    ("common.refresh", "Refresh", "刷新"),
    ("mcp.running", "running", "运行中"),
    ("mcp.daemon", "daemon", "守护进程"),
    ("mcp.stopped", "stopped", "已停止"),
    ("footer.mcp", "MCP: {}", "MCP: {}"),
    // Dashboard
    ("dash.stat.rules", "Rules & contracts", "规则与契约"),
    ("dash.stat.sessions", "Sessions", "会话"),
    ("dash.stat.provenance", "Provenance records", "溯源记录"),
    ("dash.stat.conflicts", "Open conflicts", "待解决冲突"),
    ("dash.mcp.running", "MCP running", "MCP 运行中"),
    ("dash.mcp.stopped", "MCP stopped", "MCP 已停止"),
    ("dash.harness", "harness: {}", "harness: {}"),
    ("dash.no_harness", "no harness", "无 harness"),
    ("dash.perm.required", "permission required", "需要授权"),
    ("dash.perm.allow_once", "Allow once", "单次允许"),
    ("dash.perm.allow_always", "Allow always", "总是允许"),
    ("dash.perm.reject", "Reject", "拒绝"),
    ("dash.perm.cancel", "Cancel", "取消"),
    ("dash.conflict.rule_vs_rule", "rule vs rule", "规则间冲突"),
    (
        "dash.conflict.rule_vs_code",
        "rule vs code",
        "规则与代码冲突",
    ),
    ("dash.hint", "hint: {}", "提示: {}"),
    (
        "dash.check_conflicts",
        "Check project conflicts",
        "检查项目冲突",
    ),
    ("dash.conflicts", "Conflicts", "冲突"),
    ("dash.activity", "Activity log", "活动日志"),
    // Rules
    ("rules.title", "Rules", "规则"),
    ("rules.editor", "Editor", "编辑器"),
    ("rules.id", "Rule id", "规则 ID"),
    ("rules.title_label", "Title", "标题"),
    (
        "rules.topic",
        "Competition topic (optional)",
        "竞争主题（可选）",
    ),
    ("rules.scope", "Scope", "作用域"),
    ("scope.user", "User", "用户"),
    ("scope.project", "Project", "项目"),
    ("scope.session", "Session", "会话"),
    ("scope.user.tag", "user", "用户"),
    ("scope.project.tag", "project", "项目"),
    ("scope.session.tag", "session", "会话"),
    ("rules.on", "on", "启用"),
    ("rules.off", "off", "禁用"),
    (
        "rules.project_path",
        "Project path (for project scope / delete)",
        "项目路径（用于项目作用域 / 删除）",
    ),
    ("rules.content", "Content", "内容"),
    (
        "rules.meta",
        "version {} · priority {} · source {}",
        "版本 {} · 优先级 {} · 来源 {}",
    ),
    ("rules.checksum", "checksum {}", "校验和 {}"),
    ("rules.save", "Save rule", "保存规则"),
    ("rules.delete", "Delete", "删除"),
    ("rules.saved", "Rule saved", "规则已保存"),
    ("rules.enabled", "Enabled", "启用"),
    (
        "rules.enabled_hint",
        "Disabled rules are excluded from resolution and do not bind sessions.",
        "禁用的规则不参与解析，也不会约束会话。",
    ),
    ("rules.enabled_toast", "Rule enabled", "规则已启用"),
    ("rules.disabled_toast", "Rule disabled", "规则已禁用"),
    // Provenance
    ("prov.title", "Provenance", "溯源"),
    ("prov.rules_count", "{} rules", "{} 条规则"),
    (
        "prov.empty",
        "Select a provenance record to inspect its decision trace.",
        "选择一条溯源记录以查看其决策轨迹。",
    ),
    ("prov.context", "Query context", "查询上下文"),
    ("prov.agent", "agent: {}", "代理: {}"),
    ("prov.project", "project: {}", "项目: {}"),
    ("prov.languages", "languages: {}", "语言: {}"),
    ("prov.tags", "task tags: {}", "任务标签: {}"),
    ("prov.id", "provenance id: {}", "溯源 ID: {}"),
    ("prov.trace", "Decision trace", "决策轨迹"),
    ("prov.superseded_by", "superseded by: {}", "被取代: {}"),
    ("decision.applied", "applied", "已应用"),
    ("decision.superseded", "superseded", "被取代"),
    ("decision.conflict", "conflict", "冲突"),
    ("decision.filtered", "filtered out", "被过滤"),
    ("decision.disabled", "disabled", "已禁用"),
    (
        "reason.agent",
        "agent '{}' matched pattern '{}'",
        "代理 '{}' 匹配模式 '{}'",
    ),
    (
        "reason.path",
        "path '{}' matched pattern '{}'",
        "路径 '{}' 匹配模式 '{}'",
    ),
    ("reason.language", "language '{}' matched", "语言 '{}' 匹配"),
    ("reason.tag", "task tag '{}' matched", "任务标签 '{}' 匹配"),
    (
        "reason.universal",
        "universal rule (no filters)",
        "通用规则（无过滤）",
    ),
    (
        "reason.scope",
        "scope precedence: {} beats {}",
        "作用域优先: {} 胜过 {}",
    ),
    (
        "reason.priority",
        "priority {} beats {}",
        "优先级 {} 胜过 {}",
    ),
    ("reason.recency", "won by recency", "按最近优先胜出"),
    (
        "reason.conflict",
        "explicit conflict with '{}'",
        "与 '{}' 显式冲突",
    ),
    (
        "reason.codecheck",
        "code check failed: {} [{}] expected '{}', found '{}'",
        "代码检查失败: {} [{}] 期望 '{}'，实际 '{}'",
    ),
    ("reason.excluded", "excluded: {}", "被排除: {}"),
    // Evidence
    ("ev.session", "Session:", "会话:"),
    ("ev.verify", "Verify chain", "验证链"),
    (
        "ev.empty",
        "No evidence for this session yet.",
        "该会话暂无证据。",
    ),
    ("ev.valid", "chain VALID", "链有效"),
    ("ev.broken", "chain BROKEN", "链损坏"),
    ("ev.entries", "{} entries checked", "已检查 {} 条"),
    ("ev.broken_at", "broken at: {}", "损坏位置: {}"),
    ("ev.none", "none", "无"),
    ("ev.head", "head hash: {}", "头哈希: {}"),
    ("ev.prev", "prev: {}…", "前驱: {}…"),
    ("ev.hash", "hash: {}…", "哈希: {}…"),
    // Sessions
    ("sess.title", "Sessions", "会话"),
    ("sess.new", "New session", "新建会话"),
    ("sess.create", "Create session", "创建会话"),
    ("sess.created", "Session created", "会话已创建"),
    ("sess.status.active", "active", "进行中"),
    ("sess.status.handed_off", "handed off", "已交接"),
    ("sess.status.completed", "completed", "已完成"),
    ("sess.status.failed", "failed", "失败"),
    (
        "sess.binding",
        "{} binding rules · {}",
        "{} 条绑定规则 · {}",
    ),
    ("sess.harness.running", "harness running", "harness 运行中"),
    ("sess.session_label", "session {}", "会话 {}"),
    ("sess.pending", "pending…", "等待中…"),
    ("sess.stop", "Stop", "停止"),
    ("sess.send", "Send prompt", "发送提示"),
    ("sess.cancel_turn", "Cancel turn", "取消本轮"),
    (
        "sess.no_harness",
        "No harness running. Pick an agent, set task + project, then start.",
        "无运行中的 harness。选择代理、设置任务与项目后启动。",
    ),
    ("sess.start", "Start harness", "启动 harness"),
    ("sess.selected", "Selected session", "当前会话"),
    (
        "sess.agent_project",
        "agent: {} · project: {}",
        "代理: {} · 项目: {}",
    ),
    (
        "sess.snapshot",
        "constraint snapshot: {}… · provenance {}",
        "约束快照: {}… · 溯源 {}",
    ),
    ("sess.binding_title", "Binding constraints", "绑定约束"),
    (
        "sess.handoff_title",
        "Handoff to another harness",
        "交接给另一个 harness",
    ),
    ("sess.export", "Export bundle", "导出交接包"),
    ("sess.exported", "Handoff bundle exported", "交接包已导出"),
    ("sess.ack", "acknowledged", "已确认"),
    ("sess.pending_ack", "pending ack", "待确认"),
    ("sess.bundle", "bundle {}… · {}", "交接包 {}… · {}"),
    (
        "sess.empty",
        "Select or create a session to see its binding constraints and handoffs.",
        "选择或创建会话以查看其绑定约束与交接。",
    ),
    // Settings
    ("set.mcp", "MCP server", "MCP 服务"),
    (
        "set.mcp.endpoint",
        "Streamable HTTP endpoint: http://{}/mcp",
        "Streamable HTTP 端点: http://{}/mcp",
    ),
    (
        "set.mcp.desc",
        "Agents connect here to read applicable rules, record evidence, acknowledge constraints and export handoffs. Runs as a background daemon that stays alive after you close the console.",
        "代理在此连接以读取适用规则、记录证据、确认约束并导出交接。以后台守护进程运行，关闭指挥台后仍常驻。",
    ),
    ("set.mcp.stop", "Stop MCP server", "停止 MCP 服务"),
    ("set.mcp.start", "Start MCP server", "启动 MCP 服务"),
    ("set.agents", "Registered agents", "已注册代理"),
    (
        "set.agents.desc",
        "Edit the agents registry file to add or change harness commands.",
        "编辑代理注册文件以添加或修改 harness 命令。",
    ),
    ("set.layout", "Local data layout", "本地数据布局"),
    (
        "set.layout.desc",
        "Rules live in git-friendly Markdown/YAML files; sessions, provenance and the evidence chain live in SQLite.",
        "规则存于 git 友好的 Markdown/YAML 文件；会话、溯源与证据链存于 SQLite。",
    ),
    ("set.path.root", "Data root", "数据根目录"),
    ("set.path.db", "SQLite database", "SQLite 数据库"),
    ("set.path.user_rules", "User rules", "用户规则"),
    ("set.path.agents", "Agents registry", "代理注册表"),
    ("set.path.bundles", "Handoff bundles", "交接包"),
    ("set.path.logs", "Logs (app / mcp)", "日志（app / mcp）"),
    ("set.layers", "The four layers", "四层架构"),
    (
        "set.layer1",
        "1. Personal preferences — user-scope rules that follow you across projects.",
        "1. 个人偏好 — 跨项目跟随你的用户作用域规则。",
    ),
    (
        "set.layer2",
        "2. Project contracts — project-scope rules that bind every agent working in a repo.",
        "2. 项目契约 — 约束仓库内所有代理的项目作用域规则。",
    ),
    (
        "set.layer3",
        "3. Cross-harness continuity — constraint snapshots and handoff bundles keep the same rules binding when a task moves to another agent.",
        "3. 跨 harness 连续性 — 约束快照与交接包确保任务移交后同一规则仍然生效。",
    ),
    (
        "set.layer4",
        "4. Verifiable evidence — a SHA-256 hash chain records what each agent did, so tampering is detectable.",
        "4. 可验证证据 — SHA-256 哈希链记录每个代理的行为，篡改可被检测。",
    ),
    ("set.language", "Language", "语言"),
    (
        "set.language.desc",
        "Choose the display language for the console.",
        "选择指挥台的显示语言。",
    ),
    ("lang.en", "English", "English"),
    ("lang.zh", "中文", "中文"),
    // Placeholders
    ("ph.rule_id", "rule-id", "规则 ID"),
    ("ph.rule_title", "Human readable title", "易读标题"),
    (
        "ph.rule_topic",
        "competition topic (optional)",
        "竞争主题（可选）",
    ),
    ("ph.project", "C:/path/to/project", "C:/路径/到/项目"),
    ("ph.task", "What should the agent do?", "希望代理做什么？"),
    ("ph.handoff", "target harness id", "目标 harness ID"),
    // Activity log / notifications
    ("log.ready", "console ready", "指挥台就绪"),
    (
        "log.rule_required",
        "rule id and title are required",
        "规则 ID 与标题必填",
    ),
    (
        "log.project_required",
        "project scope requires a project path",
        "项目作用域需要项目路径",
    ),
    ("log.saved", "saved rule '{}' → {}", "已保存规则 '{}' → {}"),
    ("log.save_failed", "save failed: {}", "保存失败: {}"),
    ("log.deleted", "deleted rule '{}'", "已删除规则 '{}'"),
    ("log.not_found", "rule file not found", "未找到规则文件"),
    ("log.delete_failed", "delete failed: {}", "删除失败: {}"),
    ("log.rule_enabled", "enabled rule '{}'", "已启用规则 '{}'"),
    ("log.rule_disabled", "disabled rule '{}'", "已禁用规则 '{}'"),
    ("log.toggle_failed", "toggle failed: {}", "切换失败: {}"),
    (
        "log.task_required",
        "task and project path are required",
        "任务与项目路径必填",
    ),
    (
        "log.session_created",
        "session {} created with {} binding rules",
        "会话 {} 已创建，含 {} 条绑定规则",
    ),
    (
        "log.session_failed",
        "session creation failed: {}",
        "会话创建失败: {}",
    ),
    (
        "log.already_running",
        "a harness is already running; stop it first",
        "已有 harness 运行；请先停止",
    ),
    ("log.no_agent", "no agent registered", "未注册代理"),
    (
        "log.set_first",
        "set task and project path first",
        "请先设置任务与项目路径",
    ),
    (
        "log.harness_starting",
        "harness starting…",
        "harness 启动中…",
    ),
    (
        "log.harness_failed",
        "harness start failed: {}",
        "harness 启动失败: {}",
    ),
    ("log.prompt_sent", "prompt sent: {}", "已发送提示: {}"),
    ("log.send_failed", "send failed: {}", "发送失败: {}"),
    ("log.cancel_sent", "cancel sent", "已发送取消"),
    ("log.harness_stopping", "harness stopping", "harness 停止中"),
    (
        "log.select_session",
        "select a session first",
        "请先选择会话",
    ),
    (
        "log.enter_target",
        "enter a target harness id",
        "请输入目标 harness ID",
    ),
    (
        "log.handoff_exported",
        "handoff bundle exported: {} → {} (hash {}…)",
        "交接包已导出: {} → {}（哈希 {}…）",
    ),
    ("log.handoff_failed", "handoff failed: {}", "交接失败: {}"),
    (
        "log.chain",
        "chain verification: {} ({} entries)",
        "链验证: {}（{} 条）",
    ),
    ("log.valid", "VALID", "有效"),
    ("log.broken", "BROKEN", "损坏"),
    (
        "log.verify_failed",
        "verification failed: {}",
        "验证失败: {}",
    ),
    (
        "log.enter_project",
        "enter a project path to check conflicts",
        "请输入项目路径以检查冲突",
    ),
    (
        "log.resolved",
        "resolved {} applicable rules, {} conflicts (provenance {})",
        "解析 {} 条适用规则，{} 冲突（溯源 {}）",
    ),
    ("log.resolve_failed", "resolve failed: {}", "解析失败: {}"),
    (
        "log.perm_requested",
        "permission requested: {}",
        "请求授权: {}",
    ),
    (
        "log.perm_decision",
        "permission decision: {}",
        "授权决策: {}",
    ),
    ("log.disconnected", "harness disconnected", "harness 已断开"),
    (
        "log.session_evt",
        "session {} created: {} rules, snapshot {}…",
        "会话 {} 已创建: {} 条规则，快照 {}…",
    ),
    (
        "log.mcp_stopped",
        "MCP HTTP server stopped",
        "MCP HTTP 服务已停止",
    ),
    (
        "log.mcp_starting",
        "MCP HTTP server starting",
        "MCP HTTP 服务启动中",
    ),
    (
        "log.mcp_reuse",
        "reusing running MCP daemon at {}",
        "复用运行中的 MCP 守护进程：{}",
    ),
    (
        "log.mcp_daemon",
        "started background MCP daemon at {} (survives closing the console)",
        "已启动后台 MCP 守护进程：{}（关闭指挥台后仍常驻）",
    ),
    // Projects / onboarding
    ("nav.projects", "Projects", "项目"),
    ("header.projects", "Project Management", "项目管理"),
    ("header.project", "Project: {}", "项目：{}"),
    ("header.no_project", "No project", "未选择项目"),
    ("proj.title", "Projects", "项目"),
    ("proj.tree", "Project tree", "项目树"),
    ("proj.contracts", "Contracts", "契约"),
    ("proj.sessions", "Sessions", "会话"),
    ("proj.conflicts", "Conflicts", "冲突"),
    ("proj.add", "Add project", "接入项目"),
    (
        "proj.empty",
        "No projects yet. Onboard a project to organize its contracts, sessions and evidence.",
        "尚无项目。接入一个项目以组织其契约、会话与证据。",
    ),
    ("proj.onboard_title", "Onboard a project", "接入项目"),
    (
        "proj.onboard_desc",
        "Pick a repository folder. Agent Control registers it and scaffolds .agent-control/rules/ for project contracts; existing rules are preserved.",
        "选择一个仓库文件夹。Agent Control 会注册它并创建 .agent-control/rules/ 用于项目契约；已有规则会被保留。",
    ),
    ("proj.path", "Project folder", "项目文件夹"),
    ("proj.name", "Display name", "显示名称"),
    ("proj.browse", "Browse…", "浏览…"),
    ("proj.onboard", "Onboard", "接入"),
    (
        "proj.guidance_title",
        "How project contracts work",
        "项目契约如何运作",
    ),
    (
        "proj.guidance1",
        "Project-level rules live in <project>/.agent-control/rules/ and bind every agent working in that repo. Commit them like code.",
        "项目级规则位于 <project>/.agent-control/rules/，约束该仓库内的所有代理，请像代码一样提交。",
    ),
    (
        "proj.guidance2",
        "User-level personal preferences are separate: they live in ~/.agent-control/rules/user/ and follow you across all projects.",
        "用户级个人偏好独立存放：位于 ~/.agent-control/rules/user/，跨所有项目跟随你。",
    ),
    (
        "proj.guidance3",
        "Agents read these rules over MCP and freeze them into a session snapshot, so the same constraints stay binding after a handoff to another harness.",
        "代理通过 MCP 读取这些规则并冻结为会话快照，确保交接给另一个 harness 后同一约束仍然生效。",
    ),
    ("proj.detail", "Project details", "项目详情"),
    ("proj.current", "current", "当前"),
    ("proj.set_current", "Set as current", "设为当前"),
    ("proj.rename", "Rename", "重命名"),
    ("proj.reveal", "Open contracts folder", "打开契约目录"),
    ("proj.remove", "Remove", "移除"),
    (
        "proj.remove_hint",
        "Removing only unregisters the project; files in the repo are kept.",
        "移除仅注销项目，仓库内文件会保留。",
    ),
    ("proj.name_label", "Name", "名称"),
    ("proj.path_label", "Path", "路径"),
    ("proj.created", "Created {}", "创建于 {}"),
    ("proj.opened", "Last opened {}", "最近打开 {}"),
    (
        "proj.counts",
        "{} contracts · {} sessions",
        "{} 条契约 · {} 个会话",
    ),
    (
        "proj.select_hint",
        "Select a project in the tree to see its details, or onboard a new one.",
        "在树中选择一个项目查看详情，或接入新项目。",
    ),
    ("proj.rename_title", "Rename project", "重命名项目"),
    ("proj.rename_save", "Save name", "保存名称"),
    // Rules scope separation
    ("rules.personal", "Personal", "个人偏好"),
    ("rules.project", "Project", "项目契约"),
    (
        "rules.personal_desc",
        "User-scope preferences that follow you across every project.",
        "用户级偏好，跨所有项目跟随你。",
    ),
    (
        "rules.project_desc",
        "Contracts for {} — bind every agent in this repo.",
        "{} 的契约 — 约束本仓库内所有代理。",
    ),
    (
        "rules.no_project",
        "Select or onboard a project to manage its contracts.",
        "选择或接入一个项目以管理其契约。",
    ),
    ("rules.go_projects", "Go to Projects", "前往项目管理"),
    // Dashboard / sessions project context
    ("dash.stat.user_rules", "Personal rules", "个人规则"),
    ("dash.stat.project_rules", "Project contracts", "项目契约"),
    (
        "dash.current_project",
        "Current project: {}",
        "当前项目：{}",
    ),
    ("dash.no_project", "No project selected", "未选择项目"),
    ("sess.current_project", "Project: {}", "项目：{}"),
    (
        "sess.no_project",
        "Select or onboard a project first.",
        "请先选择或接入一个项目。",
    ),
    // Placeholders (projects)
    ("ph.project_path", "C:/path/to/repo", "C:/路径/到/仓库"),
    ("ph.project_name", "Project display name", "项目显示名称"),
    // Activity log (projects)
    (
        "log.project_registered",
        "project '{}' onboarded → {}",
        "项目 '{}' 已接入 → {}",
    ),
    (
        "log.project_onboard_failed",
        "onboard failed: {}",
        "接入失败：{}",
    ),
    (
        "log.project_path_required",
        "enter or browse a project folder first",
        "请先输入或浏览选择项目文件夹",
    ),
    (
        "log.project_removed",
        "project '{}' removed (files kept)",
        "项目 '{}' 已移除（文件保留）",
    ),
    (
        "log.project_remove_failed",
        "remove failed: {}",
        "移除失败：{}",
    ),
    (
        "log.project_renamed",
        "project renamed to '{}'",
        "项目已重命名为 '{}'",
    ),
    ("log.current_project", "current project: {}", "当前项目：{}"),
    (
        "log.scaffold_failed",
        "contract scaffold update failed: {}",
        "契约脚手架更新失败：{}",
    ),
    (
        "log.no_project",
        "no project selected; onboard or pick one first",
        "未选择项目；请先接入或选择一个项目",
    ),
    (
        "log.contracts_init",
        "contracts scaffold ready → {}",
        "契约脚手架已就绪 → {}",
    ),
    (
        "log.browse_cancelled",
        "folder selection cancelled",
        "已取消文件夹选择",
    ),
    (
        "log.browse_failed",
        "folder picker failed: {}",
        "文件夹选择器失败：{}",
    ),
    // Shell: page subtitles, groups, status bar
    ("nav.suggestions", "Suggestions", "建议审核"),
    ("header.suggestions", "Rule Suggestions", "规则建议"),
    ("nav.group.workspace", "Workspace", "工作区"),
    ("nav.group.verify", "Verify", "验证"),
    ("nav.group.system", "System", "系统"),
    (
        "desc.dashboard",
        "Overview of rules, sessions and activity",
        "规则、会话与活动总览",
    ),
    (
        "desc.projects",
        "Organize repos, contracts and sessions",
        "组织仓库、契约与会话",
    ),
    (
        "desc.preferences",
        "User-scope preferences that follow you across projects",
        "跨项目跟随你的用户级偏好",
    ),
    (
        "desc.rules",
        "Project-scope contracts binding this repository",
        "绑定本仓库的项目级契约",
    ),
    (
        "desc.suggestions",
        "Review agent proposals before they bind",
        "在建议生效前审核代理提案",
    ),
    (
        "desc.provenance",
        "Why each rule was applied",
        "每条规则为何被应用",
    ),
    (
        "desc.evidence",
        "Tamper-evident hash chain of agent actions",
        "代理行为的防篡改哈希链",
    ),
    (
        "desc.sessions",
        "Constraint snapshots and cross-harness handoff",
        "约束快照与跨 harness 交接",
    ),
    (
        "desc.settings",
        "Server, agents, data and language",
        "服务、代理、数据与语言",
    ),
    ("header.manage_projects", "Manage projects…", "管理项目…"),
    ("common.cancel", "Cancel", "取消"),
    ("common.confirm", "Confirm", "确认"),
    ("common.delete", "Delete", "删除"),
    ("common.none", "None", "无"),
    // Suggestions review
    ("sugg.pending", "Pending", "待审核"),
    ("sugg.approved", "Approved", "已批准"),
    ("sugg.rejected", "Rejected", "已拒绝"),
    ("sugg.all", "All", "全部"),
    ("sugg.queue", "Review queue", "审核队列"),
    (
        "sugg.empty",
        "No suggestions yet. Agents can propose rules via the suggest_rule MCP tool.",
        "暂无建议。代理可通过 suggest_rule MCP 工具提出规则建议。",
    ),
    (
        "sugg.empty_filter",
        "No suggestions match this filter.",
        "没有符合此筛选的建议。",
    ),
    ("sugg.detail", "Proposal detail", "提案详情"),
    (
        "sugg.select_hint",
        "Select a suggestion to review, edit, then approve or reject it.",
        "选择一条建议以审核、编辑，然后批准或拒绝。",
    ),
    ("sugg.proposed_by", "proposed by {}", "提案者：{}"),
    ("sugg.rationale", "Agent rationale", "代理理由"),
    (
        "sugg.no_rationale",
        "No rationale provided.",
        "未提供理由。",
    ),
    ("sugg.edit_title", "Review & edit", "审核与编辑"),
    (
        "sugg.edit_hint",
        "Adjust the draft before approving; it will be written to the project's rules folder.",
        "批准前可修改草稿；批准后将写入项目规则目录。",
    ),
    (
        "sugg.edit_hint_user",
        "Adjust the draft before approving; it will be written to your personal preferences and apply across all projects.",
        "批准前可修改草稿；批准后将写入个人偏好，并在所有项目中生效。",
    ),
    ("sugg.note", "Review note (optional)", "审核备注（可选）"),
    (
        "ph.sugg_note",
        "Why approved / rejected…",
        "批准/拒绝的原因…",
    ),
    ("sugg.approve", "Approve → project rule", "批准 → 项目规则"),
    (
        "sugg.approve_user",
        "Approve → personal preference",
        "批准 → 个人偏好",
    ),
    ("sugg.reject", "Reject", "拒绝"),
    (
        "sugg.approved_toast",
        "Approved and written to project rules",
        "已批准并写入项目规则",
    ),
    (
        "sugg.approved_toast_user",
        "Approved and written to personal preferences",
        "已批准并写入个人偏好",
    ),
    ("sugg.rejected_toast", "Suggestion rejected", "建议已拒绝"),
    ("sugg.reviewed", "Reviewed {}", "审核于 {}"),
    (
        "sugg.approved_rule",
        "Approved as rule '{}'",
        "已批准为规则 '{}'",
    ),
    (
        "sugg.reject_confirm_title",
        "Reject this suggestion?",
        "拒绝此建议？",
    ),
    (
        "sugg.reject_confirm_desc",
        "The proposal will be marked rejected and no rule file is written. This cannot be undone.",
        "提案将被标记为已拒绝，不会写入规则文件。此操作不可撤销。",
    ),
    ("sugg.count", "{} pending", "{} 条待审核"),
    ("sugg.kind", "Kind", "类型"),
    ("kind.preference", "preference", "偏好"),
    ("kind.contract", "contract", "契约"),
    ("kind.constraint", "constraint", "约束"),
    ("dash.stat.suggestions", "Pending suggestions", "待审核建议"),
    (
        "dash.suggestions_hint",
        "Agent proposals waiting for your review",
        "等待你审核的代理提案",
    ),
    ("dash.review", "Review", "去审核"),
    (
        "log.sugg_approved",
        "suggestion approved → rule '{}' ({})",
        "建议已批准 → 规则 '{}'（{}）",
    ),
    (
        "log.sugg_rejected",
        "suggestion rejected: {}",
        "建议已拒绝：{}",
    ),
    ("log.sugg_failed", "review failed: {}", "审核失败：{}"),
    (
        "log.sugg_select",
        "select a pending suggestion first",
        "请先选择一条待审核建议",
    ),
    (
        "rules.delete_confirm_title",
        "Delete this rule?",
        "删除此规则？",
    ),
    (
        "rules.delete_confirm_desc",
        "The rule file will be removed from disk. Agents will no longer receive this rule.",
        "规则文件将从磁盘删除，代理将不再收到此规则。",
    ),
    (
        "proj.remove_confirm_title",
        "Remove this project?",
        "移除此项目？",
    ),
];
