use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_control_acp::{
    HarnessChannels, HarnessCommand, HarnessConfig, HarnessEvent, HarnessManager, McpEndpoint,
    PermissionDecision, PermissionOptionInfo, PermissionPolicy,
};
use agent_control_core::Store;
use agent_control_core::model::{
    AgentRegistration, Conflict, Enforcement, EvidenceEntry, Project, ProvenanceRecord, Rule,
    RuleSuggestion, Scope, Session, SuggestionEdits, SuggestionStatus, VerificationReport,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants, DropdownButton};
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tree::{TreeItem, TreeState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable, Sizable, StyledExt, TitleBar, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use tokio::sync::{mpsc, oneshot};
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
    Suggestions,
    Provenance,
    Evidence,
    Sessions,
    Settings,
}

pub struct ActiveHarness {
    pub agent_id: String,
    pub control_session_id: Option<Uuid>,
    pub command_tx: mpsc::Sender<HarnessCommand>,
    pub event_rx: mpsc::Receiver<HarnessEvent>,
}

pub struct PendingPermission {
    pub tool_title: String,
    pub options: Vec<PermissionOptionInfo>,
    pub responder: Option<oneshot::Sender<PermissionDecision>>,
}

pub struct LogEntry {
    pub time: String,
    pub text: String,
}

pub struct Console {
    pub store: Arc<Store>,
    pub manager: Arc<HarnessManager>,
    pub nav: Nav,
    pub locale: Locale,

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
    pub show_onboarding: bool,
    pub export_stale: bool,

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
    pub active_harness: Option<ActiveHarness>,
    pub pending_permission: Option<PendingPermission>,

    pub mcp_running: bool,
    pub mcp_external: bool,
    pub mcp_shutdown: Option<CancellationToken>,
    pub mcp_bind: String,

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
    pub prompt_input: Entity<TextareaState>,
    pub handoff_target_input: Entity<InputState>,

    poll_task: Option<Task<()>>,
}

impl Console {
    pub fn new(
        store: Arc<Store>,
        manager: Arc<HarnessManager>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let locale = i18n::load(&store.layout.root);
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
        let prompt_input = cx.new(|cx| TextareaState::new(window, cx).default_value(""));
        let handoff_target_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.handoff")));
        let sugg_rule_id_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.rule_id")));
        let sugg_title_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.rule_title")));
        let sugg_content_input = cx.new(|cx| TextareaState::new(window, cx).default_value(""));
        let sugg_note_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(i18n::t(locale, "ph.sugg_note")));

        let mut console = Self {
            store,
            manager,
            nav: Nav::Dashboard,
            locale,
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
            show_onboarding: false,
            export_stale: false,
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
            active_harness: None,
            pending_permission: None,
            mcp_running: false,
            mcp_external: false,
            mcp_shutdown: None,
            mcp_bind: "127.0.0.1:9870".into(),
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
            prompt_input,
            handoff_target_input,
            poll_task: None,
        };
        console.refresh_data(cx);
        if let Some(id) = console.current_project {
            console.ensure_project_scaffold(id);
        }
        console.start_mcp_http();
        console.start_polling(cx);
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
        i18n::save(&self.store.layout.root, locale);
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
        self.handoff_target_input.update(cx, |s, cx| {
            s.set_placeholder(i18n::t(locale, "ph.handoff"), window, cx)
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
    }

    /// Recompute whether the selected project's native export is stale relative to its current
    /// enabled rules. Cached here (refresh/select/export), never at render time. Project-only:
    /// the console export action does not write user-global files, so include_user=false.
    fn refresh_export_drift(&mut self) {
        self.export_stale = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| {
                self.store
                    .export_drift(&p.path, false)
                    .map(|r| r.stale)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
    }

    fn rebuild_project_tree(&mut self, cx: &mut App) {
        let mut roots = Vec::new();
        let mut stats = std::collections::HashMap::new();
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

        // 3. Fallback (dev run without a sibling binary): host in-process on the manager runtime.
        let shutdown = CancellationToken::new();
        let store = self.store.clone();
        let sd = shutdown.clone();
        self.manager.spawn(async move {
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

    fn start_polling(&mut self, cx: &mut Context<Self>) {
        self.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let _ = this.update(cx, |console, cx| {
                    let mut changed = false;
                    let events: Vec<HarnessEvent> = match &mut console.active_harness {
                        Some(harness) => {
                            let mut events = Vec::new();
                            while let Ok(event) = harness.event_rx.try_recv() {
                                events.push(event);
                            }
                            events
                        }
                        None => Vec::new(),
                    };
                    for event in events {
                        changed = true;
                        match event {
                            HarnessEvent::SessionCreated {
                                control_session_id,
                                snapshot_hash,
                                rule_count,
                                ..
                            } => {
                                if let Some(h) = &mut console.active_harness {
                                    h.control_session_id = Some(control_session_id);
                                }
                                console.log(console.tr(
                                    "log.session_evt",
                                    &[
                                        &control_session_id.to_string(),
                                        &rule_count.to_string(),
                                        &format!("{snapshot_hash:.12}"),
                                    ],
                                ));
                                console.refresh_data(cx);
                            }
                            HarnessEvent::PermissionRequest {
                                tool_title,
                                options,
                                responder,
                                ..
                            } => {
                                console.log(console.tr("log.perm_requested", &[&tool_title]));
                                console.pending_permission = Some(PendingPermission {
                                    tool_title,
                                    options,
                                    responder: Some(responder),
                                });
                            }
                            HarnessEvent::Disconnected => {
                                console.log(console.t("log.disconnected"));
                            }
                            other => {
                                console.log(other.summary());
                                if matches!(other, HarnessEvent::EvidenceRecorded { .. }) {
                                    console.refresh_evidence();
                                }
                            }
                        }
                    }
                    if changed {
                        cx.notify();
                    }
                });
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
            }
        }));
    }

    pub fn decide_permission(&mut self, decision: PermissionDecision) {
        if let Some(mut pending) = self.pending_permission.take() {
            if let Some(responder) = pending.responder.take() {
                let _ = responder.send(decision);
            }
            self.log(self.tr("log.perm_decision", &[&format!("{decision:?}")]));
        }
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
        let project_path = if scope == Scope::Project {
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
        rule.enforcement = self.rule_enforcement;
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
        self.rules_tab = if rule.scope == Scope::Project {
            Scope::Project
        } else {
            Scope::User
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
        let project_path = if rule.scope == Scope::Project {
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

    pub fn set_selected_rule_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) -> bool {
        let Some(index) = self.selected_rule else {
            return false;
        };
        let Some(rule) = self.rules.get(index) else {
            return false;
        };
        let project_path = if rule.scope == Scope::Project {
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
                    .t(if rule.scope == Scope::User {
                        "sugg.approved_toast_user"
                    } else {
                        "sugg.approved_toast"
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
        let ok_text = self.t("sugg.reject").to_string();
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

    pub fn create_session_from_inputs(&mut self, cx: &mut Context<Self>) -> bool {
        let task = self.session_task_input.read(cx).value().trim().to_string();
        let Some(project) = self.current_project_path() else {
            self.log(self.t("log.no_project"));
            return false;
        };
        if task.is_empty() {
            self.log(self.t("log.task_required"));
            return false;
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
            Ok(session) => {
                self.log(self.tr(
                    "log.session_created",
                    &[
                        &session.id.to_string(),
                        &session.constraint_snapshot.rules.len().to_string(),
                    ],
                ));
                self.selected_session = Some(session.id);
                self.refresh_data(cx);
                true
            }
            Err(e) => {
                self.log(self.tr("log.session_failed", &[&e.to_string()]));
                false
            }
        }
    }

    pub fn start_harness(&mut self, cx: &mut Context<Self>) {
        if self.active_harness.is_some() {
            self.log(self.t("log.already_running"));
            return;
        }
        let Some(registration) = self.agents.get(self.selected_agent).cloned() else {
            self.log(self.t("log.no_agent"));
            return;
        };
        let Some(project) = self.current_project_path() else {
            self.log(self.t("log.no_project"));
            return;
        };
        let task = self.session_task_input.read(cx).value().trim().to_string();
        if task.is_empty() {
            self.log(self.t("log.set_first"));
            return;
        }
        let config = HarnessConfig {
            registration,
            project_path: project,
            task,
            task_tags: vec![],
            mcp_endpoint: Some(McpEndpoint::Http {
                url: format!("http://{}/mcp", self.mcp_bind),
            }),
            permission_policy: PermissionPolicy::Ask,
        };
        match self.manager.start(config) {
            Ok(HarnessChannels {
                command_tx,
                event_rx,
                ..
            }) => {
                self.active_harness = Some(ActiveHarness {
                    agent_id: self.agents[self.selected_agent].id.clone(),
                    control_session_id: None,
                    command_tx,
                    event_rx,
                });
                self.log(self.t("log.harness_starting"));
            }
            Err(e) => self.log(self.tr("log.harness_failed", &[&e.to_string()])),
        }
    }

    pub fn send_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.prompt_input.read(cx).value();
        if text.trim().is_empty() {
            return;
        }
        if let Some(harness) = &self.active_harness {
            match harness.command_tx.try_send(HarnessCommand::Prompt {
                text: text.to_string(),
            }) {
                Ok(()) => {
                    self.log(self.tr(
                        "log.prompt_sent",
                        &[&text.chars().take(60).collect::<String>()],
                    ));
                    self.prompt_input
                        .update(cx, |s, cx| s.set_value("", window, cx));
                }
                Err(e) => self.log(self.tr("log.send_failed", &[&e.to_string()])),
            }
        }
    }

    pub fn cancel_prompt(&mut self) {
        if let Some(harness) = &self.active_harness {
            let _ = harness.command_tx.try_send(HarnessCommand::Cancel);
            self.log(self.t("log.cancel_sent"));
        }
    }

    pub fn stop_harness(&mut self, cx: &mut Context<Self>) {
        if let Some(harness) = self.active_harness.take() {
            let _ = harness.command_tx.try_send(HarnessCommand::Shutdown);
            self.log(self.t("log.harness_stopping"));
            self.refresh_data(cx);
        }
    }

    pub fn export_handoff(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(session_id) = self.selected_session else {
            self.log(self.t("log.select_session"));
            return false;
        };
        let target = self
            .handoff_target_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if target.is_empty() {
            self.log(self.t("log.enter_target"));
            return false;
        }
        match self
            .store
            .export_handoff(session_id, &target, serde_json::json!({}))
        {
            Ok(bundle) => {
                self.log(self.tr(
                    "log.handoff_exported",
                    &[
                        &bundle.from_agent,
                        &target,
                        &format!("{:.12}", bundle.bundle_hash),
                    ],
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

    /// Compile the selected project's enabled contracts into native harness entry points
    /// (AGENTS.md / CLAUDE.md / .cursor/rules) as a managed block. Project-only: personal
    /// preferences are not written to the user-global Claude memory from this action.
    pub fn export_native_for_selected_project(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|p| p.path.clone())
        else {
            self.log(self.t("log.no_project"));
            return;
        };
        match self.store.export_native(&path, false) {
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

    pub fn confirm_remove_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_project.is_none() {
            return;
        }
        let title = self.t("proj.remove_confirm_title").to_string();
        let desc = self.t("proj.remove_hint").to_string();
        let ok_text = self.t("proj.remove").to_string();
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
                self.load_rule_into_editor(index, window, cx);
            }
        }
        cx.notify();
    }

    pub fn notify(&mut self, window: &mut Window, cx: &mut App, message: &str) {
        window.push_notification(Notification::info(message), cx);
    }
}

impl Render for Console {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.nav {
            Nav::Dashboard => self.render_dashboard(cx).into_any_element(),
            Nav::Projects => self.render_projects(cx).into_any_element(),
            Nav::Preferences => self.render_rules(cx).into_any_element(),
            Nav::Rules => self.render_rules(cx).into_any_element(),
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
            .on_click(cx.listener(move |this, _, _, cx| {
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
        let harness_label = match &self.active_harness {
            Some(h) => self.tr("dash.harness", &[&h.agent_id]),
            None => self.t("dash.no_harness").to_string(),
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
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(ui::dot(if self.active_harness.is_some() {
                        cx.theme().primary
                    } else {
                        cx.theme().muted_foreground
                    }))
                    .child(div().child(harness_label)),
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
    use std::io::Write;
    use std::net::TcpStream;
    use std::time::Duration;
    let Ok(mut stream) = TcpStream::connect(bind) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let req = format!(
        "POST /shutdown HTTP/1.1\r\nHost: {bind}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let _ = stream.write_all(req.as_bytes());
    let _ = stream.flush();
}
