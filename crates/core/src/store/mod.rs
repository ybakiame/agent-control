pub mod db;
pub mod files;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::DataLayout;
use crate::engine::RuleEngine;
use crate::model::{
    AgentRegistration, ConstraintSnapshot, EvidenceEntry, EvidenceKind, HandoffBundle,
    HandoffRecord, Project, ProvenanceRecord, QueryContext, Resolution, Rule, RuleKind,
    RuleSuggestion, Scope, Session, SessionStatus, SourceKind, SuggestionEdits, SuggestionStatus,
    VerificationReport, canonical_hash,
};

pub use db::Db;

pub struct Store {
    pub layout: DataLayout,
    db: Db,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AcknowledgmentReport {
    pub session_id: Uuid,
    pub agent_id: String,
    pub acknowledged: Vec<String>,
    pub missing: Vec<String>,
    pub unexpected: Vec<String>,
    pub fully_compliant: bool,
}

impl Store {
    pub fn open(layout: DataLayout) -> Result<Self> {
        layout.ensure_dirs()?;
        let db = Db::open(&layout.db_path())?;
        Ok(Self { layout, db })
    }

    pub fn open_default() -> Result<Self> {
        Self::open(DataLayout::default_layout()?)
    }

    pub fn open_with_root(root: PathBuf) -> Result<Self> {
        Self::open(DataLayout::new(root))
    }

    pub fn load_rules(&self, project_path: Option<&Path>) -> Result<Vec<Rule>> {
        let mut rules = files::load_rules_from_dir(&self.layout.user_rules_dir(), Scope::User)?;
        if let Some(project) = project_path {
            let project_rules = files::load_rules_from_dir(
                &DataLayout::project_rules_dir(project),
                Scope::Project,
            )?;
            rules.extend(project_rules);
        }
        Ok(rules)
    }

    pub fn save_rule(&self, rule: &Rule, project_path: Option<&Path>) -> Result<PathBuf> {
        let dir = match rule.scope {
            Scope::User => self.layout.user_rules_dir(),
            Scope::Project => {
                let project = project_path.context("project scope requires a project path")?;
                DataLayout::project_rules_dir(project)
            }
            Scope::Session => {
                bail!("session rules are stored in the session snapshot, not as files")
            }
        };
        let mut rule = rule.clone().with_checksum();
        rule.source.file_path = None;
        files::write_rule_file(&dir, &rule)
    }

    /// Create or update a rule by id. When the id already exists, bump its version and preserve the
    /// original `created_at` so provenance reflects when the contract first appeared. Used by the
    /// MCP `save_rule` tool so agents can record project contracts actively. Returns the saved rule
    /// (with its final version) and the file path.
    pub fn upsert_rule(&self, rule: &Rule, project_path: Option<&Path>) -> Result<(Rule, PathBuf)> {
        let mut rule = rule.clone();
        if let Some(existing) = self.get_rule(&rule.id, project_path)? {
            rule.version = existing.version + 1;
            rule.source.created_at = existing.source.created_at;
        }
        let path = self.save_rule(&rule, project_path)?;
        Ok((rule.with_checksum(), path))
    }

    pub fn delete_rule(
        &self,
        rule_id: &str,
        scope: Scope,
        project_path: Option<&Path>,
    ) -> Result<bool> {
        let dir = match scope {
            Scope::User => self.layout.user_rules_dir(),
            Scope::Project => {
                let project = project_path.context("project scope requires a project path")?;
                DataLayout::project_rules_dir(project)
            }
            Scope::Session => bail!("session rules cannot be deleted as files"),
        };
        files::delete_rule_file(&dir, rule_id)
    }

    pub fn get_rule(&self, rule_id: &str, project_path: Option<&Path>) -> Result<Option<Rule>> {
        Ok(self
            .load_rules(project_path)?
            .into_iter()
            .find(|r| r.id == rule_id))
    }

    pub fn resolve_rules(&self, ctx: &QueryContext) -> Result<(ProvenanceRecord, Resolution)> {
        let rules = self.load_rules(Some(&ctx.project_path))?;
        let (record, resolution) = RuleEngine::provenance(&rules, ctx);
        self.db.insert_provenance(&record)?;
        Ok((record, resolution))
    }

    pub fn explain_rule(
        &self,
        rule_id: &str,
        ctx: &QueryContext,
    ) -> Result<Option<RuleExplanation>> {
        let rules = self.load_rules(Some(&ctx.project_path))?;
        let Some(rule) = rules.iter().find(|r| r.id == rule_id) else {
            return Ok(None);
        };
        let (record, resolution) = RuleEngine::provenance(&rules, ctx);
        self.db.insert_provenance(&record)?;
        let rule_match = resolution
            .trace
            .iter()
            .find(|m| m.rule.id == rule_id)
            .cloned();
        let related_conflicts = resolution
            .conflicts
            .iter()
            .filter(|c| c.rule_ids.contains(&rule_id.to_string()))
            .cloned()
            .collect();
        Ok(Some(RuleExplanation {
            rule: rule.clone(),
            decision_trace: rule_match,
            provenance_id: record.id,
            conflicts: related_conflicts,
        }))
    }

    pub fn create_session(
        &self,
        task: &str,
        project_path: &Path,
        agent_id: &str,
        task_tags: Vec<String>,
    ) -> Result<Session> {
        let ctx = QueryContext {
            project_path: project_path.to_path_buf(),
            agent_id: agent_id.to_string(),
            file_paths: vec![],
            languages: vec![],
            task_tags,
            session_id: None,
        };
        let (record, resolution) = self.resolve_rules(&ctx)?;
        let applied_rules: Vec<Rule> = resolution
            .applied
            .iter()
            .filter(|m| m.decision == crate::model::Decision::Applied)
            .map(|m| m.rule.clone())
            .collect();
        let snapshot = ConstraintSnapshot::new(applied_rules, record.id);
        let now = Utc::now();
        let session = Session {
            id: Uuid::new_v4(),
            task: task.to_string(),
            project_path: project_path.to_path_buf(),
            agent_id: agent_id.to_string(),
            status: SessionStatus::Active,
            created_at: now,
            updated_at: now,
            constraint_snapshot: snapshot,
            handoffs: vec![],
        };
        self.db.upsert_session(&session)?;
        self.db.append_evidence(
            session.id,
            EvidenceKind::SessionStart,
            agent_id,
            serde_json::json!({
                "task": task,
                "snapshot_hash": session.constraint_snapshot.snapshot_hash,
                "rule_count": session.constraint_snapshot.rules.len(),
                "provenance_id": record.id,
            }),
        )?;
        Ok(session)
    }

    pub fn get_session(&self, id: Uuid) -> Result<Option<Session>> {
        self.db.get_session(id)
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        self.db.list_sessions()
    }

    pub fn set_session_status(&self, id: Uuid, status: SessionStatus) -> Result<()> {
        let mut session = self.db.get_session(id)?.context("session not found")?;
        session.status = status;
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)
    }

    pub fn record_evidence(
        &self,
        session_id: Uuid,
        kind: EvidenceKind,
        agent_id: &str,
        payload: serde_json::Value,
    ) -> Result<EvidenceEntry> {
        self.db
            .get_session(session_id)?
            .context("session not found")?;
        self.db.append_evidence(session_id, kind, agent_id, payload)
    }

    pub fn get_evidence(&self, session_id: Uuid) -> Result<Vec<EvidenceEntry>> {
        self.db.get_evidence(session_id)
    }

    pub fn verify_chain(&self, session_id: Uuid) -> Result<VerificationReport> {
        self.db.verify_chain(session_id)
    }

    pub fn evidence_head_hash(&self, session_id: Uuid) -> Result<String> {
        self.db.evidence_head_hash(session_id)
    }

    pub fn acknowledge_rules(
        &self,
        session_id: Uuid,
        agent_id: &str,
        rule_ids: &[String],
    ) -> Result<AcknowledgmentReport> {
        let session = self
            .db
            .get_session(session_id)?
            .context("session not found")?;
        let expected: Vec<String> = session
            .constraint_snapshot
            .rules
            .iter()
            .map(|r| r.id.clone())
            .collect();
        let acknowledged: Vec<String> = rule_ids
            .iter()
            .filter(|id| expected.contains(id))
            .cloned()
            .collect();
        let missing: Vec<String> = expected
            .iter()
            .filter(|id| !rule_ids.contains(id))
            .cloned()
            .collect();
        let unexpected: Vec<String> = rule_ids
            .iter()
            .filter(|id| !expected.contains(id))
            .cloned()
            .collect();
        let fully_compliant = missing.is_empty() && unexpected.is_empty();
        let report = AcknowledgmentReport {
            session_id,
            agent_id: agent_id.to_string(),
            acknowledged,
            missing,
            unexpected,
            fully_compliant,
        };
        self.db.append_evidence(
            session_id,
            EvidenceKind::RuleAcknowledgment,
            agent_id,
            serde_json::to_value(&report)?,
        )?;
        if !fully_compliant {
            self.db.append_evidence(
                session_id,
                EvidenceKind::ConstraintViolation,
                agent_id,
                serde_json::json!({
                    "reason": "rule acknowledgment mismatch",
                    "report": report,
                }),
            )?;
        }
        if let Some(handoff) = session.handoffs.last()
            && !handoff.acknowledged
            && fully_compliant
        {
            self.db.mark_handoff_acknowledged(session_id, handoff.id)?;
        }
        Ok(report)
    }

    pub fn export_handoff(
        &self,
        session_id: Uuid,
        to_agent: &str,
        context: serde_json::Value,
    ) -> Result<HandoffBundle> {
        let mut session = self
            .db
            .get_session(session_id)?
            .context("session not found")?;
        let head_hash = self.db.evidence_head_hash(session_id)?;
        let mut bundle = HandoffBundle {
            session_id,
            task: session.task.clone(),
            project_path: session.project_path.clone(),
            constraint_snapshot: session.constraint_snapshot.clone(),
            evidence_head_hash: head_hash,
            context,
            from_agent: session.agent_id.clone(),
            created_at: Utc::now(),
            bundle_hash: String::new(),
        };
        bundle.bundle_hash = bundle.compute_hash();
        let record = HandoffRecord {
            id: Uuid::new_v4(),
            from_agent: session.agent_id.clone(),
            to_agent: to_agent.to_string(),
            timestamp: bundle.created_at,
            bundle_hash: bundle.bundle_hash.clone(),
            acknowledged: false,
        };
        self.db.append_evidence(
            session_id,
            EvidenceKind::HandoffOut,
            &session.agent_id,
            serde_json::json!({
                "to_agent": to_agent,
                "bundle_hash": bundle.bundle_hash,
                "handoff_id": record.id,
            }),
        )?;
        session.handoffs.push(record);
        session.status = SessionStatus::HandedOff;
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;

        let bundle_path = self.layout.bundles_dir().join(format!("{session_id}.json"));
        std::fs::write(&bundle_path, serde_json::to_vec_pretty(&bundle)?)?;
        Ok(bundle)
    }

    pub fn import_handoff(&self, bundle: &HandoffBundle, new_agent: &str) -> Result<Session> {
        if !bundle.verify_integrity() {
            bail!("handoff bundle failed integrity verification");
        }
        let mut session = match self.db.get_session(bundle.session_id)? {
            Some(existing) => existing,
            None => Session {
                id: bundle.session_id,
                task: bundle.task.clone(),
                project_path: bundle.project_path.clone(),
                agent_id: new_agent.to_string(),
                status: SessionStatus::Active,
                created_at: bundle.created_at,
                updated_at: Utc::now(),
                constraint_snapshot: bundle.constraint_snapshot.clone(),
                handoffs: vec![],
            },
        };
        let snapshot_hash = canonical_hash(
            &serde_json::to_value(&session.constraint_snapshot.rules).unwrap_or_default(),
        );
        if snapshot_hash != bundle.constraint_snapshot.snapshot_hash {
            bail!(
                "constraint snapshot drift detected: local session rules differ from handoff bundle"
            );
        }
        session.agent_id = new_agent.to_string();
        session.status = SessionStatus::Active;
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;
        self.db.append_evidence(
            session.id,
            EvidenceKind::HandoffIn,
            new_agent,
            serde_json::json!({
                "from_agent": bundle.from_agent,
                "bundle_hash": bundle.bundle_hash,
                "evidence_head_hash": bundle.evidence_head_hash,
            }),
        )?;
        Ok(session)
    }

    pub fn get_provenance(&self, id: Uuid) -> Result<Option<ProvenanceRecord>> {
        self.db.get_provenance(id)
    }

    pub fn list_provenance(&self, limit: usize) -> Result<Vec<ProvenanceRecord>> {
        self.db.list_provenance(limit)
    }

    pub fn load_agents(&self) -> Result<Vec<AgentRegistration>> {
        let path = self.layout.agents_file();
        if !path.exists() {
            return Ok(default_agents());
        }
        let raw = std::fs::read_to_string(&path)?;
        Ok(serde_yaml::from_str(&raw)?)
    }

    pub fn save_agents(&self, agents: &[AgentRegistration]) -> Result<()> {
        let path = self.layout.agents_file();
        std::fs::write(&path, serde_yaml::to_string(agents)?)?;
        Ok(())
    }

    /// Onboard a project: normalize its path, dedup by path, and register it. Re-registering an
    /// existing path updates its name/description and marks it most-recently-opened.
    pub fn register_project(
        &self,
        name: &str,
        path: &Path,
        description: Option<String>,
    ) -> Result<Project> {
        let canonical = normalize_project_path(path)?;
        let key = canonical.to_string_lossy().to_string();
        if let Some(mut existing) = self.db.project_by_path(&key)? {
            if !name.trim().is_empty() {
                existing.name = name.trim().to_string();
            }
            if description.is_some() {
                existing.description = description;
            }
            existing.last_opened_at = Utc::now();
            self.db.upsert_project(&existing)?;
            return Ok(existing);
        }
        let display_name = if name.trim().is_empty() {
            canonical
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| key.clone())
        } else {
            name.trim().to_string()
        };
        let now = Utc::now();
        let project = Project {
            id: Uuid::new_v4(),
            name: display_name,
            path: canonical,
            description,
            created_at: now,
            last_opened_at: now,
        };
        self.db.upsert_project(&project)?;
        Ok(project)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        self.db.list_projects()
    }

    pub fn get_project(&self, id: Uuid) -> Result<Option<Project>> {
        self.db.get_project(id)
    }

    pub fn project_by_path(&self, path: &Path) -> Result<Option<Project>> {
        let key = normalize_project_path(path)?.to_string_lossy().to_string();
        self.db.project_by_path(&key)
    }

    pub fn touch_project(&self, id: Uuid) -> Result<()> {
        self.db.touch_project(id)
    }

    pub fn rename_project(&self, id: Uuid, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            bail!("project name cannot be empty");
        }
        let mut project = self.db.get_project(id)?.context("project not found")?;
        project.name = name.trim().to_string();
        self.db.upsert_project(&project)
    }

    /// Unregister a project from the console. Does NOT delete any files in the repository.
    pub fn remove_project(&self, id: Uuid) -> Result<bool> {
        self.db.delete_project(id)
    }

    /// Scaffold a project's contract directory plus a bilingual guide and copy-ready rule
    /// templates, then migrate previously scaffolded files to the current templates.
    ///
    /// Migration is hash-guarded by `.scaffold.json`: a generated file is refreshed only when its
    /// on-disk content still matches the hash we recorded when we wrote it (i.e. the user never
    /// edited it). User-modified files are always preserved. Legacy scaffolds with no marker are
    /// adopted only when their content already equals the current template. Returns the rules dir.
    pub fn init_project_contracts(&self, path: &Path) -> Result<PathBuf> {
        let rules_dir = DataLayout::project_rules_dir(path);
        std::fs::create_dir_all(&rules_dir)
            .with_context(|| format!("creating project rules dir {}", rules_dir.display()))?;
        let config_dir = DataLayout::project_config_dir(path);
        let examples_dir = DataLayout::project_examples_dir(path);
        std::fs::create_dir_all(&examples_dir)
            .with_context(|| format!("creating project examples dir {}", examples_dir.display()))?;

        let marker_path = DataLayout::project_scaffold_marker(path);
        let mut marker = read_scaffold_marker(&marker_path);

        for (rel, template) in [
            ("README.md", PROJECT_GUIDE),
            ("examples/contract-example.md", EXAMPLE_CONTRACT),
            ("examples/constraint-example.md", EXAMPLE_CONSTRAINT),
        ] {
            let file = config_dir.join(rel);
            let new_hash = sha256_hex(template.as_bytes());
            if !file.exists() {
                std::fs::write(&file, template)
                    .with_context(|| format!("writing scaffold file {}", file.display()))?;
                marker.files.insert(rel.to_string(), new_hash);
                continue;
            }
            let current_hash = sha256_hex(
                &std::fs::read(&file)
                    .with_context(|| format!("reading scaffold file {}", file.display()))?,
            );
            match marker.files.get(rel) {
                // We wrote it and it is untouched: refresh when the template has changed.
                Some(recorded) if *recorded == current_hash => {
                    if current_hash != new_hash {
                        std::fs::write(&file, template).with_context(|| {
                            format!("migrating scaffold file {}", file.display())
                        })?;
                    }
                    marker.files.insert(rel.to_string(), new_hash);
                }
                // Legacy unmodified scaffold (no marker yet): adopt it for future upgrades.
                None if current_hash == new_hash => {
                    marker.files.insert(rel.to_string(), new_hash);
                }
                // User-edited or unknown older content: preserve, stop managing it.
                _ => {
                    marker.files.remove(rel);
                }
            }
        }

        marker.version = TEMPLATE_VERSION;
        write_scaffold_marker(&marker_path, &marker)?;
        Ok(rules_dir)
    }

    /// Queue an agent-proposed rule for human review. The draft is stored in SQLite as `pending`;
    /// nothing is written to the project until a human approves it.
    pub fn submit_suggestion(&self, mut suggestion: RuleSuggestion) -> Result<RuleSuggestion> {
        if suggestion.rule_id.trim().is_empty() {
            bail!("suggestion rule id cannot be empty");
        }
        if suggestion.title.trim().is_empty() {
            bail!("suggestion title cannot be empty");
        }
        if suggestion.content.trim().is_empty() {
            bail!("suggestion content cannot be empty");
        }
        match suggestion.scope {
            Scope::User => suggestion.project_path = PathBuf::new(),
            Scope::Project => {
                suggestion.project_path = normalize_project_path(&suggestion.project_path)?;
            }
            Scope::Session => bail!("session scope cannot be suggested as a durable rule"),
        }
        let now = Utc::now();
        suggestion.id = Uuid::new_v4();
        suggestion.status = SuggestionStatus::Pending;
        suggestion.created_at = now;
        suggestion.updated_at = now;
        suggestion.reviewed_at = None;
        suggestion.review_note = None;
        suggestion.approved_rule_id = None;
        self.db.insert_suggestion(&suggestion)?;
        Ok(suggestion)
    }

    pub fn list_suggestions(
        &self,
        project_path: Option<&Path>,
        status: Option<SuggestionStatus>,
    ) -> Result<Vec<RuleSuggestion>> {
        let key = project_path.map(|p| {
            normalize_project_path(p)
                .unwrap_or_else(|_| p.to_path_buf())
                .to_string_lossy()
                .to_string()
        });
        self.db.list_suggestions(key.as_deref(), status)
    }

    pub fn count_suggestions(
        &self,
        project_path: Option<&Path>,
        status: Option<SuggestionStatus>,
    ) -> Result<usize> {
        let key = project_path.map(|p| {
            normalize_project_path(p)
                .unwrap_or_else(|_| p.to_path_buf())
                .to_string_lossy()
                .to_string()
        });
        self.db.count_suggestions(key.as_deref(), status)
    }

    pub fn get_suggestion(&self, id: Uuid) -> Result<Option<RuleSuggestion>> {
        self.db.get_suggestion(id)
    }

    /// Approve a pending suggestion, applying optional human edits, and write it as a binding
    /// project rule (via `upsert_rule`, so an existing rule id gets a version bump). Returns the
    /// reviewed suggestion, the saved rule, and its file path.
    pub fn approve_suggestion(
        &self,
        id: Uuid,
        edits: SuggestionEdits,
        note: Option<String>,
    ) -> Result<(RuleSuggestion, Rule, PathBuf)> {
        let mut suggestion = self
            .db
            .get_suggestion(id)?
            .context("suggestion not found")?;
        if suggestion.status != SuggestionStatus::Pending {
            bail!("suggestion already {}", suggestion.status);
        }
        let rule_id = edits.rule_id.unwrap_or_else(|| suggestion.rule_id.clone());
        let title = edits.title.unwrap_or_else(|| suggestion.title.clone());
        let content = edits.content.unwrap_or_else(|| suggestion.content.clone());
        if rule_id.trim().is_empty() || title.trim().is_empty() || content.trim().is_empty() {
            bail!("approved rule requires a non-empty id, title and content");
        }
        let kind = edits.kind.unwrap_or(suggestion.kind);
        let mut rule = new_rule(
            &rule_id,
            &title,
            &content,
            kind,
            suggestion.scope,
            Some(suggestion.agent_id.clone()),
        );
        rule.topic = edits.topic.or_else(|| suggestion.topic.clone());
        rule.tags = edits.tags.unwrap_or_else(|| suggestion.tags.clone());
        rule.source.origin = Some("agent-suggestion".to_string());
        let project_path = match suggestion.scope {
            Scope::User => None,
            Scope::Project => {
                if suggestion.project_path.as_os_str().is_empty() {
                    bail!("project-scope suggestion has no project path");
                }
                Some(suggestion.project_path.clone())
            }
            Scope::Session => bail!("session scope cannot be approved as a durable rule"),
        };
        let (saved, path) = self.upsert_rule(&rule, project_path.as_deref())?;

        let now = Utc::now();
        suggestion.status = SuggestionStatus::Approved;
        suggestion.reviewed_at = Some(now);
        suggestion.updated_at = now;
        suggestion.review_note = note.filter(|n| !n.trim().is_empty());
        suggestion.approved_rule_id = Some(saved.id.clone());
        self.db.update_suggestion(&suggestion)?;
        Ok((suggestion, saved, path))
    }

    /// Reject a pending suggestion with an optional reason. No rule file is written.
    pub fn reject_suggestion(&self, id: Uuid, reason: Option<String>) -> Result<RuleSuggestion> {
        let mut suggestion = self
            .db
            .get_suggestion(id)?
            .context("suggestion not found")?;
        if suggestion.status != SuggestionStatus::Pending {
            bail!("suggestion already {}", suggestion.status);
        }
        let now = Utc::now();
        suggestion.status = SuggestionStatus::Rejected;
        suggestion.reviewed_at = Some(now);
        suggestion.updated_at = now;
        suggestion.review_note = reason.filter(|n| !n.trim().is_empty());
        self.db.update_suggestion(&suggestion)?;
        Ok(suggestion)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuleExplanation {
    pub rule: Rule,
    pub decision_trace: Option<crate::model::RuleMatch>,
    pub provenance_id: Uuid,
    pub conflicts: Vec<crate::model::Conflict>,
}

fn default_agents() -> Vec<AgentRegistration> {
    vec![
        AgentRegistration {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            command: "claude-code-acp".into(),
            args: vec![],
            env: Default::default(),
            protocol: Some(crate::model::AgentProtocol::Acp),
        },
        AgentRegistration {
            id: "gemini-cli".into(),
            name: "Gemini CLI".into(),
            command: "gemini".into(),
            args: vec!["--experimental-acp".into()],
            env: Default::default(),
            protocol: Some(crate::model::AgentProtocol::Acp),
        },
    ]
}

pub fn new_user_rule(id: &str, title: &str, content: &str) -> Rule {
    new_rule(
        id,
        title,
        content,
        crate::model::RuleKind::Preference,
        Scope::User,
        None,
    )
}

/// Build a fresh rule with sensible defaults. `author` records who/what created it (e.g. an agent
/// id); the source kind follows the scope. Callers may override priority, tags, topic, etc.
pub fn new_rule(
    id: &str,
    title: &str,
    content: &str,
    kind: crate::model::RuleKind,
    scope: Scope,
    author: Option<String>,
) -> Rule {
    let source_kind = match scope {
        Scope::User => SourceKind::User,
        Scope::Project => SourceKind::Project,
        Scope::Session => SourceKind::Imported,
    };
    Rule {
        id: id.to_string(),
        version: 1,
        title: title.to_string(),
        topic: None,
        content: content.to_string(),
        kind,
        scope,
        source: files::new_rule_source(source_kind, author),
        priority: 100,
        applies_to: Default::default(),
        tags: vec![],
        conflicts_with: vec![],
        code_checks: vec![],
        enabled: true,
        checksum: String::new(),
    }
    .with_checksum()
}

/// Build a fresh pending suggestion draft. Callers may set `topic`, `tags` and `rationale` before
/// handing it to `Store::submit_suggestion`, which assigns the id and timestamps.
pub fn new_suggestion(
    project_path: &Path,
    agent_id: &str,
    rule_id: &str,
    title: &str,
    content: &str,
    kind: RuleKind,
    scope: Scope,
) -> RuleSuggestion {
    let now = Utc::now();
    RuleSuggestion {
        id: Uuid::nil(),
        project_path: project_path.to_path_buf(),
        scope,
        agent_id: agent_id.to_string(),
        rule_id: rule_id.to_string(),
        title: title.to_string(),
        topic: None,
        content: content.to_string(),
        kind,
        tags: vec![],
        rationale: None,
        status: SuggestionStatus::Pending,
        created_at: now,
        updated_at: now,
        reviewed_at: None,
        review_note: None,
        approved_rule_id: None,
    }
}

/// Resolve a project path to a stable, display-friendly absolute key. The path must exist and be
/// a directory. On Windows the `\\?\` verbatim prefix added by `canonicalize` is stripped so the
/// stored path matches what users type and what sessions record.
fn normalize_project_path(path: &Path) -> Result<PathBuf> {
    let canonical = path.canonicalize().with_context(|| {
        format!(
            "project path does not exist or is inaccessible: {}",
            path.display()
        )
    })?;
    if !canonical.is_dir() {
        bail!("project path is not a directory: {}", canonical.display());
    }
    Ok(strip_verbatim_prefix(canonical))
}

#[cfg(windows)]
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", rest))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

/// Bump when the scaffold templates below change so untouched projects migrate on next init.
const TEMPLATE_VERSION: u32 = 1;

/// Records the template version and the sha256 of each generated file's content at the time we
/// wrote it. A file is only auto-refreshed while its on-disk hash still equals the recorded one.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ScaffoldMarker {
    version: u32,
    files: BTreeMap<String, String>,
}

impl Default for ScaffoldMarker {
    fn default() -> Self {
        Self {
            version: TEMPLATE_VERSION,
            files: BTreeMap::new(),
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// A missing or corrupt marker is treated as "no prior scaffold" (legacy): existing files are
/// preserved unless they already match the current template.
fn read_scaffold_marker(path: &Path) -> ScaffoldMarker {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_scaffold_marker(path: &Path, marker: &ScaffoldMarker) -> Result<()> {
    let text = serde_json::to_string_pretty(marker).context("serializing scaffold marker")?;
    std::fs::write(path, text)
        .with_context(|| format!("writing scaffold marker {}", path.display()))
}

const PROJECT_GUIDE: &str = "\
# Project contracts / 项目契约

This directory (`.agent-control/`) holds this project's engineering contracts.
本目录（`.agent-control/`）保存本项目的工程契约。

- `rules/` — project-scope rule files (Markdown + YAML frontmatter). Every agent working in this
  repo inherits them; commit and review them like code.
  `rules/` — 项目作用域规则文件（Markdown + YAML frontmatter）。仓库内所有代理都会继承，请像代码一样提交与评审。
- `examples/` — copy-ready rule templates (NOT binding). Copy one into `rules/` and edit it to
  activate a contract. See `contract-example.md` and `constraint-example.md`.
  `examples/` — 可复制的规则模板（不会生效）。复制到 `rules/` 并修改即可启用一条契约。
- Personal preferences are user-scope and live separately in `~/.agent-control/rules/user/`; they
  follow you across projects and are NOT stored here.
  个人偏好为用户级，单独存于 `~/.agent-control/rules/user/`，跨项目跟随你，不保存在此。

Agents discover these rules over MCP (`get_applicable_rules`) and freeze them into a session
snapshot (`create_session`) so the same constraints stay binding across harness handoffs.
代理通过 MCP（`get_applicable_rules`）发现这些规则，并在会话快照（`create_session`）中冻结，
确保跨 harness 交接后同一约束仍然生效。

Agents can also record a newly discovered project contract actively via the MCP `save_rule` tool
(id, title, content, kind, scope=project, project_path); it writes a file into `rules/` that shows
up in the console and binds future sessions.
代理也可通过 MCP `save_rule` 工具主动记录新发现的项目契约，写入 `rules/` 后即出现在指挥台并约束后续会话。
";

const EXAMPLE_CONTRACT: &str = "\
---
# Stable unique id. Reusing an id updates the rule and bumps its version.
id: upstream-api-isasc-type
title: Upstream isAsc parameter has an ambiguous type
# preference | contract | constraint
kind: contract
# Files in rules/ are always project-scope regardless of this value.
scope: project
topic: api-integration
source:
  # user | project | imported | inferred
  kind: project
  author: your-team
  created_at: 2026-01-01T00:00:00Z
# Higher priority wins on conflict (default 100).
priority: 100
tags: [api, upstream]
---
The upstream API's `isAsc` parameter is documented inconsistently: some endpoints accept a
boolean (`true`/`false`), others accept a string (`\"true\"`/`\"false\"`).

Before calling any upstream sorting endpoint:
1. Confirm the accepted type for that specific endpoint (check its schema or tests).
2. Normalize the value explicitly; never rely on implicit coercion.
3. Record the confirmed contract here so other agents inherit it.

上游 API 的 `isAsc` 参数类型不确定：部分接口接收 bool，部分接收 string。
调用任何上游排序接口前，先确认该接口接受的类型并显式归一化，不要依赖隐式转换。
";

const EXAMPLE_CONSTRAINT: &str = "\
---
id: no-secrets-in-source
title: Never commit secrets to source control
kind: constraint
scope: project
topic: security
source:
  kind: project
  author: your-team
  created_at: 2026-01-01T00:00:00Z
priority: 200
tags: [security]
---
Hard constraints for every agent working in this repo:

- Do not hardcode credentials, API keys, tokens, or connection strings in source. Read them from
  environment variables or a local, git-ignored config file.
- Run the test suite before declaring a change complete.
- If a task conflicts with this rule, stop and surface the conflict to the user rather than
  silently weakening the rule.

硬性约束：禁止在源码中硬编码任何密钥/令牌；完成改动前必须运行测试；
任务与本规则冲突时，先向用户暴露冲突，而不是悄悄削弱规则。
";
