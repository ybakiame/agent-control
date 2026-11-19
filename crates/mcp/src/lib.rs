use std::path::PathBuf;
use std::sync::Arc;

use agent_control_core::Store;
use agent_control_core::model::{
    Decision, EvidenceKind, QueryContext, RuleKind, Scope, SessionStatus, SuggestionStatus,
    WorkItem, WorkItemStatus,
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

fn parse_enforcement(
    raw: Option<String>,
) -> Result<agent_control_core::model::Enforcement, ErrorData> {
    let value = raw.unwrap_or_else(|| "advisory".into());
    serde_json::from_value(serde_json::Value::String(value.clone())).map_err(|_| {
        ErrorData::invalid_params(
            format!("unknown enforcement '{value}'; use advisory|verify|hard"),
            None,
        )
    })
}

/// Resolve the requested export targets. Omitted or empty defaults to `AGENTS.md` only; unknown names
/// are rejected. Duplicates collapse, preserving first-seen order.
fn parse_targets(
    raw: Option<Vec<String>>,
) -> Result<Vec<agent_control_core::ExportTarget>, ErrorData> {
    use agent_control_core::ExportTarget;
    let names = raw.unwrap_or_default();
    if names.is_empty() {
        return Ok(ExportTarget::default_targets());
    }
    let mut out: Vec<ExportTarget> = Vec::new();
    for name in names {
        let target = ExportTarget::parse(&name).ok_or_else(|| {
            ErrorData::invalid_params(
                format!("unknown export target '{name}'; use agents|claude|gemini|cursor"),
                None,
            )
        })?;
        if !out.contains(&target) {
            out.push(target);
        }
    }
    Ok(out)
}

/// Resolve the requested hook targets. Omitted or empty defaults to all nine harnesses (each reads
/// only its own config, so writing all is safe); unknown names are rejected, duplicates collapse
/// preserving first-seen order.
fn parse_hook_targets(
    raw: Option<Vec<String>>,
) -> Result<Vec<agent_control_core::HookTarget>, ErrorData> {
    use agent_control_core::HookTarget;
    let names = raw.unwrap_or_default();
    if names.is_empty() {
        return Ok(HookTarget::ALL.to_vec());
    }
    let mut out: Vec<HookTarget> = Vec::new();
    for name in names {
        let target = HookTarget::parse(&name).ok_or_else(|| {
            ErrorData::invalid_params(
                format!(
                    "unknown hook target '{name}'; use claude|codebuddy|workbuddy|qoder|qoder-cn|codex|cursor|antigravity|kimi"
                ),
                None,
            )
        })?;
        if !out.contains(&target) {
            out.push(target);
        }
    }
    Ok(out)
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
    /// Optional receiver hint recorded in the handoff history. The real receiver is chosen at
    /// import_handoff (new_agent), so this may be omitted.
    #[serde(default)]
    pub to_agent: Option<String>,
    /// Context summary to carry over (conversation state, files touched, decisions)
    #[serde(default)]
    pub context: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetSessionStatusRequest {
    /// Session UUID
    pub session_id: String,
    /// New lifecycle status: active | completed | failed (handed_off is set by export_handoff)
    pub status: String,
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
    /// Scope: user | project | environment. Defaults to project. environment = machine/project-local
    /// descriptive context (git-ignored, never committed). Session scope cannot be saved as a file.
    #[serde(default)]
    pub scope: Option<String>,
    /// Absolute project path; required when scope is project or environment
    #[serde(default)]
    pub project_path: Option<String>,
    /// Optional topic to group related rules
    #[serde(default)]
    pub topic: Option<String>,
    /// Optional tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Declared enforcement intent: advisory (semantic, default) | verify (mechanically checkable
    /// via code_checks) | hard (must be mediated at a chokepoint that can deny). Declaring hard does
    /// NOT by itself achieve the Enforced tier — without a chokepoint it caps at Advisory/Verified.
    #[serde(default)]
    pub enforcement: Option<String>,
    /// Mechanical checks evaluated against repo files by verify_constraints. Each reads `key` from
    /// `file` and passes when the value agrees with `expect`. Supply these when enforcement=verify.
    #[serde(default)]
    pub code_checks: Vec<CodeCheckInput>,
    /// Tool guards enforced at the PreToolUse gate when enforcement=hard. Each denies a matching tool
    /// call (tool name glob + optional input regex). Only consulted by check_tool_call, never by
    /// verify_constraints. Supply these to make a hard rule deny specific dangerous calls.
    #[serde(default)]
    pub tool_guards: Vec<ToolGuardInput>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodeCheckInput {
    /// Repo-relative file to read (e.g. ".editorconfig", "package.json")
    pub file: String,
    /// Config key to inspect within the file
    pub key: String,
    /// Expected value; the check passes when the file's value agrees
    pub expect: String,
    /// Optional human-readable description of what this check guards
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ToolGuardInput {
    /// Tool name glob to match (e.g. "Bash", "Edit|Write", "*" or empty for any tool)
    #[serde(default)]
    pub tool: String,
    /// Optional regex matched against the JSON-serialized tool input; empty matches any input
    #[serde(default)]
    pub pattern: String,
    /// Why this call is denied; surfaced to the agent as the gate's denial reason
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetRuleEnabledRequest {
    /// Stable id of the rule to enable or disable
    pub id: String,
    /// true = the rule binds sessions; false = it is excluded from resolution (Decision::Disabled)
    pub enabled: bool,
    /// Scope: user | project | environment. Defaults to project. Session scope cannot be toggled as a file.
    #[serde(default)]
    pub scope: Option<String>,
    /// Absolute project path; required when scope is project or environment
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SuggestRuleRequest {
    /// Absolute path of the project this suggestion targets; required when scope=project or
    /// scope=environment, ignored when scope=user
    #[serde(default)]
    pub project_path: Option<String>,
    /// Scope: user | project | environment. Defaults to project. user = a personal preference that
    /// follows the user across projects; environment = a machine/project-local note (e.g. WSL), not a
    /// portable preference (both reviewed in the console Suggestions flow).
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
    /// Declared enforcement intent: advisory (semantic, default) | verify | hard. The human reviewer
    /// can override it on approval. Declaring hard does not by itself achieve the Enforced tier.
    #[serde(default)]
    pub enforcement: Option<String>,
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
    /// Absolute path of the project whose contracts are pointed at from native harness entry points
    pub project_path: String,
    /// Which harness entry points to write: any of agents | claude | gemini | cursor (file names like
    /// AGENTS.md also accepted). Omitted or empty defaults to ["agents"] only — writing every target
    /// at once is opt-in, because Claude Code reads only CLAUDE.md when both it and AGENTS.md exist.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// Also export personal preferences to the user-global ~/.claude/CLAUDE.md. Defaults to false
    /// so a project export never writes outside the project unless explicitly requested.
    #[serde(default)]
    pub include_user: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportDriftRequest {
    /// Absolute path of the project whose native export targets are checked for staleness
    pub project_path: String,
    /// Which harness entry points to check: agents | claude | gemini | cursor. Defaults to ["agents"].
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// Also check the user-global ~/.claude/CLAUDE.md against personal preferences. Defaults to false.
    #[serde(default)]
    pub include_user: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RevokeNativeRequest {
    /// Absolute path of the project whose native entry points should have the managed block removed
    pub project_path: String,
    /// Which harness entry points to revoke: agents | claude | gemini | cursor. Defaults to ["agents"].
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// Also revoke the personal-preferences block from the user-global ~/.claude/CLAUDE.md. Defaults
    /// to false so a project revoke never writes outside the project unless explicitly requested.
    #[serde(default)]
    pub include_user: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyConstraintsRequest {
    /// Session UUID whose frozen constraint snapshot is verified against the repository
    pub session_id: String,
    /// Harness running the verification (recorded in the evidence chain)
    pub agent_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkItemInput {
    /// What the unit of work is
    pub title: String,
    /// done | doing | todo (defaults to todo)
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateWorkStateRequest {
    /// Session UUID whose work state is recorded
    pub session_id: String,
    /// The objective (/goal) the session is working toward
    #[serde(default)]
    pub goal: String,
    /// Resumable work items; replaces the session's current list
    #[serde(default)]
    pub work_items: Vec<WorkItemInput>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckToolCallRequest {
    /// Absolute project path whose enabled hard rules gate this call
    pub project_path: String,
    /// Name of the tool about to run (e.g. "Bash", "Edit", "Write")
    pub tool_name: String,
    /// The tool's input arguments as JSON; matched against hard rules' tool_guards patterns
    #[serde(default)]
    pub tool_input: serde_json::Value,
    /// Harness making the call (narrows rules via applies_to.agents; recorded in evidence)
    pub agent_id: String,
    /// Optional session UUID; when given, the verdict is recorded in the evidence chain and mediated
    /// hard rules are marked Enforced. Omit for a stateless pre-check.
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RegisterHooksRequest {
    /// Absolute project path whose harness config files receive the hook
    pub project_path: String,
    /// Which harnesses to register: claude | codebuddy | workbuddy | qoder | qoder-cn | codex |
    /// cursor | antigravity | kimi. Omitted defaults to all nine. Qoder and Qoder CN share one
    /// project file (.qoder/settings.json), so selecting both writes it once (one report outcome
    /// per file). Kimi and Antigravity are user-scope: they write the user-global config, but the
    /// gate still evaluates per project_path (cwd), so enforcement is project-aware even though
    /// registration is global.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// Full command line that runs the hook script (e.g. node "/abs/agent-control-hook.mjs"). Omitted
    /// resolves the bundled script beside the executable (release) or in the repo scripts/ dir (dev).
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RevokeHooksRequest {
    /// Absolute project path whose harness config files should have the hook removed
    pub project_path: String,
    /// Which harnesses to revoke: claude | codebuddy | workbuddy | qoder | qoder-cn | codex |
    /// cursor | antigravity | kimi. Omitted defaults to all nine. Qoder and Qoder CN share
    /// .qoder/settings.json, so revoking either removes the shared entry for both.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DetectHooksRequest {
    /// Absolute project path to inspect for installed harnesses and existing hook registration.
    /// Reports all nine targets with their scope (project or user); user-scope targets (kimi,
    /// antigravity) inspect the user-global config.
    pub project_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DoctorRequest {
    /// Absolute project path to diagnose.
    pub project_path: String,
    /// Daemon bind address to probe (defaults to the configured MCP port on 127.0.0.1).
    #[serde(default)]
    pub daemon_bind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetupRequest {
    /// Absolute project path to onboard.
    pub project_path: String,
    /// Hook targets (claude|codebuddy|workbuddy|qoder|qoder-cn|codex|cursor|antigravity|kimi).
    /// Defaults to all nine.
    #[serde(default)]
    pub targets: Option<Vec<String>>,
    /// Native export targets (agents|claude|gemini|cursor). Defaults to ["agents"].
    #[serde(default)]
    pub export_targets: Option<Vec<String>>,
    /// Also write the user-global ~/.claude/CLAUDE.md pointer. Defaults to false.
    #[serde(default)]
    pub include_user: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    /// Search text. Chinese queries of two or more characters work (CJK bigram segmentation).
    pub query: String,
    /// Restrict to one corpus: rules | sessions | evidence. Omitted searches all three.
    #[serde(default)]
    pub kind: Option<String>,
    /// Restrict to one project's rows (user-scope rules carry an empty path and are excluded).
    #[serde(default)]
    pub project_path: Option<String>,
    /// Maximum hits across corpora (default 20).
    #[serde(default)]
    pub limit: Option<usize>,
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
                    "briefing": agent_control_core::render_constraint_briefing(&resolution, ""),
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
        description = "Create a work session with a frozen constraint snapshot. The snapshot binds the applicable rules at session start so continuity can be verified across harness handoffs. Idempotent per (project_path, agent_id, task): an existing ACTIVE session with the same triple is returned unchanged with reused=true instead of piling up duplicates; a new task freezes a fresh snapshot."
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
            Ok((session, reused)) => json_result(&serde_json::json!({
                "session_id": session.id,
                "reused": reused,
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
        description = "Export a handoff bundle to transfer this session to another harness, and refresh the project-local handoff/latest.md continuity brief. The bundle carries the frozen constraints, achieved tiers, goal/work state and evidence chain head so the next harness can be verified against the same rules. to_agent is an optional hint; the receiver chooses itself at import_handoff. Marks the source session handed_off."
    )]
    async fn export_handoff(
        &self,
        Parameters(req): Parameters<ExportHandoffRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        match self
            .store
            .export_handoff(session_id, req.to_agent.as_deref(), req.context)
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
            Ok(session) => {
                let carried = session.assurance.clone();
                json_result(&serde_json::json!({
                    "session_id": session.id,
                    "agent_id": session.agent_id,
                    "rules_to_acknowledge": session.constraint_snapshot.rules.iter().map(|r| &r.id).collect::<Vec<_>>(),
                    "carried_assurance": carried,
                    "goal": session.goal,
                    "work_items": session.work_items,
                    "tier_warning": agent_control_core::handoff_tier_warning(&session.assurance),
                }))
            }
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
        description = "Create or update a rule file so a discovered project contract or personal preference becomes binding and visible in the console. Use this to actively record project-scoped issues/contracts (scope=project, kind=contract) instead of only recording session evidence. scope=user records a portable personal preference; scope=environment records machine/project-local context (e.g. agents run inside WSL) into git-ignored .agent-control/local/ — descriptive, always applied, never committed, requires project_path. Reusing an id updates the rule and bumps its version. The written file is picked up by get_applicable_rules and frozen into future session snapshots."
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
                    "unknown scope '{}'; use user|project|environment",
                    req.scope.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        if scope == Scope::Session {
            return Err(ErrorData::invalid_params(
                "session scope cannot be saved as a rule file; use user, project or environment",
                None,
            ));
        }
        let project_path = match scope {
            Scope::Project | Scope::Environment => Some(PathBuf::from(
                req.project_path.clone().ok_or_else(|| {
                    ErrorData::invalid_params(
                        "project_path is required for project/environment scope",
                        None,
                    )
                })?,
            )),
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
        rule.enforcement = parse_enforcement(req.enforcement.clone())?;
        rule.code_checks = req
            .code_checks
            .iter()
            .map(|c| agent_control_core::model::CodeCheck {
                file: c.file.clone(),
                key: c.key.clone(),
                expect: c.expect.clone(),
                description: c.description.clone(),
            })
            .collect();
        rule.tool_guards = req
            .tool_guards
            .iter()
            .map(|g| agent_control_core::model::ToolGuard {
                tool: g.tool.clone(),
                pattern: g.pattern.clone(),
                reason: g.reason.clone(),
            })
            .collect();
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
        description = "Enable or disable an existing rule without changing its content. Disabled rules are excluded from resolution (Decision::Disabled) and no longer bind sessions; enabling restores them. Use this to pause a contract/preference temporarily instead of deleting it. scope=project (default) and scope=environment require project_path; scope=user targets a personal preference. Content updates via save_rule never change enablement."
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
                    "unknown scope '{}'; use user|project|environment",
                    req.scope.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        if scope == Scope::Session {
            return Err(ErrorData::invalid_params(
                "session scope cannot be toggled as a rule file; use user, project or environment",
                None,
            ));
        }
        let project_path = match scope {
            Scope::Project | Scope::Environment => Some(PathBuf::from(
                req.project_path.clone().ok_or_else(|| {
                    ErrorData::invalid_params(
                        "project_path is required for project/environment scope",
                        None,
                    )
                })?,
            )),
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
        description = "Propose a new rule for HUMAN REVIEW instead of writing it directly. The draft is queued as a pending suggestion in the console; a user reviews, edits, approves (making it binding) or rejects it. scope=project (default) targets a project contract and requires project_path; scope=user proposes a personal preference that follows the user across projects (no project_path needed); scope=environment proposes a machine/project-local note (requires project_path) — the right home for observed environment facts like WSL, which are not portable preferences. Prefer this over save_rule when the rule is inferred, uncertain, or needs user confirmation. Include a rationale explaining why the rule is proposed."
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
                    "unknown scope '{}'; use user|project|environment",
                    req.scope.clone().unwrap_or_default()
                ),
                None,
            )
        })?;
        if scope == Scope::Session {
            return Err(ErrorData::invalid_params(
                "session scope cannot be suggested; use user, project or environment",
                None,
            ));
        }
        let project_path = req.project_path.clone().unwrap_or_default();
        if req.id.trim().is_empty()
            || req.title.trim().is_empty()
            || req.content.trim().is_empty()
            || ((scope == Scope::Project || scope == Scope::Environment)
                && project_path.trim().is_empty())
        {
            return Err(ErrorData::invalid_params(
                "id, title and content are required; project_path is also required for project/environment scope",
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
        draft.enforcement = parse_enforcement(req.enforcement.clone())?;
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
        description = "Write a static pointer block into the selected native harness entry points so agents that never call MCP still learn where the contracts live. The block (agent-control:begin … agent-control:end) points at the live rule files in .agent-control/rules/ — it does NOT copy rule text, so editing a rule never makes it stale and re-export is byte-idempotent. Choose targets with `targets` (agents | claude | gemini | cursor); omitted defaults to AGENTS.md only. Everything outside the block is preserved. Set include_user=true to also write a preferences pointer to the user-global ~/.claude/CLAUDE.md. Use revoke_native to undo."
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
        let targets = parse_targets(req.targets)?;
        let project = PathBuf::from(req.project_path.trim());
        match self
            .store
            .export_native(&project, &targets, req.include_user)
        {
            Ok(report) => json_result(&serde_json::json!({
                "report": report,
                "instruction": "Pointer block written to the selected entry points. It directs agents to read the live rules in .agent-control/rules/ (or call get_applicable_rules); it does not inline rule text. Everything outside the agent-control managed block was preserved and re-running is idempotent. Use revoke_native to remove it.",
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Check whether the selected native entry points carry a current-format pointer block. Each managed block embeds a block-format version; a target is stale when it exists and that version differs from the current one (e.g. a legacy v1 full-text block that should be migrated to the v2 pointer). Choose targets with `targets` (agents | claude | gemini | cursor); omitted defaults to AGENTS.md only. It does NOT rewrite anything and never goes stale on a rule edit (the pointer is static). When stale, re-run export_native. This detects file-vs-store format drift only — not whether a running session's context is stale."
    )]
    async fn check_export_drift(
        &self,
        Parameters(req): Parameters<ExportDriftRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let targets = parse_targets(req.targets)?;
        let project = PathBuf::from(req.project_path.trim());
        match self
            .store
            .export_drift(&project, &targets, req.include_user)
        {
            Ok(report) => {
                let stale = report.stale;
                json_result(&serde_json::json!({
                    "report": report,
                    "instruction": if stale {
                        "Native export is stale: a target is missing the current-format pointer block (or holds a legacy v1 full-text block). Re-run export_native to migrate it. Nothing was rewritten by this check."
                    } else {
                        "Native export is in sync (or not yet exported). No re-export needed."
                    },
                }))
            }
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Revoke (remove) the Agent Control pointer block from the selected native harness entry points — the explicit undo of export_native. Surrounding prose is preserved; a file that held only the block (or only the generated Cursor frontmatter + block) is deleted. Choose targets with `targets` (agents | claude | gemini | cursor); omitted defaults to AGENTS.md only. Set include_user=true to also revoke the preferences block from ~/.claude/CLAUDE.md. A target with no block is left unchanged, so re-running is safe."
    )]
    async fn revoke_native(
        &self,
        Parameters(req): Parameters<RevokeNativeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let targets = parse_targets(req.targets)?;
        let project = PathBuf::from(req.project_path.trim());
        match self
            .store
            .revoke_native(&project, &targets, req.include_user)
        {
            Ok(report) => json_result(&serde_json::json!({
                "report": report,
                "instruction": "Pointer block revoked from the selected entry points. Files that held only the block were deleted; files with surrounding prose kept it (action=removed). Targets with no block were left unchanged.",
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Verify a session's frozen constraints against the repository and compute the ACHIEVED assurance tier per rule. Runs each rule's code_checks against the project files: all pass => Verified; no checks (a semantic rule) or any failure => Advisory. Declaring enforcement=hard does NOT by itself achieve Enforced — without a chokepoint that can deny, hard rules cap at Advisory/Verified. Records the result in the evidence chain. This is distinct from verify_evidence, which only recomputes hash-chain integrity. Call after acknowledge_rules and before handing off."
    )]
    async fn verify_constraints(
        &self,
        Parameters(req): Parameters<VerifyConstraintsRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        match self.store.verify_constraints(session_id, &req.agent_id) {
            Ok(report) => json_result(&serde_json::to_value(report).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Record the session's resumable work state: the goal (the /goal objective) plus work items marked done/doing/todo. Replaces the session's goal and work items. export_handoff carries them into the bundle and handoff/latest.md so a new harness inherits progress without relying on its own todo list. Call it as work completes or starts, and before exporting a handoff."
    )]
    async fn update_work_state(
        &self,
        Parameters(req): Parameters<UpdateWorkStateRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        let items = req
            .work_items
            .into_iter()
            .map(|w| WorkItem {
                title: w.title,
                status: match w.status.trim() {
                    "done" => WorkItemStatus::Done,
                    "doing" => WorkItemStatus::Doing,
                    _ => WorkItemStatus::Todo,
                },
            })
            .collect();
        match self
            .store
            .set_session_work_state(session_id, req.goal, items)
        {
            Ok(session) => json_result(&serde_json::to_value(session).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Close or reopen a session: status = active | completed | failed. Completing or failing appends a session_end evidence entry (the chain stays auditable) and takes the session out of the active list; reopen with active to resume. handed_off cannot be set here — export_handoff owns that transition. Call it when a unit of work ends so sessions do not pile up as forever-active."
    )]
    async fn set_session_status(
        &self,
        Parameters(req): Parameters<SetSessionStatusRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = parse_uuid(&req.session_id)?;
        let status: SessionStatus = serde_json::from_value(serde_json::Value::String(
            req.status.clone(),
        ))
        .map_err(|_| {
            ErrorData::invalid_params(
                format!(
                    "unknown status '{}'; use active|completed|failed",
                    req.status
                ),
                None,
            )
        })?;
        if status == SessionStatus::HandedOff {
            return Err(ErrorData::invalid_params(
                "handed_off is set by export_handoff, not set_session_status",
                None,
            ));
        }
        match self.store.set_session_status(session_id, status) {
            Ok(session) => json_result(&serde_json::to_value(session).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "The deny-capable PreToolUse gate. Call BEFORE running a tool to check it against the project's enabled hard rules: a hard rule's code_checks (repo gate) failing/inconclusive, or a tool_guard matching this call, denies it. Returns allowed, denials (with reasons), and enforced_rules (the hard rules that mediated and allowed the call). This is the ONLY path that achieves the Enforced assurance tier — declaring enforcement=hard without this chokepoint caps at Advisory/Verified. When session_id is given the verdict is recorded in the evidence chain. With no enabled hard rules the call is allowed. Wire it into a harness PreToolUse hook (register_hooks) so every call is mediated; on allowed=false the hook must block the tool and feed denial_reason back to the agent."
    )]
    async fn check_tool_call(
        &self,
        Parameters(req): Parameters<CheckToolCallRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        if req.tool_name.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "tool_name is required and cannot be empty",
                None,
            ));
        }
        let session_id = match req.session_id.as_deref() {
            Some(s) if !s.trim().is_empty() => Some(parse_uuid(s)?),
            _ => None,
        };
        let project = PathBuf::from(req.project_path.trim());
        match self.store.check_tool_call(
            &project,
            req.tool_name.trim(),
            &req.tool_input,
            &req.agent_id,
            session_id,
        ) {
            Ok(report) => {
                let allowed = report.allowed;
                let reason = report.denial_reason();
                let instruction = if allowed {
                    "Tool call allowed by the gate. Proceed.".to_string()
                } else {
                    format!(
                        "Tool call DENIED by the gate. Do not run it. Reason: {reason}. Fix the underlying constraint (or adjust the hard rule) and re-check."
                    )
                };
                json_result(&serde_json::json!({
                    "report": report,
                    "denial_reason": reason,
                    "instruction": instruction,
                }))
            }
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Register the Agent Control hook into the selected harnesses' config so each harness drives enforcement itself. Writes a start hook (freezes the constraint snapshot and, where supported, injects the binding-rule briefing) and a gate hook (runs check_tool_call to deny calls that violate hard rules) — this is what turns a hard rule into an Enforced one. Nine targets: claude | codebuddy | workbuddy | qoder | qoder-cn | codex | cursor | antigravity | kimi; omitted registers all nine. Each writes its own config in its own schema: nested settings.json hooks (claude, codebuddy, workbuddy, qoder, qoder-cn, codex — qoder and qoder-cn SHARE .qoder/settings.json, written once with one report outcome), Cursor flat hooks.json (version:1, sessionStart/preToolUse), Antigravity named bundle in ~/.gemini/config/hooks.json (PreInvocation/PreToolUse), Kimi [[hooks]] block in ~/.kimi-code/config.toml. Kimi and Antigravity are user-scope (write the user-global config) but the gate evaluates per project_path/cwd, so enforcement stays project-aware. Deny shapes are vendor-native: exit 2 + stderr for the claude family, {permission:deny} JSON for Cursor, {decision:deny} JSON for Antigravity. The hook command defaults to the bundled agent-control-hook.mjs beside the executable (vendor + event baked into argv); override with `command`. Every other config key and user-authored hook is preserved; re-registering is idempotent. The daemon must be reachable at the hook endpoint (default http://127.0.0.1:9870/mcp). Use revoke_hooks to undo, detect_hooks to inspect."
    )]
    async fn register_hooks(
        &self,
        Parameters(req): Parameters<RegisterHooksRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let targets = parse_hook_targets(req.targets)?;
        let command = match req.command {
            Some(c) if !c.trim().is_empty() => c,
            _ => agent_control_core::hooks::default_hook_command().ok_or_else(|| {
                ErrorData::invalid_params(
                    "hook script not found beside the executable; supply an explicit `command` (e.g. node \"/abs/agent-control-hook.mjs\")",
                    None,
                )
            })?,
        };
        let project = PathBuf::from(req.project_path.trim());
        match self.store.register_hooks(&project, &targets, &command) {
            Ok(report) => json_result(&serde_json::json!({
                "report": report,
                "command": command,
                "instruction": "Hook registered. The harness now injects the briefing at SessionStart and mediates tool calls at PreToolUse via check_tool_call. Restart the harness session for the hook to take effect; hard rules can now reach the Enforced tier. Use revoke_hooks to remove.",
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Revoke (remove) the Agent Control hook from the selected harnesses' config — the explicit undo of register_hooks. Other config and user-authored hooks are preserved; a config file that held only our hook is deleted (for Kimi, only our managed [[hooks]] block is stripped from config.toml, leaving the rest byte-for-byte). Choose targets (claude | codebuddy | workbuddy | qoder | qoder-cn | codex | cursor | antigravity | kimi); omitted defaults to all nine. Kimi and Antigravity are user-scope, so this edits the user-global config. Qoder and Qoder CN share .qoder/settings.json, so revoking either removes the shared entry for both. A target with no managed hook is left unchanged, so re-running is safe. After revoking, hard rules are no longer mediated and cap at Advisory/Verified."
    )]
    async fn revoke_hooks(
        &self,
        Parameters(req): Parameters<RevokeHooksRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let targets = parse_hook_targets(req.targets)?;
        let project = PathBuf::from(req.project_path.trim());
        match self.store.revoke_hooks(&project, &targets) {
            Ok(report) => json_result(&serde_json::json!({
                "report": report,
                "instruction": "Hook revoked from the selected harnesses. Files that held only our hook were deleted; other config and user hooks were preserved. Hard rules are no longer mediated at a chokepoint.",
            })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Detect which harnesses are installed and whether the Agent Control hook is already registered in each one's config. Covers all nine: Claude Code, CodeBuddy, WorkBuddy, Qoder, Qoder CN, Codex, Cursor, Antigravity, Kimi Code. Read-only; drives the console's one-click register/restore UI. Returns per-harness installed/registered flags, the config path, and the scope (project or user — kimi and antigravity inspect the user-global config; qoder and qoder-cn inspect the same shared .qoder/settings.json). Use before register_hooks to choose targets."
    )]
    async fn detect_hooks(
        &self,
        Parameters(req): Parameters<DetectHooksRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let project = PathBuf::from(req.project_path.trim());
        match self.store.detect_hooks(&project) {
            Ok(statuses) => json_result(&serde_json::json!({ "harnesses": statuses })),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Read-only project diagnostics in one report: hook registration across the nine harnesses, native-export drift, evidence-chain integrity for every session, rule-conflict count and MCP daemon reachability. `healthy` is false when any chain is broken, any export target is stale or any conflict exists. Performs no writes."
    )]
    async fn doctor(
        &self,
        Parameters(req): Parameters<DoctorRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let project = PathBuf::from(req.project_path.trim());
        match self.store.doctor(&project, req.daemon_bind.as_deref()) {
            Ok(report) => json_result(&serde_json::to_value(report).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "One-command harness onboarding: composes export_native (pointer block into the selected native entry points) and register_hooks (start briefing + pre-tool gate) in one idempotent call. Defaults: all nine hook targets, AGENTS.md export only. Re-running reports unchanged. Returns the export and hook reports plus the wired command; use revoke_native / revoke_hooks to undo."
    )]
    async fn setup(
        &self,
        Parameters(req): Parameters<SetupRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if req.project_path.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project_path is required and cannot be empty",
                None,
            ));
        }
        let hook_targets = parse_hook_targets(req.targets)?;
        let export_targets = parse_targets(req.export_targets)?;
        let project = PathBuf::from(req.project_path.trim());
        match self
            .store
            .setup(&project, &hook_targets, &export_targets, req.include_user)
        {
            Ok(report) => json_result(&serde_json::to_value(report).unwrap()),
            Err(e) => tool_err(e),
        }
    }

    #[tool(
        description = "Full-text search over rules, sessions and evidence with bm25 ranking and snippets. Chinese queries of two or more characters match (the index and the query share a CJK-bigram segmenter). Optional kind (rules|sessions|evidence) and project_path narrow the corpus; limit defaults to 20. Read-only."
    )]
    async fn search(
        &self,
        Parameters(req): Parameters<SearchRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind = match req.kind.as_deref() {
            None => None,
            Some(k) => Some(
                serde_json::from_str::<agent_control_core::SearchKind>(&format!("\"{k}\""))
                    .map_err(|_| {
                        ErrorData::invalid_params(
                            format!("unknown kind '{k}'; use rules|sessions|evidence"),
                            None,
                        )
                    })?,
            ),
        };
        let project = req
            .project_path
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);
        match self.store.search(
            &req.query,
            kind,
            project.as_deref(),
            req.limit.unwrap_or(20),
        ) {
            Ok(hits) => {
                let count = hits.len();
                json_result(&serde_json::json!({ "hits": hits, "count": count }))
            }
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
        let briefing = agent_control_core::render_constraint_briefing(&resolution, &task);
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
