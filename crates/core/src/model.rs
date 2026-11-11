use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    User = 0,
    Project = 1,
    Session = 2,
}

impl Scope {
    pub fn rank(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Session => "session",
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleKind {
    Preference,
    Contract,
    Constraint,
}

impl std::fmt::Display for RuleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleKind::Preference => f.write_str("preference"),
            RuleKind::Contract => f.write_str("contract"),
            RuleKind::Constraint => f.write_str("constraint"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    User,
    Project,
    Imported,
    Inferred,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSource {
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppliesTo {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<String>,
}

impl AppliesTo {
    pub fn is_universal(&self) -> bool {
        self.agents.is_empty()
            && self.paths.is_empty()
            && self.languages.is_empty()
            && self.tasks.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeCheck {
    pub file: String,
    pub key: String,
    pub expect: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    #[serde(default = "default_version")]
    pub version: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default)]
    pub content: String,
    pub kind: RuleKind,
    pub scope: Scope,
    pub source: RuleSource,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default)]
    pub applies_to: AppliesTo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts_with: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_checks: Vec<CodeCheck>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checksum: String,
}

fn default_version() -> u32 {
    1
}
fn default_priority() -> i32 {
    100
}
fn default_enabled() -> bool {
    true
}

impl Rule {
    pub fn compute_checksum(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update(self.version.to_le_bytes());
        hasher.update(self.content.as_bytes());
        hasher.update(serde_json::to_vec(&self.applies_to).unwrap_or_default());
        hex::encode(hasher.finalize())
    }

    pub fn with_checksum(mut self) -> Self {
        self.checksum = self.compute_checksum();
        self
    }

    pub fn competition_key(&self) -> String {
        self.topic
            .clone()
            .unwrap_or_else(|| format!("rule:{}", self.id))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryContext {
    pub project_path: PathBuf,
    pub agent_id: String,
    #[serde(default)]
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub task_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Applied,
    Superseded,
    ConflictFlagged,
    FilteredOut,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatchReason {
    AgentMatch {
        pattern: String,
        agent: String,
    },
    PathMatch {
        pattern: String,
        path: String,
    },
    LanguageMatch {
        language: String,
    },
    TaskTagMatch {
        tag: String,
    },
    Universal,
    ScopePrecedence {
        winner: Scope,
        over: Scope,
    },
    PriorityWin {
        winner: i32,
        over: i32,
    },
    RecencyWin,
    ExplicitConflict {
        other_rule: String,
    },
    CodeCheckFailed {
        file: String,
        key: String,
        expected: String,
        actual: String,
    },
    Excluded {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMatch {
    pub rule: Rule,
    pub decision: Decision,
    pub reasons: Vec<MatchReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub applied: Vec<RuleMatch>,
    pub trace: Vec<RuleMatch>,
    pub conflicts: Vec<Conflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub rule_ids: Vec<String>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    RuleVsRule,
    RuleVsCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    pub agent_id: String,
    pub context: QueryContext,
    pub trace: Vec<RuleMatch>,
    pub conflicts: Vec<Conflict>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    RuleAcknowledgment,
    ToolCall,
    FileChange,
    TestResult,
    HandoffOut,
    HandoffIn,
    ConstraintViolation,
    SessionStart,
    SessionEnd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceEntry {
    pub seq: u64,
    pub id: Uuid,
    pub session_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub kind: EvidenceKind,
    pub agent_id: String,
    pub payload: serde_json::Value,
    pub payload_hash: String,
    pub prev_hash: String,
    pub entry_hash: String,
}

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

impl EvidenceEntry {
    pub fn seal(
        seq: u64,
        session_id: Uuid,
        timestamp: DateTime<Utc>,
        kind: EvidenceKind,
        agent_id: String,
        payload: serde_json::Value,
        prev_hash: String,
    ) -> Self {
        let payload_hash = canonical_hash(&payload);
        let entry_hash = chain_hash(seq, session_id, timestamp, kind, &payload_hash, &prev_hash);
        Self {
            seq,
            id: Uuid::new_v4(),
            session_id,
            timestamp,
            kind,
            agent_id,
            payload,
            payload_hash,
            prev_hash,
            entry_hash,
        }
    }

    pub fn verify(&self, expected_prev: &str) -> bool {
        if self.prev_hash != expected_prev {
            return false;
        }
        if canonical_hash(&self.payload) != self.payload_hash {
            return false;
        }
        chain_hash(
            self.seq,
            self.session_id,
            self.timestamp,
            self.kind,
            &self.payload_hash,
            &self.prev_hash,
        ) == self.entry_hash
    }
}

pub fn canonical_hash(value: &serde_json::Value) -> String {
    let canonical = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    hex::encode(hasher.finalize())
}

fn chain_hash(
    seq: u64,
    session_id: Uuid,
    timestamp: DateTime<Utc>,
    kind: EvidenceKind,
    payload_hash: &str,
    prev_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seq.to_le_bytes());
    hasher.update(session_id.as_bytes());
    hasher.update(timestamp.to_rfc3339().as_bytes());
    hasher.update(format!("{kind:?}").as_bytes());
    hasher.update(payload_hash.as_bytes());
    hasher.update(prev_hash.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub session_id: Uuid,
    pub entries_checked: u64,
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub broken_at: Vec<u64>,
    pub head_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    HandedOff,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintSnapshot {
    pub rules: Vec<Rule>,
    pub snapshot_hash: String,
    pub provenance_id: Uuid,
}

impl ConstraintSnapshot {
    pub fn new(rules: Vec<Rule>, provenance_id: Uuid) -> Self {
        let snapshot_hash = canonical_hash(&serde_json::to_value(&rules).unwrap_or_default());
        Self {
            rules,
            snapshot_hash,
            provenance_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub task: String,
    pub project_path: PathBuf,
    pub agent_id: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub constraint_snapshot: ConstraintSnapshot,
    #[serde(default)]
    pub handoffs: Vec<HandoffRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffRecord {
    pub id: Uuid,
    pub from_agent: String,
    pub to_agent: String,
    pub timestamp: DateTime<Utc>,
    pub bundle_hash: String,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffBundle {
    pub session_id: Uuid,
    pub task: String,
    pub project_path: PathBuf,
    pub constraint_snapshot: ConstraintSnapshot,
    pub evidence_head_hash: String,
    pub context: serde_json::Value,
    pub from_agent: String,
    pub created_at: DateTime<Utc>,
    pub bundle_hash: String,
}

impl HandoffBundle {
    pub fn compute_hash(&self) -> String {
        let mut clone = self.clone();
        clone.bundle_hash = String::new();
        canonical_hash(&serde_json::to_value(&clone).unwrap_or_default())
    }

    pub fn verify_integrity(&self) -> bool {
        self.compute_hash() == self.bundle_hash
            && canonical_hash(
                &serde_json::to_value(&self.constraint_snapshot.rules).unwrap_or_default(),
            ) == self.constraint_snapshot.snapshot_hash
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistration {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<AgentProtocol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentProtocol {
    Acp,
    Mcp,
}

/// A project onboarded into the console. The registry lives in SQLite (local app state);
/// the project's contract rules live in git-friendly files under `<path>/.agent-control/rules/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionStatus {
    Pending,
    Approved,
    Rejected,
}

impl SuggestionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

impl std::fmt::Display for SuggestionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A rule proposed by an agent, held in SQLite until a human approves it (optionally editing the
/// draft) or rejects it. Approval writes a binding project rule file; the row keeps the audit
/// trail (who proposed, what was changed, why it was rejected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSuggestion {
    pub id: Uuid,
    pub project_path: PathBuf,
    /// Whether approval writes a personal preference (user) or a project contract (project).
    /// `project_path` is empty for user-scope suggestions.
    #[serde(default = "default_suggestion_scope")]
    pub scope: Scope,
    pub agent_id: String,
    pub rule_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub content: String,
    pub kind: RuleKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub status: SuggestionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_rule_id: Option<String>,
}

fn default_suggestion_scope() -> Scope {
    Scope::Project
}

/// Optional human edits applied when approving a suggestion. `None` fields keep the agent's draft.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuggestionEdits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<RuleKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}
