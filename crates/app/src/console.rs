use std::path::PathBuf;
use std::sync::Arc;

use agent_control_core::model::{
    AgentRegistration, Conflict, Enforcement, EvidenceEntry, Project, ProvenanceRecord, Rule,
    RuleSuggestion, Scope, Session, SessionStatus, SuggestionEdits, SuggestionStatus,
    VerificationReport, WorkItem, WorkItemStatus,
};
use agent_control_core::{
    AppConfig, DoctorReport, ExportTarget, HookStatus, HookTarget, SearchHit, Store,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants, DropdownButton};
use gpui_kit::component::dialog::{Confirm, DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputState, TextareaState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tree::{TreeItem, TreeState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable, Sizable, StyledExt, TitleBar, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::i18n::{self, Locale};
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Dashboard,
    Projects,
    Preferences,
    Rules,
    Environment,
    Suggestions,
    Provenance,
    Evidence,
    Sessions,
    Settings,
}

pub struct LogEntry {
    pub time: String,
    pub text: String,
}

pub struct Console {
    pub store: Arc<Store>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub nav: Nav,
    pub locale: Locale,
    pub config: AppConfig,

    pub rules: Vec<Rule>,
    pub sessions: Vec<Session>,
    pub provenance: Vec<ProvenanceRecord>,
    pub agents: Vec<AgentRegistration>,
    pub conflicts: Vec<Conflict>,

    pub projects: Vec<Project>,
    pub current_project: Option<Uuid>,
    pub selected_project: Option<Uuid>,
    pub project_tree: Entity<TreeState>,
    pub project_stats: std::collections::HashMap<Uuid, (usize, usize)>,
    pub tree_rule_enabled: std::collections::HashMap<String, bool>,
    pub show_onboarding: bool,
    pub export_stale: bool,
    pub export_targets: Vec<ExportTarget>,
    pub hook_targets: Vec<HookTarget>,
    pub hook_statuses: Vec<HookStatus>,
    pub open_sections: std::collections::HashSet<String>,

    pub selected_rule: Option<usize>,
    pub selected_provenance: Option<usize>,
    pub selected_session: Option<Uuid>,
    pub evidence: Vec<EvidenceEntry>,
    pub verification: Option<VerificationReport>,

    pub suggestions: Vec<RuleSuggestion>,
    pub pending_suggestions: usize,
    pub selected_suggestion: Option<Uuid>,
    pub suggestion_filter: Option<SuggestionStatus>,
    pub sugg_rule_id_input: Entity<InputState>,
    pub sugg_title_input: Entity<InputState>,
    pub sugg_content_input: Entity<TextareaState>,
    pub sugg_note_input: Entity<InputState>,

    pub event_log: Vec<LogEntry>,

    pub mcp_running: bool,
    pub mcp_external: bool,
    pub mcp_shutdown: Option<CancellationToken>,
    pub mcp_bind: String,
    pub mcp_port_input: Entity<InputState>,
    pub doctor_report: Option<DoctorReport>,
    pub search_input: Entity<InputState>,
    pub search_hits: Option<Vec<SearchHit>>,

    pub rule_id_input: Entity<InputState>,
    pub rule_title_input: Entity<InputState>,
    pub rule_topic_input: Entity<InputState>,
    pub rule_content_input: Entity<TextareaState>,
    pub rules_tab: Scope,
    pub rule_enforcement: Enforcement,

    pub project_path_input: Entity<InputState>,
    pub project_name_input: Entity<InputState>,
    pub project_rename_input: Entity<InputState>,

    pub session_task_input: Entity<InputState>,
    pub selected_agent: usize,
    pub session_show_completed: bool,
    pub session_goal_input: Entity<TextareaState>,
    pub session_work_input: Entity<TextareaState>,
}

impl Console {
    pub fn new(
        store: Arc<Store>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = AppConfig::load(&store.layout);
        let locale = Locale::from_code(&config.locale);
        let mcp_bind = format!("127.0.0.1:{}", config.mcp_port);
        let rule_id_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.rule_id")));
        let rule_title_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.rule_title")));
        let rule_topic_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.rule_topic")));
        let rule_content_input = cx.new(|cx| TextareaState::new(window, cx).default_value(""));
        let project_path_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.project_path")));
        let project_name_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.project_name")));
        let project_rename_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.project_name")));
        let project_tree = cx.new(|cx| TreeState::new(cx));
        let session_task_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.task")));
        let session_goal_input =
            cx.new(|cx| TextareaState::new(window, cx).placeholder(i18n::t(locale, "ph.goal")));
        let session_work_input = cx
            .new(|cx| TextareaState::new(window, cx).placeholder(i18n::t(locale, "ph.work_items")));
        let sugg_rule_id_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.rule_id")));
        let sugg_title_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.rule_title")));
        let sugg_content_input = cx.new(|cx| TextareaState::new(window, cx).default_value(""));
        let sugg_note_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.sugg_note")));
        let mcp_port_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::t(locale, "ph.mcp_port"))
                .default_value(config.mcp_port.to_string())
        });
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.search")));

        let mut console = Self {
            store,
            runtime,
            nav: Nav::Dashboard,
            locale,
            config,
            rules: vec![],
            sessions: vec![],
            provenance: vec![],
            agents: vec![],
            conflicts: vec![],
            projects: vec![],
            current_project: None,
            selected_project: None,
            project_tree,
            project_stats: std::collections::HashMap::new(),
            tree_rule_enabled: std::collections::HashMap::new(),
            show_onboarding: false,
            export_stale: false,
            export_targets: ExportTarget::default_targets(),
            hook_targets: HookTarget::ALL.to_vec(),
            hook_statuses: vec![],
            open_sections: std::collections::HashSet::new(),
            selected_rule: None,
            selected_provenance: None,
            selected_session: None,
            evidence: vec![],
            verification: None,
            suggestions: vec![],
            pending_suggestions: 0,
            selected_suggestion: None,
            suggestion_filter: Some(SuggestionStatus::Pending),
            sugg_rule_id_input,
            sugg_title_input,
            sugg_content_input,
            sugg_note_input,
            event_log: vec![],
            mcp_running: false,
            mcp_external: false,
            mcp_shutdown: None,
            mcp_bind,
            mcp_port_input,
            doctor_report: None,
            search_input,
            search_hits: None,
            rule_id_input,
            rule_title_input,
            rule_topic_input,
            rule_content_input,
            rules_tab: Scope::User,
            rule_enforcement: Enforcement::Advisory,
            project_path_input,
            project_name_input,
            project_rename_input,
            session_task_input,
            selected_agent: 0,
            session_show_completed: false,
            session_goal_input,
            session_work_input,
        };
        console.refresh_data(cx);
        console.load_session_work_inputs(window, cx);
        if let Some(id) = console.current_project {
            console.ensure_project_scaffold(id);
        }
        console.start_mcp_http();
        console.log(i18n::t(locale, "log.ready"));
        console
    }

    pub fn t(&self, key: &'static str) -> &'static str {
        i18n::t(self.locale, key)
    }

    pub fn tr(&self, key: &'static str, args: &[&str]) -> String {
        i18n::tr(self.locale, key, args)
    }

    pub fn set_locale(&mut self, locale: Locale, window: &mut Window, cx: &mut Context<Self>) {
        self.locale = locale;
        self.config.locale = locale.code().to_string();
        let _ = self.config.save(&self.store.layout);
        self.rule_id_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.rule_id"), window, cx)
        });
        self.rule_title_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.rule_title"), window, cx)
        });
        self.rule_topic_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.rule_topic"), window, cx)
        });
        self.project_path_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.project_path"), window, cx)
        });
        self.project_name_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.project_name"), window, cx)
        });
        self.project_rename_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.project_name"), window, cx)
        });
        self.session_task_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.task"), window, cx)
        });
        self.session_goal_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.goal"), window, cx)
        });
        self.session_work_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.work_items"), window, cx)
        });
        self.sugg_rule_id_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.rule_id"), window, cx)
        });
        self.sugg_title_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.rule_title"), window, cx)
        });
        self.sugg_note_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.sugg_note"), window, cx)
        });
        self.mcp_port_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.mcp_port"), window, cx)
        });
        self.search_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.search"), window, cx)
        });
        self.rebuild_project_tree(cx);
        cx.notify();
    }

    pub fn current_project(&self) -> Option<&Project> {
        self.current_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
    }

    pub fn current_project_path(&self) -> Option<PathBuf> {
        self.current_project().map(|p| p.path.clone())
    }

    pub fn refresh_data(&mut self, cx: &mut App) {
        self.doctor_report = None;
        self.projects = self.store.list_projects().unwrap_or_default();
        let current_valid = self
            .current_project
            .map(|id| self.projects.iter().any(|p| p.id == id))
            .unwrap_or(false);
        if !current_valid {
            self.current_project = self.projects.first().map(|p| p.id);
        }
        let selected_valid = self
            .selected_project
            .map(|id| self.projects.iter().any(|p| p.id == id))
            .unwrap_or(false);
        if !selected_valid {
            self.selected_project = self.current_project;
        }
        self.rules = self
            .store
            .load_rules(self.current_project_path().as_deref())
            .unwrap_or_default();
        self.sessions = self.store.list_sessions().unwrap_or_default();
        self.provenance = self.store.list_provenance(50).unwrap_or_default();
        self.agents = self.store.load_agents().unwrap_or_default();
        if self.selected_session.is_none() {
            self.selected_session = self.sessions.first().map(|s| s.id);
        }
        self.suggestions = self.store.list_suggestions(None, None).unwrap_or_default();
        self.pending_suggestions = self
            .store
            .count_suggestions(None, Some(SuggestionStatus::Pending))
            .unwrap_or(0);
        if let Some(id) = self.selected_suggestion
            && !self.suggestions.iter().any(|s| s.id == id)
        {
            self.selected_suggestion = None;
        }
        self.refresh_evidence();
        self.rebuild_project_tree(cx);
        self.refresh_export_drift();
        self.refresh_hook_status();
    }

    /// Recompute whether the selected project's native export is stale (missing or outdated-format
    /// pointer block) for the currently selected targets. Cached here (refresh/select/export/toggle),
    /// never at render time. Project-only: the console export action does not write user-global files,
    /// so include_user=false.
    fn refresh_export_drift(&mut self) {
        self.export_stale = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| {
                self.store
                    .export_drift(&p.path, &self.export_targets, false)
                    .map(|r| r.stale)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
    }

    /// Detect which harnesses are installed and whether our hook is registered in the selected
    /// project's settings. Cached here (refresh/select/register/revoke), never at render time.
    fn refresh_hook_status(&mut self) {
        self.hook_statuses = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .and_then(|p| self.store.detect_hooks(&p.path).ok())
            .unwrap_or_default();
    }

    fn rebuild_project_tree(&mut self, cx: &mut App) {
        let mut roots = Vec::new();
        let mut stats = std::collections::HashMap::new();
        let mut rule_enabled = std::collections::HashMap::new();
        for project in &self.projects {
            let is_current = self.current_project == Some(project.id);
            let is_selected = self.selected_project == Some(project.id);
            let contracts: Vec<Rule> = self
                .store
                .load_rules(Some(&project.path))
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.scope == Scope::Project)
                .collect();
            let session_count = self
                .sessions
                .iter()
                .filter(|s| s.project_path == project.path)
                .count();
            let conflict_count = if is_current { self.conflicts.len() } else { 0 };
            stats.insert(project.id, (contracts.len(), session_count));

            let contract_leaves = contracts.iter().map(|rule| {
                rule_enabled.insert(format!("rule:{}:{}", project.id, rule.id), rule.enabled);
                TreeItem::new(
                    format!("rule:{}:{}", project.id, rule.id),
                    rule.title.clone(),
                )
            });
            let contracts_folder = TreeItem::new(
                format!("contracts:{}", project.id),
                format!("{} ({})", self.t("proj.contracts"), contracts.len()),
            )
            .expanded(is_current)
            .children(contract_leaves);
            let sessions_leaf = TreeItem::new(
                format!("sessions:{}", project.id),
                format!("{} ({})", self.t("proj.sessions"), session_count),
            );
            let conflicts_leaf = TreeItem::new(
                format!("conflicts:{}", project.id),
                format!("{} ({})", self.t("proj.conflicts"), conflict_count),
            );
            roots.push(
                TreeItem::new(format!("project:{}", project.id), project.name.clone())
                    .expanded(is_current || is_selected)
                    .child(contracts_folder)
                    .child(sessions_leaf)
                    .child(conflicts_leaf),
            );
        }
        self.project_stats = stats;
        self.tree_rule_enabled = rule_enabled;

        let selected_id = self.selected_project.map(|id| format!("project:{id}"));
        self.project_tree.update(cx, |state, cx| {
            state.set_items(roots, cx);
            if let Some(id) = selected_id
                && let Some(ix) = state.index_of(&id.into())
            {
                state.set_selected_index(Some(ix), cx);
            }
        });
    }

    pub fn refresh_evidence(&mut self) {
        if let Some(id) = self.selected_session {
            self.evidence = self.store.get_evidence(id).unwrap_or_default();
        } else {
            self.evidence.clear();
        }
        self.verification = None;
    }

    pub fn log(&mut self, text: impl Into<String>) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        self.event_log.push(LogEntry {
            time,
            text: text.into(),
        });
        if self.event_log.len() > 500 {
            self.event_log.drain(..self.event_log.len() - 500);
        }
    }

    pub fn start_mcp_http(&mut self) {
        let bind = self.mcp_bind.clone();

        // 1. Reuse an already-running daemon (single instance across consoles and GUI restarts).
        if daemon_listening(&bind) {
            self.mcp_external = true;
            self.mcp_running = true;
            self.log(self.tr("log.mcp_reuse", &[&bind]));
            return;
        }

        // 2. Spawn the sibling MCP binary as a detached background daemon that outlives the GUI.
        //    It resolves the same data root (portable marker sits next to both executables).
        if let Some(exe) = sibling_mcp_exe() {
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--bind").arg(&bind);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const DETACHED_PROCESS: u32 = 0x0000_0008;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
            }
            match cmd.spawn() {
                Ok(_) => {
                    self.mcp_external = true;
                    self.mcp_running = true;
                    self.log(self.tr("log.mcp_daemon", &[&bind]));
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to spawn MCP daemon; hosting in-process");
                }
            }
        }

        // 3. Fallback (dev run without a sibling binary): host in-process on the app runtime.
        let shutdown = CancellationToken::new();
        let store = self.store.clone();
        let sd = shutdown.clone();
        self.runtime.spawn(async move {
            if let Err(e) = agent_control_mcp::serve_http(store, &bind, sd).await {
                tracing::error!(error = %e, "MCP HTTP server failed");
            }
        });
        self.mcp_shutdown = Some(shutdown);
        self.mcp_external = false;
        self.mcp_running = true;
    }

    pub fn stop_mcp_http(&mut self) {
        if self.mcp_external {
            request_daemon_shutdown(&self.mcp_bind);
        } else if let Some(sd) = self.mcp_shutdown.take() {
            sd.cancel();
        }
        self.mcp_external = false;
        self.mcp_running = false;
    }

    /// Start/stop the MCP daemon from the sidebar icon button.
    pub fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().to_string();
        self.search_hits = Some(
            self.store
                .search(&query, None, None, 20)
                .unwrap_or_default(),
        );
        cx.notify();
    }

    pub fn run_doctor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.current_project_path() else {
            let msg = self.t("set.doctor.no_project");
            self.notify(window, cx, msg);
            return;
        };
        match self.store.doctor(&project, Some(&self.mcp_bind)) {
            Ok(report) => {
                let key = if report.healthy {
                    "set.doctor.toast_ok"
                } else {
                    "set.doctor.toast_issues"
                };
                let msg = self.t(key);
                self.doctor_report = Some(report);
                self.notify(window, cx, msg);
            }
            Err(err) => {
                let msg = self.tr("set.doctor.toast_failed", &[&err.to_string()]);
                self.notify(window, cx, &msg);
            }
        }
        cx.notify();
    }

    pub fn toggle_mcp_http(&mut self, cx: &mut Context<Self>) {
        if self.mcp_running {
            self.stop_mcp_http();
            self.log(self.t("log.mcp_stopped"));
        } else {
            self.start_mcp_http();
            self.log(self.t("log.mcp_starting"));
        }
        cx.notify();
    }

    /// Persist a new MCP port and restart the daemon on it, so test and production can bind
    /// distinct loopback ports instead of sharing one daemon.
    pub fn apply_mcp_port(&mut self, port: u16, cx: &mut Context<Self>) -> bool {
        if port == 0 {
            self.log(self.t("log.mcp_port_invalid"));
            return false;
        }
        self.config.mcp_port = port;
        if let Err(e) = self.config.save(&self.store.layout) {
            self.log(self.tr("log.mcp_port_failed", &[&e.to_string()]));
            return false;
        }
        self.stop_mcp_http();
        self.mcp_bind = format!("127.0.0.1:{port}");
        self.start_mcp_http();
        self.log(self.tr("log.mcp_port", &[&self.mcp_bind]));
        cx.notify();
        true
    }

    /// Read the Settings MCP port field, validate, persist and restart the daemon on it.
    pub fn save_mcp_port_from_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let raw = self.mcp_port_input.read(cx).value().trim().to_string();
        let Some(port) = raw.parse::<u16>().ok().filter(|p| *p > 0) else {
            let msg = self.t("log.mcp_port_invalid").to_string();
            self.log(msg.clone());
            self.notify(window, cx, &msg);
            return false;
        };
        if !self.apply_mcp_port(port, cx) {
            return false;
        }
        let msg = self.tr("log.mcp_port", &[&self.mcp_bind]);
        self.notify(window, cx, &msg);
        true
    }

    pub fn save_rule_from_inputs(&mut self, cx: &mut Context<Self>) -> bool {
        let id = self.rule_id_input.read(cx).value().trim().to_string();
        let title = self.rule_title_input.read(cx).value().trim().to_string();
        let topic = self.rule_topic_input.read(cx).value().trim().to_string();
        let content = self.rule_content_input.read(cx).value();
        if id.is_empty() || title.is_empty() {
            self.log(self.t("log.rule_required"));
            return false;
        }
        let scope = self.rules_tab;
        let project_path = if matches!(scope, Scope::Project | Scope::Environment) {
            match self.current_project_path() {
                Some(path) => Some(path),
                None => {
                    self.log(self.t("log.no_project"));
                    return false;
                }
            }
        } else {
            None
        };
        let mut rule = if let Some(existing) = self.rules.iter().find(|r| r.id == id) {
            existing.clone()
        } else {
            agent_control_core::store::new_user_rule(&id, &title, &content)
        };
        rule.title = title;
        rule.content = content.to_string();
        rule.topic = if topic.is_empty() { None } else { Some(topic) };
        rule.scope = scope;
        // Environment notes are descriptive context, never an enforced contract.
        rule.enforcement = if scope == Scope::Environment {
            Enforcement::Advisory
        } else {
            self.rule_enforcement
        };
        rule.version += 1;
        match self.store.save_rule(&rule, project_path.as_deref()) {
            Ok(path) => {
                self.log(self.tr("log.saved", &[&id, &path.display().to_string()]));
                self.refresh_data(cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.save_failed", &[&e.to_string()]));
                false
            }
        }
    }

    pub fn load_rule_into_editor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rule) = self.rules.get(index).cloned() else {
            return;
        };
        self.selected_rule = Some(index);
        self.rule_id_input
            .update(cx, |s, cx| s.set_value(&rule.id, window, cx));
        self.rule_title_input
            .update(cx, |s, cx| s.set_value(&rule.title, window, cx));
        self.rule_topic_input.update(cx, |s, cx| {
            s.set_value(rule.topic.as_deref().unwrap_or(""), window, cx)
        });
        self.rule_content_input
            .update(cx, |s, cx| s.set_value(&rule.content, window, cx));
        self.rule_enforcement = rule.enforcement;
        self.rules_tab = match rule.scope {
            Scope::Project => Scope::Project,
            Scope::Environment => Scope::Environment,
            _ => Scope::User,
        };
    }

    pub fn clear_rule_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_rule = None;
        self.rule_id_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.rule_title_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.rule_topic_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.rule_content_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.rule_enforcement = Enforcement::Advisory;
    }

    /// Open the rule editor as an on-demand modal (edit an existing rule, or draft a new one).
    /// The editor is never docked permanently, so switching views cannot leave stale content.
    pub fn open_rule_editor_dialog(
        &mut self,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match index {
            Some(i) => self.load_rule_into_editor(i, window, cx),
            None => self.clear_rule_editor(window, cx),
        }
        let title = self
            .t(if index.is_some() {
                "rules.editor_title_edit"
            } else {
                "rules.editor_title_new"
            })
            .to_string();
        let save = self.t("rules.save").to_string();
        let cancel = self.t("common.cancel").to_string();
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let body = weak.update(cx, |console, ccx| {
                console.render_rule_editor_form(ccx).into_any_element()
            });
            let Ok(body) = body else {
                return dialog.title(title.clone());
            };
            let save_weak = weak.clone();
            let cancel_weak = weak.clone();
            let cancel_label = cancel.clone();
            let save_label = save.clone();
            dialog
                .title(title.clone())
                .child(body)
                .width(px(560.))
                .centered()
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().trigger(move |b| b.label(cancel_label.clone())))
                        .child(
                            Button::new("rule-editor-save")
                                .label(save_label.clone())
                                .primary()
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(
                                        Box::new(Confirm { secondary: false }),
                                        cx,
                                    );
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let Some(entity) = save_weak.upgrade() else {
                        return true;
                    };
                    entity.update(cx, |this, ccx| {
                        if this.save_rule_from_inputs(ccx) {
                            let msg = this.t("rules.saved");
                            this.notify(window, ccx, msg);
                            this.clear_rule_editor(window, ccx);
                            ccx.notify();
                            true
                        } else {
                            ccx.notify();
                            false
                        }
                    })
                })
                .on_cancel(move |_, window, cx| {
                    if let Some(entity) = cancel_weak.upgrade() {
                        entity.update(cx, |this, ccx| {
                            this.clear_rule_editor(window, ccx);
                            ccx.notify();
                        });
                    }
                    true
                })
        });
    }

    /// Rename the selected project in a modal instead of a permanent inline input.
    pub fn open_rename_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_project else {
            return;
        };
        if let Some(name) = self
            .projects
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
        {
            self.project_rename_input
                .update(cx, |s, cx| s.set_value(&name, window, cx));
        }
        let title = self.t("proj.rename_title").to_string();
        let label = self.t("proj.name_label").to_string();
        let save = self.t("proj.rename_save").to_string();
        let cancel = self.t("common.cancel").to_string();
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let input = weak.update(cx, |console, _ccx| {
                Input::new(&console.project_rename_input)
                    .id("project-rename-input")
                    .into_any_element()
            });
            let Ok(input) = input else {
                return dialog.title(title.clone());
            };
            let ok_weak = weak.clone();
            let cancel_label = cancel.clone();
            let save_label = save.clone();
            dialog
                .title(title.clone())
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().child(label.clone()))
                        .child(input),
                )
                .width(px(420.))
                .centered()
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().trigger(move |b| b.label(cancel_label.clone())))
                        .child(
                            Button::new("project-rename-save")
                                .label(save_label.clone())
                                .primary()
                                .on_click(move |_, window, cx| {
                                    window.dispatch_action(
                                        Box::new(Confirm { secondary: false }),
                                        cx,
                                    );
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let Some(entity) = ok_weak.upgrade() else {
                        return true;
                    };
                    entity.update(cx, |this, ccx| {
                        if this.rename_project_from_input(ccx) {
                            let msg = this.t("proj.renamed_toast");
                            this.notify(window, ccx, msg);
                            ccx.notify();
                            true
                        } else {
                            ccx.notify();
                            false
                        }
                    })
                })
        });
    }

    pub fn confirm_delete_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_rule.is_none() {
            return;
        }
        let title = self.t("rules.delete_confirm_title").to_string();
        let desc = self.t("rules.delete_confirm_desc").to_string();
        let ok_text = self.t("common.delete").to_string();
        let cancel_text = self.t("common.cancel").to_string();
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let entity = entity.clone();
            alert
                .title(title.clone())
                .description(desc.clone())
                .show_cancel(true)
                .ok_text(ok_text.clone())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(cancel_text.clone())
                .on_ok(move |_, window, cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            this.delete_selected_rule(cx);
                            this.clear_rule_editor(window, cx);
                            cx.notify();
                        });
                    }
                    true
                })
        });
    }

    pub fn delete_selected_rule(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.selected_rule else {
            return;
        };
        let Some(rule) = self.rules.get(index) else {
            return;
        };
        let project_path = if matches!(rule.scope, Scope::Project | Scope::Environment) {
            self.current_project_path()
        } else {
            None
        };
        match self
            .store
            .delete_rule(&rule.id, rule.scope, project_path.as_deref())
        {
            Ok(true) => {
                self.log(self.tr("log.deleted", &[&rule.id]));
                self.selected_rule = None;
                self.refresh_data(cx);
            }
            Ok(false) => self.log(self.t("log.not_found")),
            Err(e) => self.log(self.tr("log.delete_failed", &[&e.to_string()])),
        }
    }

    /// Toggle enablement for the rule at `index` (card-level switch / context menu), independent of
    /// which rule the editor has open.
    pub fn set_rule_enabled_at(
        &mut self,
        index: usize,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rule) = self.rules.get(index) else {
            return false;
        };
        let project_path = if matches!(rule.scope, Scope::Project | Scope::Environment) {
            self.current_project_path()
        } else {
            None
        };
        match self
            .store
            .set_rule_enabled(&rule.id, rule.scope, project_path.as_deref(), enabled)
        {
            Ok(updated) => {
                self.log(self.tr(
                    if enabled {
                        "log.rule_enabled"
                    } else {
                        "log.rule_disabled"
                    },
                    &[&updated.id],
                ));
                self.refresh_data(cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.toggle_failed", &[&e.to_string()]));
                false
            }
        }
    }

    /// Update the editor's declared enforcement intent. The rule is only persisted on save.
    pub fn set_rule_enforcement(&mut self, enforcement: Enforcement, cx: &mut Context<Self>) {
        self.rule_enforcement = enforcement;
        cx.notify();
    }

    /// Evaluate the selected session's frozen code_checks against the repo and record the achieved
    /// assurance tiers in the evidence chain. Returns (verified, advisory) counts for the toast.
    pub fn verify_selected_session_constraints(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(usize, usize)> {
        let id = self.selected_session?;
        let agent_id = self
            .sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.agent_id.clone())
            .unwrap_or_else(|| "console".to_string());
        match self.store.verify_constraints(id, &agent_id) {
            Ok(report) => {
                self.refresh_data(cx);
                Some((report.verified, report.advisory))
            }
            Err(e) => {
                self.log(self.tr("log.verify_failed", &[&e.to_string()]));
                None
            }
        }
    }

    pub fn select_suggestion(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(suggestion) = self.suggestions.iter().find(|s| s.id == id).cloned() else {
            return;
        };
        self.selected_suggestion = Some(id);
        self.sugg_rule_id_input
            .update(cx, |s, cx| s.set_value(&suggestion.rule_id, window, cx));
        self.sugg_title_input
            .update(cx, |s, cx| s.set_value(&suggestion.title, window, cx));
        self.sugg_content_input
            .update(cx, |s, cx| s.set_value(&suggestion.content, window, cx));
        self.sugg_note_input
            .update(cx, |s, cx| s.set_value("", window, cx));
    }

    pub fn clear_suggestion_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_suggestion = None;
        self.sugg_rule_id_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sugg_title_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sugg_content_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sugg_note_input
            .update(cx, |s, cx| s.set_value("", window, cx));
    }

    pub fn approve_selected_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_suggestion else {
            self.log(self.t("log.sugg_select"));
            return;
        };
        let edits = SuggestionEdits {
            rule_id: Some(self.sugg_rule_id_input.read(cx).value().trim().to_string()),
            title: Some(self.sugg_title_input.read(cx).value().trim().to_string()),
            content: Some(self.sugg_content_input.read(cx).value().to_string()),
            ..Default::default()
        };
        let note = self.sugg_note_input.read(cx).value().trim().to_string();
        let note = if note.is_empty() { None } else { Some(note) };
        match self.store.approve_suggestion(id, edits, note) {
            Ok((_, rule, path)) => {
                self.log(self.tr(
                    "log.sugg_approved",
                    &[&rule.id, &path.display().to_string()],
                ));
                let message = self
                    .t(match rule.scope {
                        Scope::User => "sugg.approved_toast_user",
                        Scope::Environment => "sugg.approved_toast_environment",
                        _ => "sugg.approved_toast",
                    })
                    .to_string();
                self.notify(window, cx, &message);
                self.selected_suggestion = None;
                self.refresh_data(cx);
            }
            Err(e) => self.log(self.tr("log.sugg_failed", &[&e.to_string()])),
        }
    }

    pub fn confirm_reject_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_suggestion else {
            self.log(self.t("log.sugg_select"));
            return;
        };
        let title = self.t("sugg.reject_confirm_title").to_string();
        let desc = self.t("sugg.reject_confirm_desc").to_string();
        let ok_text = self.t("sugg.reject_ok").to_string();
        let cancel_text = self.t("common.cancel").to_string();
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let entity = entity.clone();
            alert
                .title(title.clone())
                .description(desc.clone())
                .show_cancel(true)
                .ok_text(ok_text.clone())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(cancel_text.clone())
                .on_ok(move |_, window, cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            this.reject_suggestion(id, window, cx);
                        });
                    }
                    true
                })
        });
    }

    fn reject_suggestion(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.sugg_note_input.read(cx).value().trim().to_string();
        let note = if note.is_empty() { None } else { Some(note) };
        match self.store.reject_suggestion(id, note) {
            Ok(suggestion) => {
                self.log(self.tr("log.sugg_rejected", &[&suggestion.rule_id]));
                let message = self.t("sugg.rejected_toast").to_string();
                self.notify(window, cx, &message);
                self.selected_suggestion = None;
                self.refresh_data(cx);
            }
            Err(e) => self.log(self.tr("log.sugg_failed", &[&e.to_string()])),
        }
    }

    /// Returns `Some(reused)` on success (`reused` = an existing active session was returned
    /// instead of creating a duplicate), `None` on failure (already logged).
    pub fn create_session_from_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let task = self.session_task_input.read(cx).value().trim().to_string();
        let Some(project) = self.current_project_path() else {
            self.log(self.t("log.no_project"));
            return None;
        };
        if task.is_empty() {
            self.log(self.t("log.task_required"));
            return None;
        }
        let agent_id = self
            .agents
            .get(self.selected_agent)
            .map(|a| a.id.clone())
            .unwrap_or_else(|| "manual".into());
        match self
            .store
            .create_session(&task, &project, &agent_id, vec![])
        {
            Ok((session, reused)) => {
                self.log(self.tr(
                    "log.session_created",
                    &[
                        &session.id.to_string(),
                        &session.constraint_snapshot.rules.len().to_string(),
                    ],
                ));
                self.selected_session = Some(session.id);
                self.refresh_data(cx);
                self.load_session_work_inputs(window, cx);
                Some(reused)
            }
            Err(e) => {
                self.log(self.tr("log.session_failed", &[&e.to_string()]));
                None
            }
        }
    }

    pub fn export_handoff(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(session_id) = self.selected_session else {
            self.log(self.t("log.select_session"));
            return false;
        };
        let context = self
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| serde_json::json!({ "summary": s.task, "exported_from": "console" }))
            .unwrap_or_else(|| serde_json::json!({ "exported_from": "console" }));
        match self.store.export_handoff(session_id, None, context) {
            Ok(bundle) => {
                self.log(self.tr(
                    "log.handoff_exported",
                    &[&bundle.from_agent, &format!("{:.12}", bundle.bundle_hash)],
                ));
                self.refresh_data(cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.handoff_failed", &[&e.to_string()]));
                false
            }
        }
    }

    /// Close (Completed) or reopen (Active) the selected session; the evidence chain is kept.
    pub fn set_selected_session_status(
        &mut self,
        status: SessionStatus,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session_id) = self.selected_session else {
            self.log(self.t("log.select_session"));
            return false;
        };
        match self.store.set_session_status(session_id, status) {
            Ok(session) => {
                let label = match session.status {
                    SessionStatus::Active => self.t("sess.status.active"),
                    SessionStatus::HandedOff => self.t("sess.status.handed_off"),
                    SessionStatus::Completed => self.t("sess.status.completed"),
                    SessionStatus::Failed => self.t("sess.status.failed"),
                };
                self.log(self.tr("log.session_status", &[&session.id.to_string(), label]));
                self.refresh_data(cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.session_failed", &[&e.to_string()]));
                false
            }
        }
    }

    /// Select a session: move the highlight, refresh its evidence, and load its work-state inputs.
    pub fn select_session(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_session = Some(id);
        self.refresh_evidence();
        self.load_session_work_inputs(window, cx);
        cx.notify();
    }

    /// Mirror the selected session's goal + work items into the editor textareas (empty when none).
    pub fn load_session_work_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (goal, work) = self
            .selected_session
            .and_then(|id| self.sessions.iter().find(|s| s.id == id))
            .map(|s| (s.goal.clone(), format_work_items(&s.work_items)))
            .unwrap_or_default();
        self.session_goal_input
            .update(cx, |state, cx| state.set_value(goal, window, cx));
        self.session_work_input
            .update(cx, |state, cx| state.set_value(work, window, cx));
    }

    /// Persist the edited goal + work items for the selected session.
    pub fn save_session_work_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(session_id) = self.selected_session else {
            self.log(self.t("log.select_session"));
            return false;
        };
        let goal = self.session_goal_input.read(cx).value().trim().to_string();
        let work_text = self.session_work_input.read(cx).value().to_string();
        let work_items = parse_work_items(&work_text);
        match self
            .store
            .set_session_work_state(session_id, goal, work_items)
        {
            Ok(session) => {
                self.log(self.tr(
                    "log.work_state_saved",
                    &[&session.work_items.len().to_string()],
                ));
                self.refresh_data(cx);
                self.load_session_work_inputs(window, cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.work_state_failed", &[&e.to_string()]));
                false
            }
        }
    }

    /// Delete the selected session and, cascade, its evidence chain + provenance record.
    pub fn delete_selected_session(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(session_id) = self.selected_session else {
            self.log(self.t("log.select_session"));
            return false;
        };
        match self.store.delete_session(session_id) {
            Ok(()) => {
                self.log(self.tr("log.session_deleted", &[&session_id.to_string()]));
                self.selected_session = None;
                self.refresh_data(cx);
                self.load_session_work_inputs(window, cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.session_delete_failed", &[&e.to_string()]));
                false
            }
        }
    }

    pub fn confirm_delete_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_session.is_none() {
            return;
        }
        let title = self.t("sess.delete_confirm_title").to_string();
        let desc = self.t("sess.delete_hint").to_string();
        let ok_text = self.t("sess.delete_ok").to_string();
        let cancel_text = self.t("common.cancel").to_string();
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let entity = entity.clone();
            alert
                .title(title.clone())
                .description(desc.clone())
                .show_cancel(true)
                .ok_text(ok_text.clone())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(cancel_text.clone())
                .on_ok(move |_, window, cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            if this.delete_selected_session(window, cx) {
                                let msg = this.t("sess.deleted_toast").to_string();
                                this.notify(window, cx, &msg);
                            }
                            cx.notify();
                        });
                    }
                    true
                })
        });
    }

    pub fn verify_selected_chain(&mut self) {
        let Some(session_id) = self.selected_session else {
            return;
        };
        match self.store.verify_chain(session_id) {
            Ok(report) => {
                self.log(self.tr(
                    "log.chain",
                    &[
                        self.t(if report.valid {
                            "log.valid"
                        } else {
                            "log.broken"
                        }),
                        &report.entries_checked.to_string(),
                    ],
                ));
                self.verification = Some(report);
            }
            Err(e) => self.log(self.tr("log.verify_failed", &[&e.to_string()])),
        }
    }

    pub fn resolve_conflicts_for_project(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.current_project_path() else {
            self.log(self.t("log.no_project"));
            return;
        };
        let ctx = agent_control_core::model::QueryContext {
            project_path: project,
            agent_id: self
                .agents
                .get(self.selected_agent)
                .map(|a| a.id.clone())
                .unwrap_or_else(|| "console".into()),
            file_paths: vec![],
            languages: vec![],
            task_tags: vec![],
            session_id: None,
        };
        match self.store.resolve_rules(&ctx) {
            Ok((record, resolution)) => {
                self.conflicts = resolution.conflicts.clone();
                self.log(self.tr(
                    "log.resolved",
                    &[
                        &resolution.applied.len().to_string(),
                        &resolution.conflicts.len().to_string(),
                        &record.id.to_string(),
                    ],
                ));
                self.refresh_data(cx);
            }
            Err(e) => self.log(self.tr("log.resolve_failed", &[&e.to_string()])),
        }
    }

    pub fn browse_project_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.t("proj.path").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => {
                    let _ = this.update_in(cx, |console, _, cx| {
                        console.log(console.t("log.browse_cancelled"));
                        cx.notify();
                    });
                    return;
                }
                Ok(Err(e)) => {
                    let msg = e.to_string();
                    let _ = this.update_in(cx, |console, _, cx| {
                        console.log(console.tr("log.browse_failed", &[&msg]));
                        cx.notify();
                    });
                    return;
                }
                Err(_) => return,
            };
            if let Some(path) = picked {
                let value = path.to_string_lossy().to_string();
                let _ = this.update_in(cx, |console, window, cx| {
                    console
                        .project_path_input
                        .update(cx, |s, cx| s.set_value(&value, window, cx));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn onboard_project(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let path_str = self.project_path_input.read(cx).value().trim().to_string();
        let name = self.project_name_input.read(cx).value().trim().to_string();
        if path_str.is_empty() {
            self.log(self.t("log.project_path_required"));
            return false;
        }
        let path = PathBuf::from(&path_str);
        match self.store.register_project(&name, &path, None) {
            Ok(project) => {
                let id = project.id;
                let pname = project.name.clone();
                let ppath = project.path.clone();
                match self.store.init_project_contracts(&ppath) {
                    Ok(rules_dir) => {
                        self.log(self.tr("log.contracts_init", &[&rules_dir.display().to_string()]))
                    }
                    Err(e) => {
                        self.log(self.tr("log.project_onboard_failed", &[&e.to_string()]));
                        return false;
                    }
                }
                self.current_project = Some(id);
                self.selected_project = Some(id);
                self.show_onboarding = false;
                self.log(self.tr(
                    "log.project_registered",
                    &[&pname, &ppath.display().to_string()],
                ));
                self.project_path_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                self.project_name_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                self.refresh_data(cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.project_onboard_failed", &[&e.to_string()]));
                false
            }
        }
    }

    pub fn select_project(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_project = Some(id);
        if let Some(name) = self
            .projects
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
        {
            self.project_rename_input
                .update(cx, |s, cx| s.set_value(&name, window, cx));
        }
        // Selection only: rebuilding items here would reset expand/collapse state, which the
        // tree row's own click toggle then flips again, so a root could never stay open.
        self.project_tree.update(cx, |state, cx| {
            if let Some(ix) = state.index_of(&format!("project:{id}").into()) {
                state.set_selected_index(Some(ix), cx);
            }
        });
        self.refresh_export_drift();
        self.refresh_hook_status();
        cx.notify();
    }

    /// Ensure the project's contract scaffold exists and migrate untouched templates to the current
    /// version. Idempotent and edit-safe; failures are logged, never fatal.
    fn ensure_project_scaffold(&mut self, id: Uuid) {
        let path = match self.store.get_project(id) {
            Ok(Some(project)) => project.path,
            _ => return,
        };
        if let Err(e) = self.store.init_project_contracts(&path) {
            self.log(self.tr("log.scaffold_failed", &[&e.to_string()]));
        }
    }

    pub fn set_current_project(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.current_project = Some(id);
        self.selected_project = Some(id);
        let _ = self.store.touch_project(id);
        self.ensure_project_scaffold(id);
        self.refresh_data(cx);
        let name = self
            .current_project()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        self.log(self.tr("log.current_project", &[&name]));
        cx.notify();
    }

    pub fn rename_project_from_input(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.selected_project else {
            return false;
        };
        let name = self
            .project_rename_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        match self.store.rename_project(id, &name) {
            Ok(()) => {
                self.log(self.tr("log.project_renamed", &[&name]));
                self.refresh_data(cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.save_failed", &[&e.to_string()]));
                false
            }
        }
    }

    pub fn reveal_project_contracts(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| p.path.clone())
        else {
            return;
        };
        let rules_dir = agent_control_core::DataLayout::project_rules_dir(&path);
        cx.reveal_path(&rules_dir);
    }

    /// Write the static pointer block into the selected project's chosen harness entry points. The
    /// block points at the live rules in `.agent-control/rules/`; it does not copy rule text.
    /// Project-only: personal preferences are not written to the user-global Claude memory here.
    pub fn setup_for_selected_project(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| p.path.clone())
        else {
            self.log(self.t("log.no_project"));
            return;
        };
        match self.store.setup(
            &path,
            &HookTarget::ALL,
            &ExportTarget::default_targets(),
            false,
        ) {
            Ok(report) => {
                let exports = report
                    .export
                    .targets
                    .iter()
                    .filter(|t| t.action != "unchanged")
                    .count();
                let hooks = report
                    .hooks
                    .targets
                    .iter()
                    .filter(|t| t.action != "unchanged")
                    .count();
                self.log(self.tr(
                    "log.setup_done",
                    &[&exports.to_string(), &hooks.to_string()],
                ));
            }
            Err(err) => {
                self.log(self.tr("log.setup_failed", &[&err.to_string()]));
            }
        }
        cx.notify();
    }

    pub fn export_native_for_selected_project(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| p.path.clone())
        else {
            self.log(self.t("log.no_project"));
            return;
        };
        match self.store.export_native(&path, &self.export_targets, false) {
            Ok(report) => {
                let targets = report
                    .targets
                    .iter()
                    .map(|t| format!("{} ({})", t.path.display(), t.action))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.log(self.tr(
                    "log.export_done",
                    &[&report.project_rules.to_string(), &targets],
                ));
            }
            Err(e) => self.log(self.tr("log.export_failed", &[&e.to_string()])),
        }
        self.refresh_export_drift();
        cx.notify();
    }

    /// Revoke the pointer block from the selected project's chosen harness entry points, preserving
    /// surrounding prose. The explicit undo of `export_native_for_selected_project`.
    pub fn revoke_native_for_selected_project(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| p.path.clone())
        else {
            self.log(self.t("log.no_project"));
            return;
        };
        match self.store.revoke_native(&path, &self.export_targets, false) {
            Ok(report) => {
                let targets = report
                    .targets
                    .iter()
                    .map(|t| format!("{} ({})", t.path.display(), t.action))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.log(self.tr("log.revoke_done", &[&targets]));
            }
            Err(e) => self.log(self.tr("log.revoke_failed", &[&e.to_string()])),
        }
        self.refresh_export_drift();
        cx.notify();
    }

    /// Toggle one harness entry point in the export selection. Keeps at least the order stable and
    /// recomputes the stale badge for the new selection.
    pub fn toggle_export_target(
        &mut self,
        target: ExportTarget,
        checked: bool,
        cx: &mut Context<Self>,
    ) {
        if checked {
            if !self.export_targets.contains(&target) {
                self.export_targets.push(target);
            }
        } else {
            self.export_targets.retain(|t| *t != target);
        }
        self.refresh_export_drift();
        cx.notify();
    }

    /// Confirm before revoking: the action can delete block-only files, so it goes through an alert
    /// dialog like the other destructive actions.
    pub fn confirm_revoke_native(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_project.is_none() {
            return;
        }
        let title = self.t("proj.revoke_confirm_title").to_string();
        let desc = self.t("proj.revoke_hint").to_string();
        let ok_text = self.t("proj.revoke_ok").to_string();
        let cancel_text = self.t("common.cancel").to_string();
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let entity = entity.clone();
            alert
                .title(title.clone())
                .description(desc.clone())
                .show_cancel(true)
                .ok_text(ok_text.clone())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(cancel_text.clone())
                .on_ok(move |_, _, cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            this.revoke_native_for_selected_project(cx);
                        });
                    }
                    true
                })
        });
    }

    /// Register the Agent Control hook (SessionStart briefing + PreToolUse gate) into the selected
    /// project's chosen harness settings. The command runs the bundled hook script with Node; this is
    /// what turns a declared Hard rule into an Enforced one. Other config and user hooks are preserved.
    pub fn register_hooks_for_selected_project(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| p.path.clone())
        else {
            self.log(self.t("log.no_project"));
            return;
        };
        let Some(command) = agent_control_core::hooks::default_hook_command() else {
            self.log(self.t("log.hooks_no_script"));
            self.notify(window, cx, self.t("proj.hooks_no_script_toast"));
            return;
        };
        match self
            .store
            .register_hooks(&path, &self.hook_targets, &command)
        {
            Ok(report) => {
                let targets = report
                    .targets
                    .iter()
                    .map(|t| format!("{} ({})", t.path.display(), t.action))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.log(self.tr("log.hooks_register_done", &[&targets]));
                let msg = self.tr(
                    "proj.hooks_register_toast",
                    &[&report.targets.len().to_string()],
                );
                self.notify(window, cx, &msg);
            }
            Err(e) => {
                self.log(self.tr("log.hooks_register_failed", &[&e.to_string()]));
                let msg = self.tr("proj.hooks_failed_toast", &[&e.to_string()]);
                self.notify(window, cx, &msg);
            }
        }
        self.refresh_hook_status();
        cx.notify();
    }

    /// Revoke the Agent Control hook from the selected project's chosen harness settings, preserving
    /// other config and user hooks. The explicit undo of `register_hooks_for_selected_project`.
    pub fn revoke_hooks_for_selected_project(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| p.path.clone())
        else {
            self.log(self.t("log.no_project"));
            return;
        };
        match self.store.revoke_hooks(&path, &self.hook_targets) {
            Ok(report) => {
                let targets = report
                    .targets
                    .iter()
                    .map(|t| format!("{} ({})", t.path.display(), t.action))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.log(self.tr("log.hooks_revoke_done", &[&targets]));
                let msg = self.tr(
                    "proj.hooks_revoke_toast",
                    &[&report.targets.len().to_string()],
                );
                self.notify(window, cx, &msg);
            }
            Err(e) => {
                self.log(self.tr("log.hooks_revoke_failed", &[&e.to_string()]));
                let msg = self.tr("proj.hooks_failed_toast", &[&e.to_string()]);
                self.notify(window, cx, &msg);
            }
        }
        self.refresh_hook_status();
        cx.notify();
    }

    /// Toggle one harness in the hook selection.
    pub fn toggle_hook_target(
        &mut self,
        target: HookTarget,
        checked: bool,
        cx: &mut Context<Self>,
    ) {
        if checked {
            if !self.hook_targets.contains(&target) {
                self.hook_targets.push(target);
            }
        } else {
            self.hook_targets.retain(|t| *t != target);
        }
        cx.notify();
    }

    /// Confirm before revoking hooks: the action can delete settings files that held only our hook.
    pub fn confirm_revoke_hooks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_project.is_none() {
            return;
        }
        let title = self.t("proj.hooks_revoke_confirm_title").to_string();
        let desc = self.t("proj.hooks_revoke_hint").to_string();
        let ok_text = self.t("proj.hooks_revoke_ok").to_string();
        let cancel_text = self.t("common.cancel").to_string();
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let entity = entity.clone();
            alert
                .title(title.clone())
                .description(desc.clone())
                .show_cancel(true)
                .ok_text(ok_text.clone())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(cancel_text.clone())
                .on_ok(move |_, window, cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            this.revoke_hooks_for_selected_project(window, cx);
                        });
                    }
                    true
                })
        });
    }

    pub fn confirm_remove_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_project.is_none() {
            return;
        }
        let title = self.t("proj.remove_confirm_title").to_string();
        let desc = self.t("proj.remove_hint").to_string();
        let ok_text = self.t("proj.remove_ok").to_string();
        let cancel_text = self.t("common.cancel").to_string();
        let entity = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let entity = entity.clone();
            alert
                .title(title.clone())
                .description(desc.clone())
                .show_cancel(true)
                .ok_text(ok_text.clone())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(cancel_text.clone())
                .on_ok(move |_, _, cx| {
                    if let Some(entity) = entity.upgrade() {
                        entity.update(cx, |this, cx| {
                            this.remove_selected_project(cx);
                            cx.notify();
                        });
                    }
                    true
                })
        });
    }

    pub fn remove_selected_project(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.selected_project else {
            return false;
        };
        let name = self
            .projects
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        match self.store.remove_project(id) {
            Ok(true) => {
                if self.current_project == Some(id) {
                    self.current_project = None;
                }
                self.selected_project = None;
                self.log(self.tr("log.project_removed", &[&name]));
                self.refresh_data(cx);
                true
            }
            Ok(false) => {
                self.log(self.t("log.not_found"));
                false
            }
            Err(e) => {
                self.log(self.tr("log.project_remove_failed", &[&e.to_string()]));
                false
            }
        }
    }

    fn activate_project(&mut self, id: Uuid, tab: Scope, nav: Nav, cx: &mut Context<Self>) {
        self.current_project = Some(id);
        self.selected_project = Some(id);
        self.rules_tab = tab;
        self.nav = nav;
        let _ = self.store.touch_project(id);
        self.ensure_project_scaffold(id);
        self.refresh_data(cx);
    }

    pub fn handle_tree_click(
        &mut self,
        node_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(rest) = node_id.strip_prefix("project:") {
            if let Ok(id) = Uuid::parse_str(rest) {
                self.select_project(id, window, cx);
            }
        } else if let Some(rest) = node_id.strip_prefix("contracts:") {
            if let Ok(id) = Uuid::parse_str(rest) {
                self.activate_project(id, Scope::Project, Nav::Rules, cx);
            }
        } else if let Some(rest) = node_id.strip_prefix("sessions:") {
            if let Ok(id) = Uuid::parse_str(rest) {
                let tab = self.rules_tab;
                self.activate_project(id, tab, Nav::Sessions, cx);
            }
        } else if let Some(rest) = node_id.strip_prefix("conflicts:") {
            if let Ok(id) = Uuid::parse_str(rest) {
                let tab = self.rules_tab;
                self.activate_project(id, tab, Nav::Dashboard, cx);
                self.resolve_conflicts_for_project(cx);
            }
        } else if let Some(rest) = node_id.strip_prefix("rule:")
            && let Some((pid, rule_id)) = rest.split_once(':')
            && let Ok(id) = Uuid::parse_str(pid)
        {
            self.activate_project(id, Scope::Project, Nav::Rules, cx);
            if let Some(index) = self
                .rules
                .iter()
                .position(|r| r.id == rule_id && r.scope == Scope::Project)
            {
                self.open_rule_editor_dialog(Some(index), window, cx);
            }
        }
        cx.notify();
    }

    /// Enable/disable a project contract straight from the project tree context menu.
    pub fn toggle_rule_from_tree(
        &mut self,
        pid: Uuid,
        rule_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_project(pid, Scope::Project, Nav::Rules, cx);
        if let Some(index) = self
            .rules
            .iter()
            .position(|r| r.id == rule_id && r.scope == Scope::Project)
        {
            let enabled = self.rules[index].enabled;
            if self.set_rule_enabled_at(index, !enabled, cx) {
                let msg = self.t(if !enabled {
                    "rules.enabled_toast"
                } else {
                    "rules.disabled_toast"
                });
                self.notify(window, cx, msg);
            }
        }
        cx.notify();
    }

    pub fn notify(&mut self, window: &mut Window, cx: &mut App, message: &str) {
        window.push_notification(Notification::info(message), cx);
    }

    /// Toggle a named disclosure section (project export/help, hooks help, session verification).
    pub fn toggle_section(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.open_sections.remove(key) {
            self.open_sections.insert(key.to_string());
        }
        cx.notify();
    }

    /// Run a command dispatched from the project tree's right-click context menu. Project-scoped
    /// commands select the target first so the existing detail-pane actions operate on it.
    pub fn run_tree_action(
        &mut self,
        action: TreeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            TreeAction::SetCurrent(id) => self.set_current_project(id, cx),
            TreeAction::Rename(id) => {
                self.select_project(id, window, cx);
                self.open_rename_dialog(window, cx);
            }
            TreeAction::Export(id) => {
                self.select_project(id, window, cx);
                self.export_native_for_selected_project(cx);
            }
            TreeAction::Revoke(id) => {
                self.select_project(id, window, cx);
                self.confirm_revoke_native(window, cx);
            }
            TreeAction::RegisterHooks(id) => {
                self.select_project(id, window, cx);
                self.register_hooks_for_selected_project(window, cx);
            }
            TreeAction::RevokeHooks(id) => {
                self.select_project(id, window, cx);
                self.confirm_revoke_hooks(window, cx);
            }
            TreeAction::Reveal(id) => {
                self.select_project(id, window, cx);
                self.reveal_project_contracts(cx);
            }
            TreeAction::Remove(id) => {
                self.select_project(id, window, cx);
                self.confirm_remove_project(window, cx);
            }
            TreeAction::Open(node) => self.handle_tree_click(&node, window, cx),
            TreeAction::ToggleRule(id, rule_id) => {
                self.toggle_rule_from_tree(id, &rule_id, window, cx)
            }
        }
        cx.notify();
    }
}

/// A command dispatched from the project tree's right-click context menu.
#[derive(Clone)]
pub enum TreeAction {
    SetCurrent(Uuid),
    Rename(Uuid),
    Export(Uuid),
    Revoke(Uuid),
    RegisterHooks(Uuid),
    RevokeHooks(Uuid),
    Reveal(Uuid),
    Remove(Uuid),
    Open(String),
    ToggleRule(Uuid, String),
}

impl Render for Console {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.nav {
            Nav::Dashboard => self.render_dashboard(cx).into_any_element(),
            Nav::Projects => self.render_projects(cx).into_any_element(),
            Nav::Preferences => self.render_rules(cx).into_any_element(),
            Nav::Rules => self.render_rules(cx).into_any_element(),
            Nav::Environment => self.render_rules(cx).into_any_element(),
            Nav::Suggestions => self.render_suggestions(cx).into_any_element(),
            Nav::Provenance => self.render_provenance(cx).into_any_element(),
            Nav::Evidence => self.render_evidence(cx).into_any_element(),
            Nav::Sessions => self.render_sessions(cx).into_any_element(),
            Nav::Settings => self.render_settings(cx).into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .px_2()
                        .child(
                            h_flex()
                                .size(px(20.))
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().primary)
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::SquareTerminal)
                                        .size_3()
                                        .text_color(cx.theme().primary_foreground),
                                ),
                        )
                        .child(div().font_semibold().text_sm().child("Agent Control"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("— {}", self.nav_title())),
                        ),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.render_nav(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .child(self.render_header(cx))
                            .child(content),
                    ),
            )
            .child(self.render_status_bar(cx))
            .into_any_element()
    }
}

impl Console {
    fn nav_title(&self) -> &'static str {
        match self.nav {
            Nav::Dashboard => self.t("header.dashboard"),
            Nav::Projects => self.t("header.projects"),
            Nav::Preferences => self.t("header.preferences"),
            Nav::Rules => self.t("header.rules"),
            Nav::Environment => self.t("header.environment"),
            Nav::Suggestions => self.t("header.suggestions"),
            Nav::Provenance => self.t("header.provenance"),
            Nav::Evidence => self.t("header.evidence"),
            Nav::Sessions => self.t("header.sessions"),
            Nav::Settings => self.t("header.settings"),
        }
    }

    fn nav_subtitle(&self) -> &'static str {
        match self.nav {
            Nav::Dashboard => self.t("desc.dashboard"),
            Nav::Projects => self.t("desc.projects"),
            Nav::Preferences => self.t("desc.preferences"),
            Nav::Rules => self.t("desc.rules"),
            Nav::Environment => self.t("desc.environment"),
            Nav::Suggestions => self.t("desc.suggestions"),
            Nav::Provenance => self.t("desc.provenance"),
            Nav::Evidence => self.t("desc.evidence"),
            Nav::Sessions => self.t("desc.sessions"),
            Nav::Settings => self.t("desc.settings"),
        }
    }

    fn nav_item(
        &self,
        nav: Nav,
        key: &'static str,
        id: &'static str,
        icon: IconName,
        badge: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self.t(key);
        let primary = cx.theme().primary;
        Button::new(id)
            .ghost()
            .selected(self.nav == nav)
            .accessibility_label(label)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(icon).size_4())
                    .child(div().flex_1().min_w_0().text_sm().child(label))
                    .when(badge > 0, |this| {
                        this.child(Badge::new().count(badge).color(primary))
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.nav != nav {
                    this.clear_rule_editor(window, cx);
                    window.close_dialog(cx);
                }
                this.nav = nav;
                this.refresh_data(cx);
                cx.notify();
            }))
    }

    fn nav_group_label(&self, key: &'static str, cx: &App) -> impl IntoElement {
        div()
            .px_2()
            .pt_3()
            .pb_1()
            .text_xs()
            .font_medium()
            .text_color(cx.theme().muted_foreground)
            .child(self.t(key))
    }

    fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let pending = self.pending_suggestions;
        v_flex()
            .w(px(220.))
            .h_full()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .px_2()
            .py_2()
            .gap_0p5()
            .child(self.nav_group_label("nav.group.workspace", cx))
            .child(self.nav_item(
                Nav::Dashboard,
                "nav.dashboard",
                "nav-dashboard",
                IconName::LayoutDashboard,
                0,
                cx,
            ))
            .child(self.nav_item(
                Nav::Projects,
                "nav.projects",
                "nav-projects",
                IconName::Folder,
                0,
                cx,
            ))
            .child(self.nav_item(
                Nav::Preferences,
                "nav.preferences",
                "nav-preferences",
                IconName::User,
                0,
                cx,
            ))
            .child(self.nav_item(
                Nav::Rules,
                "nav.rules",
                "nav-rules",
                IconName::BookOpen,
                0,
                cx,
            ))
            .child(self.nav_item(
                Nav::Environment,
                "nav.environment",
                "nav-environment",
                IconName::Cpu,
                0,
                cx,
            ))
            .child(self.nav_item(
                Nav::Suggestions,
                "nav.suggestions",
                "nav-suggestions",
                IconName::Inbox,
                pending,
                cx,
            ))
            .child(self.nav_item(
                Nav::Sessions,
                "nav.sessions",
                "nav-sessions",
                IconName::SquareTerminal,
                0,
                cx,
            ))
            .child(self.nav_group_label("nav.group.verify", cx))
            .child(self.nav_item(
                Nav::Provenance,
                "nav.provenance",
                "nav-provenance",
                IconName::Network,
                0,
                cx,
            ))
            .child(self.nav_item(
                Nav::Evidence,
                "nav.evidence",
                "nav-evidence",
                IconName::FileText,
                0,
                cx,
            ))
            .child(self.nav_group_label("nav.group.system", cx))
            .child(self.nav_item(
                Nav::Settings,
                "nav.settings",
                "nav-settings",
                IconName::Settings,
                0,
                cx,
            ))
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .px_2()
                    .py_2()
                    .border_t_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(ui::dot(if self.mcp_running {
                        cx.theme().success
                    } else {
                        cx.theme().danger
                    }))
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.tr(
                                "footer.mcp",
                                &[self.t(if self.mcp_running {
                                    "mcp.running"
                                } else {
                                    "mcp.stopped"
                                })],
                            )),
                    )
                    .child(
                        Button::new("mcp-sidebar-toggle")
                            .icon(Icon::new(if self.mcp_running {
                                IconName::Square
                            } else {
                                IconName::Play
                            }))
                            .ghost()
                            .small()
                            .accessibility_label(self.t(if self.mcp_running {
                                "set.mcp.stop"
                            } else {
                                "set.mcp.start"
                            }))
                            .tooltip(self.t(if self.mcp_running {
                                "set.mcp.stop"
                            } else {
                                "set.mcp.start"
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_mcp_http(cx);
                            })),
                    ),
            )
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.nav_title();
        let subtitle = self.nav_subtitle();
        let project_label = self
            .current_project()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| self.t("header.no_project").to_string());
        let manage_label = self.t("header.manage_projects").to_string();
        let refresh_label = self.t("common.refresh");
        let projects = self.projects.clone();
        let current = self.current_project;
        let entity = cx.entity().downgrade();

        let project_switcher = DropdownButton::new("project-switcher")
            .button(
                Button::new("project-switcher-button")
                    .icon(Icon::new(IconName::Folder))
                    .label(project_label)
                    .ghost()
                    .small(),
            )
            .dropdown_menu(move |mut menu, _window, _cx| {
                for project in &projects {
                    let id = project.id;
                    let entity = entity.clone();
                    menu = menu.item(
                        PopupMenuItem::new(project.name.clone())
                            .checked(current == Some(id))
                            .on_click(move |_, _, cx| {
                                if let Some(entity) = entity.upgrade() {
                                    entity.update(cx, |this, cx| {
                                        this.set_current_project(id, cx);
                                    });
                                }
                            }),
                    );
                }
                let entity = entity.clone();
                menu.separator().item(
                    PopupMenuItem::new(manage_label.clone())
                        .icon(Icon::new(IconName::Settings2))
                        .on_click(move |_, _, cx| {
                            if let Some(entity) = entity.upgrade() {
                                entity.update(cx, |this, cx| {
                                    this.nav = Nav::Projects;
                                    this.refresh_data(cx);
                                    cx.notify();
                                });
                            }
                        }),
                )
            });

        h_flex()
            .w_full()
            .h(px(56.))
            .px_4()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .gap_0p5()
                    .child(div().text_lg().font_bold().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(project_switcher)
                    .child(
                        Button::new("refresh")
                            .icon(Icon::new(IconName::RefreshCw))
                            .ghost()
                            .small()
                            .accessibility_label(refresh_label)
                            .tooltip(refresh_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_data(cx);
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let mcp_color = if self.mcp_running {
            cx.theme().success
        } else {
            cx.theme().danger
        };
        let project_label = match self.current_project() {
            Some(p) => self.tr("header.project", &[&p.name]),
            None => self.t("header.no_project").to_string(),
        };
        let locale_label = self.t(match self.locale {
            Locale::En => "lang.en",
            Locale::Zh => "lang.zh",
        });
        let version = self.tr("app.version", &[env!("CARGO_PKG_VERSION")]);

        h_flex()
            .w_full()
            .h(px(26.))
            .px_3()
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .bg(cx.theme().status_bar)
            .text_xs()
            .text_color(muted)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(ui::dot(mcp_color))
                    .child(div().child("MCP"))
                    .child(ui::mono(cx, format!("http://{}/mcp", self.mcp_bind))),
            )
            .child(div().flex_1())
            .child(div().child(project_label))
            .child(div().child(locale_label))
            .child(div().child(version))
    }
}

/// True when something is already accepting TCP connections on `bind` (a running MCP daemon).
fn daemon_listening(bind: &str) -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    match bind.parse::<SocketAddr>() {
        Ok(addr) => TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok(),
        Err(_) => false,
    }
}

/// Path to the MCP server binary shipped next to the console executable, if present.
fn sibling_mcp_exe() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "agent-control-mcp.exe"
    } else {
        "agent-control-mcp"
    };
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(name);
    path.is_file().then_some(path)
}

/// Ask a background daemon to shut down via its loopback-only `POST /shutdown` route. Best-effort:
/// the daemon may have been started by another console, so we signal it rather than kill a PID.
fn request_daemon_shutdown(bind: &str) {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    let Ok(mut stream) = TcpStream::connect(bind) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let req = format!(
        "POST /shutdown HTTP/1.1\r\nHost: {bind}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return;
    }
    let _ = stream.flush();
    // Read the reply before closing: on Windows, dropping a socket with unread data sends RST,
    // which can abort the request before the daemon processes it (the daemon would keep running).
    let mut buf = [0u8; 256];
    let _ = stream.read(&mut buf);
}

/// Render work items as one `status: title` line each, for the editor textarea.
fn format_work_items(items: &[WorkItem]) -> String {
    items
        .iter()
        .map(|item| format!("{}: {}", item.status.label(), item.title))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse the editor textarea back into work items. Each non-empty line is `status: title`; an
/// unknown or missing prefix defaults to `todo`, so free-form notes still become resumable items.
fn parse_work_items(text: &str) -> Vec<WorkItem> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (status, title) = match line.split_once(':') {
                Some((prefix, rest)) => {
                    let status = match prefix.trim().to_ascii_lowercase().as_str() {
                        "done" => WorkItemStatus::Done,
                        "doing" => WorkItemStatus::Doing,
                        "todo" => WorkItemStatus::Todo,
                        _ => WorkItemStatus::Todo,
                    };
                    (status, rest.trim())
                }
                None => (WorkItemStatus::Todo, line),
            };
            WorkItem {
                title: title.to_string(),
                status,
            }
        })
        .filter(|item| !item.title.is_empty())
        .collect()
}
