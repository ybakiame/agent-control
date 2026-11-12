use std::path::PathBuf;
use std::sync::Arc;

use agent_control_core::Store;
use agent_control_core::model::{
    Decision, EvidenceKind, QueryContext, RuleKind, Scope, SuggestionStatus,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, ErrorData, GetPromptRequestParams, GetPromptResult,
    Implementation, ListPromptsResult, ListResourcesResult, PaginatedRequestParams, Prompt,
    PromptArgument, PromptMessage, ReadResourceRequestParams, ReadResourceResult, Resource,
    ResourceContents, Role, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Clone)]
pub struct ControlServer {
    tool_router: ToolRouter<Self>,
    pub store: Arc<Store>,
}

impl ControlServer {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            store,
        }
    }
}

fn json_result(value: &serde_json::Value) -> Result<CallToolResult, ErrorData> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(value.clone());
    Ok(result)
}

fn tool_err(msg: impl std::fmt::Display) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![ContentBlock::text(
        msg.to_string(),
    )]))
}

fn parse_uuid(s: &str) -> Result<Uuid, ErrorData> {
    Uuid::parse_str(s).map_err(|e| ErrorData::invalid_params(format!("invalid UUID: {e}"), None))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RulesQuery {
    /// Absolute path of the project to resolve project contracts for
    pub project_path: String,
    /// Identifier of the calling agent/harness (e.g. "claude-code", "gemini-cli")
    pub agent_id: String,
    /// File paths relevant to the current task (relative to project root)
    #[serde(default)]
    pub file_paths: Vec<String>,
    /// Programming languages in scope
    #[serde(default)]
    pub languages: Vec<String>,
    /// Task tags for task-scoped rule matching
    #[serde(default)]
    pub task_tags: Vec<String>,
}

impl RulesQuery {
    fn to_context(&self) -> QueryContext {
        QueryContext {
            project_path: PathBuf::from(&self.project_path),
            agent_id: self.agent_id.clone(),
            file_paths: self.file_paths.clone(),
            languages: self.languages.clone(),
            task_tags: self.task_tags.clone(),
            session_id: None,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExplainQuery {
    /// The rule id to explain
    pub rule_id: String,
    /// Absolute project path for context
    pub project_path: String,
    /// Identifier of the calling agent/harness
    pub agent_id: String,
    #[serde(default)]
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub task_tags: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateSessionRequest {
    /// What the session is trying to accomplish
    pub task: String,
    /// Absolute project path
    pub project_path: String,
    /// Identifier of the harness running the session
    pub agent_id: String,
    #[serde(default)]
    pub task_tags: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordEvidenceRequest {
    /// Session UUID
    pub session_id: String,
    /// Evidence kind: rule_acknowledgment, tool_call, file_change, test_result, handoff_out, handoff_in, constraint_violation, session_start, session_end
    pub kind: String,
    /// Harness recording the evidence
    pub agent_id: String,
    /// Arbitrary JSON payload describing the event
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AcknowledgeRequest {
    /// Session UUID
    pub session_id: String,
    /// Harness acknowledging the constraints
    pub agent_id: String,
    /// Rule ids the harness commits to follow
    pub rule_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionRequest {
    /// Session UUID
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportHandoffRequest {
    /// Session UUID to hand off
    pub session_id: String,
    /// Target harness id
    pub to_agent: String,
    /// Context summary to carry over (conversation state, files touched, decisions)
    #[serde(default)]
    pub context: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportHandoffRequest {
    /// The full handoff bundle JSON produced by export_handoff
    pub bundle: serde_json::Value,
    /// Harness resuming the session
    pub new_agent: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRulesRequest {
    /// Optional absolute project path to include project contracts
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveRuleRequest {
    /// Stable unique rule id (kebab-case recommended). Reusing an id updates the existing rule
    /// and bumps its version instead of creating a duplicate.
    pub id: String,
    /// Short human-readable rule title
    pub title: String,
    /// Rule body (Markdown). State the contract/constraint and how to satisfy it.
    pub content: String,
    /// Identifier of the calling agent/harness; recorded as the rule's author for provenance
    pub agent_id: String,
    /// Rule kind: preference | contract | constraint. Defaults to contract.
    #[serde(default)]
    pub kind: Option<String>,
    /// Scope: user | project. Defaults to project. Session scope cannot be saved as a file.
    #[serde(default)]
    pub scope: Option<String>,
    /// Absolute project path; required when scope is project
    #[serde(default)]
    pub project_path: Option<String>,
    /// Optional topic to group related rules
    #[serde(default)]
    pub topic: Option<String>,
    /// Optional tags
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetRuleEnabledRequest {
    /// Stable id of the rule to enable or disable
    pub id: String,
    /// true = the rule binds sessions; false = it is excluded from resolution (Decision::Disabled)
    pub enabled: bool,
    /// Scope: user | project. Defaults to project. Session scope cannot be toggled as a file.
    #[serde(default)]
    pub scope: Option<String>,
    /// Absolute project path; required when scope is project
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SuggestRuleRequest {
    /// Absolute path of the project this suggestion targets; required when scope=project,
    /// ignored when scope=user
    #[serde(default)]
    pub project_path: Option<String>,
    /// Scope: user | project. Defaults to project. user = a personal preference that follows the
    /// user across projects (reviewed in the console Preferences/Suggestions flow).
    #[serde(default)]
    pub scope: Option<String>,
    /// Identifier of the calling agent/harness; recorded as the proposer for provenance
    pub agent_id: String,
    /// Proposed stable rule id (kebab-case recommended)
    pub id: String,
    /// Short human-readable rule title
    pub title: String,
    /// Proposed rule body (Markdown). State the contract/constraint and how to satisfy it.
    pub content: String,
    /// Rule kind: preference | contract | constraint. Defaults to contract.
    #[serde(default)]
    pub kind: Option<String>,
    /// Optional topic to group related rules
    #[serde(default)]
    pub topic: Option<String>,
    /// Optional tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Why this rule is proposed (discovery context, evidence); shown to the human reviewer
    #[serde(default)]
    pub rationale: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListSuggestionsRequest {
    /// Optional absolute project path to filter suggestions
    #[serde(default)]
    pub project_path: Option<String>,
    /// Optional status filter: pending | approved | rejected
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportNativeRequest {
    /// Absolute path of the project whose contracts are compiled into native harness entry points
    pub project_path: String,
    /// Also export personal preferences to the user-global ~/.claude/CLAUDE.md. Defaults to false
    /// so a project export never writes outside the project unless explicitly requested.
    #[serde(default)]
    pub include_user: bool,
}

#[tool_router]
impl ControlServer {
    #[tool(
        description = "Resolve which rules apply to the current task. Returns applied rules with provenance (why each rule was selected, its source scope, what superseded what) plus any conflicts. Call this before starting work on a project."
    )]
    async fn get_applicable_rules(
        &self,
        Parameters(req): Parameters<RulesQuery>,
    ) -> Result<CallToolResult, ErrorData> {
        let ctx = req.to_context();
        match self.store.resolve_rules(&ctx) {
            Ok((record, resolution)) => {
                let applied: Vec<_> = resolution
                    .applied
                    .iter()
                    .filter(|m| {
                        m.decision == Decision::Applied || m.decision == Decision::ConflictFlagged
                    })
                    .collect();
                json_result(&serde_json::json!({
                    "provenance_id": record.id,
                    "applied_rules": applied,
                    "conflicts": resolution.conflicts,
                    "instruction": "Follow the applied rules. If conflicts are present, resolve them with the user before proceeding. Record evidence of your work with record_evidence.",
                }))
            }
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Explain why a specific rule exists and how it was decided: its source (user preference vs project contract), scope precedence, priority, superseded alternatives, and conflicts with code or other rules."
    )]
    async fn explain_rule(
        &self,
        Parameters(req): Parameters<ExplainQuery>,
    ) -> Result<CallToolResult, ErrorData> {
        let ctx = QueryContext {
            project_path: PathBuf::from(&req.project_path),
            agent_id: req.agent_id.clone(),
            file_paths: req.file_paths.clone(),
            languages: req.languages.clone(),
            task_tags: req.task_tags.clone(),
            session_id: None,
        };
        match self.store.explain_rule(&req.rule_id, &ctx) {
            Ok(Some(explanation)) => json_result(&serde_json::to_value(explanation).unwrap()),
            Ok(None) => tool_err(format!("rule '{}' not found", req.rule_id)),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Check for conflicts only: rule-vs-rule declarations and rule-vs-code mismatches (e.g. a rule expects tabs but .editorconfig says spaces)."
    )]
    async fn check_conflicts(
        &self,
        Parameters(req): Parameters<RulesQuery>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.store.resolve_rules(&req.to_context()) {
            Ok((_, resolution)) => json_result(&serde_json::json!({
                "conflicts": resolution.conflicts,
                "has_conflicts": !resolution.conflicts.is_empty(),
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Create a work session with a frozen constraint snapshot. The snapshot binds the applicable rules at session start so continuity can be verified across harness handoffs."
    )]
    async fn create_session(
        &self,
        Parameters(req): Parameters<CreateSessionRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.store.create_session(
            &req.task,
            &PathBuf::from(&req.project_path),
            &req.agent_id,
            req.task_tags,
        ) {
            Ok(session) => json_result(&serde_json::json!({
                "session_id": session.id,
                "snapshot_hash": session.constraint_snapshot.snapshot_hash,
                "rules": session.constraint_snapshot.rules,
                "instruction": "Acknowledge these rules with acknowledge_rules before making changes.",
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Append a tamper-evident evidence entry to a session's hash chain. Use for tool calls, file changes (include before/after sha256), test results, and any constraint-relevant event."
    )]
    async fn record_evidence(
        &self,
        Parameters(req): Parameters<RecordEvidenceRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        let kind: EvidenceKind =
            serde_json::from_value(serde_json::Value::String(req.kind.clone())).map_err(|_| {
                ErrorData::invalid_params(format!("unknown evidence kind '{}'", req.kind), None)
            })?;
        match self
            .store
            .record_evidence(session_id, kind, &req.agent_id, req.payload)
        {
            Ok(entry) => json_result(&serde_json::to_value(entry).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Acknowledge the session's constraint rules. The server compares your acknowledgment against the frozen snapshot and reports missing/unexpected rules. Required after session start and after any handoff."
    )]
    async fn acknowledge_rules(
        &self,
        Parameters(req): Parameters<AcknowledgeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        match self
            .store
            .acknowledge_rules(session_id, &req.agent_id, &req.rule_ids)
        {
            Ok(report) => json_result(&serde_json::to_value(report).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Get session context for continuity: task, constraint snapshot, current harness, handoff history, and evidence chain head. Use when resuming work started by another harness."
    )]
    async fn get_session_context(
        &self,
        Parameters(req): Parameters<SessionRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        match self.store.get_session(session_id) {
            Ok(Some(session)) => {
                let head = self
                    .store
                    .evidence_head_hash(session_id)
                    .unwrap_or_default();
                json_result(&serde_json::json!({
                    "session": session,
                    "evidence_head_hash": head,
                }))
            }
            Ok(None) => tool_err(format!("session '{session_id}' not found")),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Export a handoff bundle to transfer this session to another harness. The bundle carries the frozen constraints and evidence chain head so the next harness can be verified against the same rules."
    )]
    async fn export_handoff(
        &self,
        Parameters(req): Parameters<ExportHandoffRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        match self
            .store
            .export_handoff(session_id, &req.to_agent, req.context)
        {
            Ok(bundle) => json_result(&serde_json::to_value(bundle).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Import a handoff bundle and resume the session under a new harness. Verifies bundle integrity and constraint snapshot consistency, then expects acknowledge_rules from the new harness."
    )]
    async fn import_handoff(
        &self,
        Parameters(req): Parameters<ImportHandoffRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let bundle: agent_control_core::model::HandoffBundle =
            serde_json::from_value(req.bundle)
                .map_err(|e| ErrorData::invalid_params(format!("invalid bundle: {e}"), None))?;
        match self.store.import_handoff(&bundle, &req.new_agent) {
            Ok(session) => json_result(&serde_json::json!({
                "session_id": session.id,
                "agent_id": session.agent_id,
                "rules_to_acknowledge": session.constraint_snapshot.rules.iter().map(|r| &r.id).collect::<Vec<_>>(),
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Verify the integrity of a session's evidence hash chain. Recomputes every entry hash and reports any tampering or gaps."
    )]
    async fn verify_evidence(
        &self,
        Parameters(req): Parameters<SessionRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        match self.store.verify_chain(session_id) {
            Ok(report) => json_result(&serde_json::to_value(report).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "List all stored rules (user preferences and, if project_path given, project contracts) with their scope, source, and status."
    )]
    async fn list_rules(
        &self,
        Parameters(req): Parameters<ListRulesRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = req.project_path.as_deref().map(PathBuf::from);
        match self.store.load_rules(project.as_deref()) {
            Ok(rules) => json_result(&serde_json::json!({ "rules": rules, "count": rules.len() })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Create or update a rule file so a discovered project contract or personal preference becomes binding and visible in the console. Use this to actively record project-scoped issues/contracts (scope=project, kind=contract) instead of only recording session evidence. Reusing an id updates the rule and bumps its version. The written file is picked up by get_applicable_rules and frozen into future session snapshots."
    )]
    async fn save_rule(
        &self,
        Parameters(req): Parameters<SaveRuleRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.id.trim().is_empty() || req.title.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "id and title are required and cannot be empty",
                None,
            ));
        }
        let kind: RuleKind = serde_json::from_value(serde_json::Value::String(
            req.kind.clone().unwrap_or_else(|| "contract".into()),
        ))
        .map_err(|_| {
            ErrorData::invalid_params(
                format!(
                    "unknown rule kind '{}'; use preference|contract|constraint",
                    req.kind.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        let scope: Scope = serde_json::from_value(serde_json::Value::String(
            req.scope.clone().unwrap_or_else(|| "project".into()),
        ))
        .map_err(|_| {
            ErrorData::invalid_params(
                format!(
                    "unknown scope '{}'; use user|project",
                    req.scope.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        if scope == Scope::Session {
            return Err(ErrorData::invalid_params(
                "session scope cannot be saved as a rule file; use user or project",
                None,
            ));
        }
        let project_path = match scope {
            Scope::Project => Some(PathBuf::from(req.project_path.clone().ok_or_else(
                || ErrorData::invalid_params("project_path is required for project scope", None),
            )?)),
            _ => None,
        };
        let mut rule = agent_control_core::store::new_rule(
            req.id.trim(),
            req.title.trim(),
            &req.content,
            kind,
            scope,
            Some(req.agent_id.clone()),
        );
        rule.topic = req.topic.clone().filter(|t| !t.trim().is_empty());
        rule.tags = req.tags.clone();
        rule.source.origin = Some("agent-control-mcp".into());
        match self.store.upsert_rule(&rule, project_path.as_deref()) {
            Ok((saved, path)) => json_result(&serde_json::json!({
                "path": path,
                "rule": saved,
                "instruction": "Rule saved. It now appears in the console and binds future sessions; call get_applicable_rules to confirm.",
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Enable or disable an existing rule without changing its content. Disabled rules are excluded from resolution (Decision::Disabled) and no longer bind sessions; enabling restores them. Use this to pause a contract/preference temporarily instead of deleting it. scope=project (default) requires project_path; scope=user targets a personal preference. Content updates via save_rule never change enablement."
    )]
    async fn set_rule_enabled(
        &self,
        Parameters(req): Parameters<SetRuleEnabledRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let scope: Scope = serde_json::from_value(serde_json::Value::String(
            req.scope.clone().unwrap_or_else(|| "project".into()),
        ))
        .map_err(|_| {
            ErrorData::invalid_params(
                format!(
                    "unknown scope '{}'; use user|project",
                    req.scope.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        if scope == Scope::Session {
            return Err(ErrorData::invalid_params(
                "session scope cannot be toggled as a rule file; use user or project",
                None,
            ));
        }
        let project_path = match scope {
            Scope::Project => Some(PathBuf::from(req.project_path.clone().ok_or_else(
                || ErrorData::invalid_params("project_path is required for project scope", None),
            )?)),
            _ => None,
        };
        match self.store.set_rule_enabled(
            req.id.trim(),
            scope,
            project_path.as_deref(),
            req.enabled,
        ) {
            Ok(rule) => {
                let enabled = rule.enabled;
                json_result(&serde_json::json!({
                    "rule": rule,
                    "instruction": if enabled {
                        "Rule enabled. It binds future sessions; call get_applicable_rules to confirm."
                    } else {
                        "Rule disabled. It is excluded from resolution and no longer binds sessions."
                    },
                }))
            }
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Propose a new rule for HUMAN REVIEW instead of writing it directly. The draft is queued as a pending suggestion in the console; a user reviews, edits, approves (making it binding) or rejects it. scope=project (default) targets a project contract and requires project_path; scope=user proposes a personal preference that follows the user across projects (no project_path needed). Prefer this over save_rule when the rule is inferred, uncertain, or needs user confirmation. Include a rationale explaining why the rule is proposed."
    )]
    async fn suggest_rule(
        &self,
        Parameters(req): Parameters<SuggestRuleRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let scope: Scope = serde_json::from_value(serde_json::Value::String(
            req.scope.clone().unwrap_or_else(|| "project".into()),
        ))
        .map_err(|_| {
            ErrorData::invalid_params(
                format!(
                    "unknown scope '{}'; use user|project",
                    req.scope.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        if scope == Scope::Session {
            return Err(ErrorData::invalid_params(
                "session scope cannot be suggested; use user or project",
                None,
            ));
        }
        let project_path = req.project_path.clone().unwrap_or_default();
        if req.id.trim().is_empty()
            || req.title.trim().is_empty()
            || req.content.trim().is_empty()
            || (scope == Scope::Project && project_path.trim().is_empty())
        {
            return Err(ErrorData::invalid_params(
                "id, title and content are required; project_path is also required for project scope",
                None,
            ));
        }
        let kind: RuleKind = serde_json::from_value(serde_json::Value::String(
            req.kind.clone().unwrap_or_else(|| "contract".into()),
        ))
        .map_err(|_| {
            ErrorData::invalid_params(
                format!(
                    "unknown rule kind '{}'; use preference|contract|constraint",
                    req.kind.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        let mut draft = agent_control_core::store::new_suggestion(
            &PathBuf::from(project_path.trim()),
            &req.agent_id,
            req.id.trim(),
            req.title.trim(),
            &req.content,
            kind,
            scope,
        );
        draft.topic = req.topic.clone().filter(|t| !t.trim().is_empty());
        draft.tags = req.tags.clone();
        draft.rationale = req.rationale.clone().filter(|r| !r.trim().is_empty());
        match self.store.submit_suggestion(draft) {
            Ok(suggestion) => json_result(&serde_json::json!({
                "suggestion": suggestion,
                "instruction": "Suggestion queued for human review. It does NOT bind sessions until a user approves it in the console (Suggestions view). Continue working under the currently applicable rules.",
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "List rule suggestions submitted by agents, optionally filtered by project path and status (pending | approved | rejected). Returns the review queue plus a pending count so agents can check whether their proposals were reviewed."
    )]
    async fn list_suggestions(
        &self,
        Parameters(req): Parameters<ListSuggestionsRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let status: Option<SuggestionStatus> = match req.status.as_deref() {
            None | Some("") => None,
            Some(raw) => Some(
                serde_json::from_value(serde_json::Value::String(raw.to_string())).map_err(
                    |_| {
                        ErrorData::invalid_params(
                            format!("unknown status '{raw}'; use pending|approved|rejected"),
                            None,
                        )
                    },
                )?,
            ),
        };
        let project = req.project_path.as_deref().map(PathBuf::from);
        match self.store.list_suggestions(project.as_deref(), status) {
            Ok(suggestions) => {
                let pending = self
                    .store
                    .count_suggestions(project.as_deref(), Some(SuggestionStatus::Pending))
                    .unwrap_or(0);
                json_result(&serde_json::json!({
                    "suggestions": suggestions,
                    "count": suggestions.len(),
                    "pending_count": pending,
                }))
            }
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Compile the binding rule set into each harness's native entry point so agents that never call MCP still inherit the contracts. Injects a managed block (agent-control:begin … agent-control:end) into the project's AGENTS.md, CLAUDE.md and .cursor/rules/agent-control.mdc, preserving everything outside the block; only enabled rules are exported and re-export is idempotent. Set include_user=true to also write personal preferences to the user-global ~/.claude/CLAUDE.md. Use this to make contracts reach Claude Code / Cursor / Codex without requiring an MCP call."
    )]
    async fn export_native(
        &self,
        Parameters(req): Parameters<ExportNativeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let project = PathBuf::from(req.project_path.trim());
        match self.store.export_native(&project, req.include_user) {
            Ok(report) => json_result(&serde_json::json!({
                "report": report,
                "instruction": "Contracts compiled into native entry points. Everything outside the agent-control managed block was preserved and re-running is idempotent. Agents that read AGENTS.md / CLAUDE.md / .cursor rules now inherit these contracts without calling MCP.",
            })),
            Err(e) => tool_err(e),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ControlServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new(
            "agent-control",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Local-first engineering protocol console. Query applicable rules before work, \
             create a session, acknowledge constraints, and record verifiable evidence. \
             Rules carry provenance: user preference vs project contract, scope precedence, \
             and conflict detection against code.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(vec![
            Resource::new("control://overview", "overview")
                .with_description("Console overview: rule counts, sessions, recent conflicts")
                .with_mime_type("application/json"),
            Resource::new("control://sessions", "sessions")
                .with_description("All work sessions with constraint snapshots and handoff state")
                .with_mime_type("application/json"),
            Resource::new("rules://index", "rules-index")
                .with_description("All user-level preference rules")
                .with_mime_type("application/json"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResponse, ErrorData> {
        let uri = request.uri.as_str();
        let value = match uri {
            "control://overview" => {
                let rules = self.store.load_rules(None).unwrap_or_default();
                let sessions = self.store.list_sessions().unwrap_or_default();
                serde_json::json!({
                    "user_rules": rules.len(),
                    "sessions": sessions.len(),
                    "active_sessions": sessions.iter().filter(|s| s.status == agent_control_core::model::SessionStatus::Active).count(),
                })
            }
            "control://sessions" => {
                serde_json::to_value(self.store.list_sessions().unwrap_or_default()).unwrap()
            }
            "rules://index" => {
                serde_json::to_value(self.store.load_rules(None).unwrap_or_default()).unwrap()
            }
            _ => {
                return Err(ErrorData::resource_not_found(
                    format!("unknown resource '{uri}'"),
                    None,
                ));
            }
        };
        let text = serde_json::to_string_pretty(&value)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let contents = ResourceContents::text(text, uri).with_mime_type("application/json");
        Ok(ReadResourceResult::new(vec![contents]).into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(vec![Prompt::new(
            "constraint-briefing",
            Some(
                "Generate a constraint briefing for an agent: all applicable rules formatted as binding instructions with provenance",
            ),
            Some(vec![
                PromptArgument::new("project_path").with_required(true),
                PromptArgument::new("agent_id").with_required(true),
                PromptArgument::new("task").with_required(false),
            ]),
        )]))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::GetPromptResponse, ErrorData> {
        if request.name != "constraint-briefing" {
            return Err(ErrorData::invalid_params(
                format!("unknown prompt '{}'", request.name),
                None,
            ));
        }
        let args = request.arguments.unwrap_or_default();
        let get = |key: &str| -> Option<String> {
            args.get(key).and_then(|v| v.as_str()).map(String::from)
        };
        let Some(project_path) = get("project_path") else {
            return Err(ErrorData::invalid_params("project_path is required", None));
        };
        let agent_id = get("agent_id").unwrap_or_else(|| "unknown".into());
        let task = get("task").unwrap_or_default();
        let ctx = QueryContext {
            project_path: PathBuf::from(&project_path),
            agent_id: agent_id.clone(),
            file_paths: vec![],
            languages: vec![],
            task_tags: vec![],
            session_id: None,
        };
        let (_, resolution) = self
            .store
            .resolve_rules(&ctx)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut briefing = String::from(
            "# Engineering Constraints\n\nThe following rules are binding for this task. \
             They come from the local agent-control console with verified provenance.\n\n",
        );
        for m in &resolution.applied {
            if m.decision != Decision::Applied {
                continue;
            }
            briefing.push_str(&format!(
                "## {} [{}:{}]\n{}\n\n",
                m.rule.title, m.rule.scope, m.rule.kind, m.rule.content
            ));
        }
        if !resolution.conflicts.is_empty() {
            briefing.push_str("## Conflicts requiring attention\n");
            for c in &resolution.conflicts {
                briefing.push_str(&format!("- {}\n", c.description));
            }
        }
        if !task.is_empty() {
            briefing.push_str(&format!("\n## Task\n{task}\n"));
        }
        Ok(
            GetPromptResult::new(vec![PromptMessage::new_text(Role::User, briefing)])
                .with_description("Binding constraint briefing with provenance")
                .into(),
        )
    }
}

pub async fn serve_http(
    store: Arc<Store>,
    bind: &str,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    use std::net::SocketAddr;

    let factory_store = store.clone();
    let service = StreamableHttpService::new(
        move || Ok(ControlServer::new(factory_store.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_cancellation_token(shutdown.clone()),
    );
    // Loopback-only shutdown so the console (or a script) can stop a background daemon it does not
    // own as a child process. Rejects non-loopback peers in case the server is bound to 0.0.0.0.
    let shutdown_token = shutdown.clone();
    let app = axum::Router::new().nest_service("/mcp", service).route(
        "/shutdown",
        axum::routing::post(
            move |axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>| {
                let token = shutdown_token.clone();
                async move {
                    if !addr.ip().is_loopback() {
                        return (axum::http::StatusCode::FORBIDDEN, "loopback only");
                    }
                    token.cancel();
                    (axum::http::StatusCode::OK, "shutting down")
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "MCP Streamable HTTP server listening at http://{bind}/mcp");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
    .await?;
    Ok(())
}
