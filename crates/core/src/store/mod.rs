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
use crate::export;
use crate::hooks::{self, HookReport, HookStatus, HookTarget, SetupReport};
use crate::model::{
    AgentRegistration, AssuranceTier, ConstraintAssurance, ConstraintSnapshot, Enforcement,
    EvidenceEntry, EvidenceKind, HandoffBundle, HandoffRecord, OwnershipSpan, Project,
    ProvenanceRecord, QueryContext, Resolution, Rule, RuleKind, RuleSuggestion, Scope, Session,
    SessionStatus, SourceKind, SuggestionEdits, SuggestionStatus, ToolGateReport,
    VerificationReport, WorkItem, canonical_hash,
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

/// Result of post-hoc constraint verification: the achieved assurance tier per rule plus counts.
/// Distinct from `VerificationReport` (evidence-chain integrity). `Enforced` is never produced here;
/// it requires a chokepoint outside the model's decision loop.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConstraintVerificationReport {
    pub session_id: Uuid,
    pub agent_id: String,
    pub assurances: Vec<ConstraintAssurance>,
    pub verified: usize,
    pub advisory: usize,
    pub enforced: usize,
}

impl Store {
    pub fn open(layout: DataLayout) -> Result<Self> {
        layout.ensure_dirs()?;
        let db = Db::open(&layout.db_path())?;
        let store = Self { layout, db };
        store.rebuild_search_index()?;
        Ok(store)
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
            let env_rules = files::load_rules_from_dir(
                &DataLayout::project_environment_dir(project),
                Scope::Environment,
            )?;
            rules.extend(env_rules);
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
            Scope::Environment => {
                let project = project_path.context("environment scope requires a project path")?;
                ensure_environment_dir(project)?
            }
            Scope::Session => {
                bail!("session rules are stored in the session snapshot, not as files")
            }
        };
        let mut rule = rule.clone().with_checksum();
        rule.source.file_path = None;
        let path = files::write_rule_file(&dir, &rule)?;
        let index_path = match rule.scope {
            Scope::User => String::new(),
            _ => project_path
                .map(|p| {
                    p.canonicalize()
                        .unwrap_or_else(|_| p.to_path_buf())
                        .display()
                        .to_string()
                })
                .unwrap_or_default(),
        };
        self.db.search_sync_rule(&rule, &index_path)?;
        Ok(path)
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
            // Enablement is its own axis: a content update must not silently re-enable a rule
            // the user disabled. Only `set_rule_enabled` flips it.
            rule.enabled = existing.enabled;
        }
        let path = self.save_rule(&rule, project_path)?;
        Ok((rule.with_checksum(), path))
    }

    /// Enable or disable a rule in place, preserving its content and provenance. This is the only
    /// write path for `enabled` (`save_rule`/`upsert_rule` never flip it). Bumps the version like
    /// any other persisted edit and returns the updated rule with its recomputed checksum.
    pub fn set_rule_enabled(
        &self,
        rule_id: &str,
        scope: Scope,
        project_path: Option<&Path>,
        enabled: bool,
    ) -> Result<Rule> {
        let dir = match scope {
            Scope::User => self.layout.user_rules_dir(),
            Scope::Project => {
                let project = project_path.context("project scope requires a project path")?;
                DataLayout::project_rules_dir(project)
            }
            Scope::Environment => {
                let project = project_path.context("environment scope requires a project path")?;
                DataLayout::project_environment_dir(project)
            }
            Scope::Session => bail!("session rules cannot be toggled as files"),
        };
        let mut rule = files::load_rules_from_dir(&dir, scope)?
            .into_iter()
            .find(|r| r.id == rule_id)
            .with_context(|| format!("rule '{rule_id}' not found in {}", dir.display()))?;
        rule.enabled = enabled;
        rule.version += 1;
        self.save_rule(&rule, project_path)?;
        Ok(rule.with_checksum())
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
            Scope::Environment => {
                let project = project_path.context("environment scope requires a project path")?;
                DataLayout::project_environment_dir(project)
            }
            Scope::Session => bail!("session rules cannot be deleted as files"),
        };
        let deleted = files::delete_rule_file(&dir, rule_id)?;
        if deleted {
            self.db.search_remove_rule(rule_id)?;
        }
        Ok(deleted)
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

    /// Get-or-create: an **active** session with the same project + agent + task is returned
    /// as-is (`reused = true`) so repeated calls from one harness conversation never pile up
    /// duplicate sessions. A new task (or a closed source session) freezes a fresh snapshot.
    pub fn create_session(
        &self,
        task: &str,
        project_path: &Path,
        agent_id: &str,
        task_tags: Vec<String>,
        harness_session_id: Option<String>,
    ) -> Result<(Session, bool)> {
        let all = self.db.list_sessions()?;
        if let Some(existing) = all.iter().find(|s| {
            s.status == SessionStatus::Active
                && s.agent_id == agent_id
                && s.project_path == project_path
                && s.task == task
        }) {
            return Ok((existing.clone(), true));
        }
        // A closed same-(project, agent, task) session means this is a chain split, not a duplicate.
        let predecessor = all
            .iter()
            .filter(|s| {
                s.status != SessionStatus::Active
                    && s.agent_id == agent_id
                    && s.project_path == project_path
                    && s.task == task
            })
            .max_by_key(|s| s.created_at)
            .map(|s| s.id);
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
            assurance: vec![],
            goal: String::new(),
            work_items: vec![],
            harness_session_id,
            predecessor,
            title: String::new(),
        };
        self.db.upsert_session(&session)?;
        // The snapshot resolution ran before the session id existed; link its provenance now (UX2).
        self.db.set_provenance_session(record.id, session.id)?;
        self.db.append_evidence(
            session.id,
            EvidenceKind::SessionStart,
            agent_id,
            serde_json::json!({
                "task": task,
                "snapshot_hash": session.constraint_snapshot.snapshot_hash,
                "rule_count": session.constraint_snapshot.rules.len(),
                "provenance_id": record.id,
                "harness_session_id": session.harness_session_id,
                "predecessor": session.predecessor,
            }),
        )?;
        Ok((session, false))
    }

    pub fn get_session(&self, id: Uuid) -> Result<Option<Session>> {
        self.db.get_session(id)
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        self.db.list_sessions()
    }

    /// Close or reopen a session. Every transition appends a `status_change` evidence entry carrying
    /// the ACTING agent (caller-supplied), and closing additionally appends `session_end` so the
    /// lifecycle change stays auditable; the chain itself is never deleted.
    pub fn set_session_status(
        &self,
        id: Uuid,
        status: SessionStatus,
        acting_agent_id: &str,
    ) -> Result<Session> {
        let mut session = self.db.get_session(id)?.context("session not found")?;
        session.status = status;
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;
        self.db.append_evidence(
            id,
            EvidenceKind::StatusChange,
            acting_agent_id,
            serde_json::json!({
                "status": db::status_label(status),
                "harness_session_id": session.harness_session_id,
            }),
        )?;
        if matches!(status, SessionStatus::Completed | SessionStatus::Failed) {
            self.db.append_evidence(
                id,
                EvidenceKind::SessionEnd,
                acting_agent_id,
                serde_json::json!({ "status": db::status_label(status) }),
            )?;
            // UX6: closing auto-exports the handoff artifacts so a manual export is redundant;
            // the closed status is preserved and the closer is the acting identity.
            self.record_handoff(
                &mut session,
                None,
                serde_json::json!({ "auto": "session_close" }),
                acting_agent_id,
                None,
            )?;
        }
        Ok(session)
    }

    /// Record the resumable work state (goal + done/doing/todo) so a handoff carries progress
    /// independent of any harness's own todo list. Appends a `work_state_update` evidence entry
    /// carrying the ACTING agent (caller-supplied), so every edit leaves an attributable trail.
    pub fn set_session_work_state(
        &self,
        id: Uuid,
        goal: String,
        work_items: Vec<WorkItem>,
        acting_agent_id: &str,
    ) -> Result<Session> {
        let mut session = self.db.get_session(id)?.context("session not found")?;
        session.goal = goal;
        session.work_items = work_items;
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;
        self.db.append_evidence(
            id,
            EvidenceKind::WorkStateUpdate,
            acting_agent_id,
            serde_json::json!({
                "goal": session.goal,
                "work_items": session.work_items,
                "harness_session_id": session.harness_session_id,
            }),
        )?;
        Ok(session)
    }

    /// Rename the human-facing title only (UX7). `task` stays the machine identity key for
    /// get-or-create / hooks / bundles; the rename is audited with the acting identity.
    pub fn set_session_title(
        &self,
        id: Uuid,
        title: &str,
        acting_agent_id: &str,
    ) -> Result<Session> {
        let mut session = self.db.get_session(id)?.context("session not found")?;
        let from = session.title.clone();
        session.title = title.trim().to_string();
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;
        self.db.append_evidence(
            id,
            EvidenceKind::SessionRename,
            acting_agent_id,
            serde_json::json!({ "from": from, "to": session.title }),
        )?;
        Ok(session)
    }

    /// Derive the ownership history from the evidence chain: the SessionStart agent opens the first
    /// span; each HandoffIn / OwnershipChange closes the current span and opens a new one. The last
    /// span stays open (`until: None`) and matches `Session.agent_id` (the current owner).
    pub fn ownership_timeline(&self, id: Uuid) -> Result<Vec<OwnershipSpan>> {
        let session = self.db.get_session(id)?.context("session not found")?;
        let entries = self.db.get_evidence(id)?;
        let mut spans: Vec<OwnershipSpan> = Vec::new();
        for e in &entries {
            let via = match e.kind {
                EvidenceKind::SessionStart => "session_start",
                EvidenceKind::HandoffIn => "handoff_in",
                EvidenceKind::OwnershipChange => "ownership_change",
                _ => continue,
            };
            let agent = match e.kind {
                EvidenceKind::OwnershipChange => e
                    .payload
                    .get("to")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&e.agent_id)
                    .to_string(),
                _ => e.agent_id.clone(),
            };
            if let Some(last) = spans.last_mut() {
                if last.agent_id == agent {
                    continue;
                }
                last.until = Some(e.timestamp);
            }
            spans.push(OwnershipSpan {
                agent_id: agent,
                since: e.timestamp,
                until: None,
                via: via.to_string(),
            });
        }
        if spans.is_empty() {
            spans.push(OwnershipSpan {
                agent_id: session.agent_id.clone(),
                since: session.created_at,
                until: None,
                via: "session_start".to_string(),
            });
        }
        Ok(spans)
    }

    /// Delete a session and, cascade, its evidence chain and provenance record.
    pub fn delete_session(&self, id: Uuid) -> Result<()> {
        self.db.delete_session(id)
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

    /// Full-text search over rules, sessions and evidence. The query goes through the same
    /// hybrid segmenter as the index (CJK bigrams), so Chinese queries of two characters or
    /// more match. `kind` and `project_path` narrow the result; an empty query returns nothing.
    pub fn search(
        &self,
        query: &str,
        kind: Option<crate::search::SearchKind>,
        project_path: Option<&Path>,
        limit: usize,
    ) -> Result<Vec<crate::search::SearchHit>> {
        let tokens = crate::search::tokenize(query);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let expr = crate::search::match_expression(&tokens);
        let project = project_path.map(|p| {
            p.canonicalize()
                .unwrap_or_else(|_| p.to_path_buf())
                .display()
                .to_string()
        });
        let mut hits = self
            .db
            .search_query(&expr, kind, project.as_deref(), limit.max(1))?;
        for hit in &mut hits {
            hit.snippet = crate::search::display_snippet(&hit.snippet);
        }
        Ok(hits)
    }

    /// Rules live in git-friendly files, so their shadow rows are rebuilt from disk on open;
    /// sessions and evidence are rebuilt from the DB so older stores gain a populated index.
    fn rebuild_search_index(&self) -> Result<()> {
        self.db.search_clear()?;
        for rule in self.load_rules(None)? {
            self.db.search_sync_rule(&rule, "")?;
        }
        for project in self.db.list_projects()? {
            for rule in self.load_rules(Some(&project.path))? {
                if rule.scope == Scope::User {
                    continue;
                }
                self.db
                    .search_sync_rule(&rule, &project.path.display().to_string())?;
            }
        }
        for session in self.db.list_sessions()? {
            self.db.search_sync_session(&session)?;
            for entry in self.db.get_evidence(session.id)? {
                self.db
                    .search_sync_evidence(&entry, &session.project_path.display().to_string())?;
            }
        }
        Ok(())
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

    /// Evaluate the session's frozen constraints against the real repo and record the achieved
    /// assurance tier per rule. This is post-hoc verification (the `Verified` tier), distinct from
    /// `verify_chain` which checks evidence integrity. A rule reaches `Verified` only when it has
    /// `code_checks` and all of them pass; otherwise it stays `Advisory`. `Enforced` requires a
    /// chokepoint and is never produced here. Results are appended to the chain and persisted.
    pub fn verify_constraints(
        &self,
        session_id: Uuid,
        agent_id: &str,
    ) -> Result<ConstraintVerificationReport> {
        use crate::conflict::{CheckStatus, evaluate_code_checks};

        let mut session = self
            .db
            .get_session(session_id)?
            .context("session not found")?;
        let mut assurances = Vec::new();
        let mut checks_payload = Vec::new();
        let mut violations = Vec::new();
        for rule in &session.constraint_snapshot.rules {
            let results = evaluate_code_checks(rule, &session.project_path);
            let (achieved, basis) = if results.is_empty() {
                (
                    AssuranceTier::Advisory,
                    "no mechanical check; semantic rule stays advisory".to_string(),
                )
            } else if results.iter().all(|r| r.status == CheckStatus::Pass) {
                (
                    AssuranceTier::Verified,
                    format!("{} code_check(s) passed", results.len()),
                )
            } else {
                let failed = results
                    .iter()
                    .filter(|r| r.status != CheckStatus::Pass)
                    .count();
                (
                    AssuranceTier::Advisory,
                    format!(
                        "{failed} of {} code_check(s) failed or inconclusive",
                        results.len()
                    ),
                )
            };
            assurances.push(ConstraintAssurance {
                rule_id: rule.id.clone(),
                declared: rule.enforcement,
                achieved,
                basis,
            });
            if !results.is_empty() {
                checks_payload.push(serde_json::json!({ "rule_id": rule.id, "checks": results }));
            }
            for r in &results {
                if r.status != CheckStatus::Pass {
                    violations.push(serde_json::json!({ "rule_id": rule.id, "check": r }));
                }
            }
        }
        session.assurance = assurances.clone();
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;
        self.db.append_evidence(
            session_id,
            EvidenceKind::TestResult,
            agent_id,
            serde_json::json!({ "constraint_checks": checks_payload, "assurance": assurances }),
        )?;
        if !violations.is_empty() {
            self.db.append_evidence(
                session_id,
                EvidenceKind::ConstraintViolation,
                agent_id,
                serde_json::json!({
                    "reason": "constraint verification failed",
                    "violations": violations,
                }),
            )?;
        }
        let verified = assurances
            .iter()
            .filter(|a| a.achieved == AssuranceTier::Verified)
            .count();
        let advisory = assurances
            .iter()
            .filter(|a| a.achieved == AssuranceTier::Advisory)
            .count();
        let enforced = assurances
            .iter()
            .filter(|a| a.achieved == AssuranceTier::Enforced)
            .count();
        Ok(ConstraintVerificationReport {
            session_id,
            agent_id: agent_id.to_string(),
            assurances,
            verified,
            advisory,
            enforced,
        })
    }

    /// The deny-capable PreToolUse chokepoint. Resolves the enabled `Hard` rules for the project,
    /// evaluates the repo gate (`code_checks`) plus `tool_guards` for this call, and — when a session
    /// is given — records the verdict in the evidence chain and marks mediated hard rules `Enforced`.
    /// This is the only path that produces the `Enforced` tier; declaring `Hard` alone never enforces.
    /// With no hard rules the call is allowed (nothing to enforce).
    pub fn check_tool_call(
        &self,
        project_path: &Path,
        tool_name: &str,
        tool_input: &serde_json::Value,
        agent_id: &str,
        session_id: Option<Uuid>,
    ) -> Result<ToolGateReport> {
        let hard_rules: Vec<Rule> = self
            .load_rules(Some(project_path))?
            .into_iter()
            .filter(|r| r.enabled && r.enforcement == Enforcement::Hard)
            .filter(|r| {
                r.applies_to.agents.is_empty() || r.applies_to.agents.iter().any(|a| a == agent_id)
            })
            .collect();
        let report = crate::gate::evaluate_gate(&hard_rules, project_path, tool_name, tool_input);

        if let Some(session_id) = session_id {
            self.db.append_evidence(
                session_id,
                EvidenceKind::ToolGate,
                agent_id,
                serde_json::json!({
                    "tool_name": tool_name,
                    "allowed": report.allowed,
                    "denials": report.denials,
                    "enforced_rules": report.enforced_rules,
                }),
            )?;
            self.record_gate_assurance(session_id, &report)?;
            if !report.allowed {
                self.db.append_evidence(
                    session_id,
                    EvidenceKind::ConstraintViolation,
                    agent_id,
                    serde_json::json!({
                        "reason": "tool gate denied call",
                        "tool_name": tool_name,
                        "denials": report.denials,
                    }),
                )?;
            }
        }
        Ok(report)
    }

    /// Upsert the assurance entries for the hard rules the gate touched: mediated-and-allowed rules
    /// reach `Enforced`; denied rules drop to `Advisory`. Non-hard rules' entries are preserved.
    fn record_gate_assurance(&self, session_id: Uuid, report: &ToolGateReport) -> Result<()> {
        let Some(mut session) = self.db.get_session(session_id)? else {
            return Ok(());
        };
        let declared: std::collections::HashMap<String, Enforcement> = session
            .constraint_snapshot
            .rules
            .iter()
            .map(|r| (r.id.clone(), r.enforcement))
            .collect();
        let mut updates: Vec<(String, AssuranceTier, String)> = Vec::new();
        for id in &report.enforced_rules {
            updates.push((
                id.clone(),
                AssuranceTier::Enforced,
                format!("mediated by PreToolUse gate on '{}'", report.tool_name),
            ));
        }
        for denial in &report.denials {
            updates.push((
                denial.rule_id.clone(),
                AssuranceTier::Advisory,
                format!("tool gate denied: {}", denial.reason),
            ));
        }
        for (id, achieved, basis) in updates {
            let decl = declared.get(&id).copied().unwrap_or(Enforcement::Hard);
            if let Some(entry) = session.assurance.iter_mut().find(|a| a.rule_id == id) {
                entry.declared = decl;
                entry.achieved = achieved;
                entry.basis = basis;
            } else {
                session.assurance.push(ConstraintAssurance {
                    rule_id: id,
                    declared: decl,
                    achieved,
                    basis,
                });
            }
        }
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;
        Ok(())
    }

    pub fn export_handoff(
        &self,
        session_id: Uuid,
        to_agent: Option<&str>,
        context: serde_json::Value,
    ) -> Result<HandoffBundle> {
        let mut session = self
            .db
            .get_session(session_id)?
            .context("session not found")?;
        let acting = session.agent_id.clone();
        self.record_handoff(
            &mut session,
            to_agent,
            context,
            &acting,
            Some(SessionStatus::HandedOff),
        )
    }

    /// Write the handoff bundle + project-local brief and record the handoff, superseding every
    /// still-unacknowledged prior record. `status_after` flips the session only when asked:
    /// `export_handoff` passes `HandedOff`, while the auto-export on close (UX6) keeps the
    /// closed status so finished work stays out of the active list.
    fn record_handoff(
        &self,
        session: &mut Session,
        to_agent: Option<&str>,
        context: serde_json::Value,
        acting_agent_id: &str,
        status_after: Option<SessionStatus>,
    ) -> Result<HandoffBundle> {
        let head_hash = self.db.evidence_head_hash(session.id)?;
        let mut bundle = HandoffBundle {
            session_id: session.id,
            task: session.task.clone(),
            title: session.title.clone(),
            project_path: session.project_path.clone(),
            constraint_snapshot: session.constraint_snapshot.clone(),
            assurance: session.assurance.clone(),
            goal: session.goal.clone(),
            work_items: session.work_items.clone(),
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
            to_agent: to_agent.map(str::to_string),
            timestamp: bundle.created_at,
            bundle_hash: bundle.bundle_hash.clone(),
            acknowledged: false,
            superseded: false,
        };
        self.db.append_evidence(
            session.id,
            EvidenceKind::HandoffOut,
            acting_agent_id,
            serde_json::json!({
                "to_agent": to_agent,
                "bundle_hash": bundle.bundle_hash,
                "handoff_id": record.id,
            }),
        )?;
        if let Some(status) = status_after {
            self.db.append_evidence(
                session.id,
                EvidenceKind::StatusChange,
                acting_agent_id,
                serde_json::json!({ "status": "handed_off", "handoff_id": record.id }),
            )?;
            session.status = status;
        }
        // A newer export replaces every still-unacknowledged prior record; only the latest
        // handoff can be acknowledged, so older ones must not read as pending forever.
        for handoff in session.handoffs.iter_mut() {
            if !handoff.acknowledged {
                handoff.superseded = true;
            }
        }
        session.handoffs.push(record);
        session.updated_at = Utc::now();
        self.db.upsert_session(session)?;

        let bundle_path = self
            .layout
            .bundles_dir()
            .join(format!("{}.json", session.id));
        std::fs::write(&bundle_path, serde_json::to_vec_pretty(&bundle)?)?;

        // Project-local continuity brief: any harness reads this file to inherit the handoff without
        // an MCP call (the AGENTS.md managed block points here).
        let latest_path = DataLayout::project_handoff_latest(&bundle.project_path);
        if let Some(parent) = latest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&latest_path, export::render_handoff_brief(&bundle, None))?;
        Ok(bundle)
    }

    pub fn import_handoff(
        &self,
        bundle: &HandoffBundle,
        new_agent: &str,
        harness_session_id: Option<String>,
    ) -> Result<Session> {
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
                assurance: vec![],
                goal: bundle.goal.clone(),
                work_items: bundle.work_items.clone(),
                harness_session_id: None,
                predecessor: None,
                title: bundle.title.clone(),
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
        // Carry the source harness's achieved tiers as the incoming baseline; the new harness must
        // re-run verify_constraints to sustain them (a harness without chokepoints cannot keep Enforced).
        session.assurance = bundle.assurance.clone();
        session.goal = bundle.goal.clone();
        session.work_items = bundle.work_items.clone();
        let previous_agent = session.agent_id.clone();
        session.agent_id = new_agent.to_string();
        if harness_session_id.is_some() {
            session.harness_session_id = harness_session_id;
        }
        session.status = SessionStatus::Active;
        session.updated_at = Utc::now();
        self.db.upsert_session(&session)?;
        if previous_agent != new_agent {
            self.db.append_evidence(
                session.id,
                EvidenceKind::OwnershipChange,
                new_agent,
                serde_json::json!({ "from": previous_agent, "to": new_agent }),
            )?;
        }
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

        // Refresh the project-local continuity brief so a harness reading latest.md inherits the
        // RESUMED state (new agent + carried tiers + re-verify warning), not the stale pre-handoff
        // export. This keeps the file-based reach path consistent with the JSON bundle after resume.
        let latest_path = DataLayout::project_handoff_latest(&session.project_path);
        if let Some(parent) = latest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &latest_path,
            export::render_handoff_brief(bundle, Some(new_agent)),
        )?;
        Ok(session)
    }

    pub fn get_provenance(&self, id: Uuid) -> Result<Option<ProvenanceRecord>> {
        self.db.get_provenance(id)
    }

    pub fn list_provenance(&self, limit: usize) -> Result<Vec<ProvenanceRecord>> {
        self.db.list_provenance(limit)
    }

    /// Provenance linked to a session, with legacy NULL rows fell back to project+agent+lifetime
    /// window (UX2 Sheet provenance section).
    pub fn provenance_for_session(&self, session: &Session) -> Result<Vec<ProvenanceRecord>> {
        self.db.provenance_for_session(session)
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
        ensure_environment_dir(path)?;

        let marker_path = DataLayout::project_scaffold_marker(path);
        let mut marker = read_scaffold_marker(&marker_path);

        for (rel, template) in [
            ("README.md", PROJECT_GUIDE),
            ("examples/contract-example.md", EXAMPLE_CONTRACT),
            ("examples/constraint-example.md", EXAMPLE_CONSTRAINT),
            ("examples/environment-example.md", EXAMPLE_ENVIRONMENT),
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

    /// Write the static pointer block into the selected native entry points so agents that never call
    /// MCP still learn where the contracts live. The block points at `.agent-control/rules/` rather
    /// than copying rule text, so it never goes stale on a rule edit. `targets` chooses which harness
    /// files to write (default `AGENTS.md` only); when `include_user` is set, a preferences pointer
    /// goes to the user-global `~/.claude/CLAUDE.md`. Everything outside the block markers is
    /// preserved, so a hand-written `AGENTS.md` is safe to export into. Re-export is idempotent.
    pub fn export_native(
        &self,
        project_path: &Path,
        targets: &[export::ExportTarget],
        include_user: bool,
    ) -> Result<export::ExportReport> {
        let rules = self.load_rules(Some(project_path))?;
        let project_rules = rules
            .iter()
            .filter(|r| r.enabled && r.scope == Scope::Project)
            .count();
        let user_rules = rules
            .iter()
            .filter(|r| r.enabled && r.scope == Scope::User)
            .count();

        let project_block = export::wrap_block(export::PROJECT_GUIDANCE);
        let mut outcomes = Vec::new();
        for target in targets {
            outcomes.push(write_managed_block_file(
                &target.path(project_path),
                &project_block,
                target.create_prefix(),
            )?);
        }

        if include_user {
            let user_block = export::wrap_block(export::USER_GUIDANCE);
            outcomes.push(write_managed_block_file(
                &DataLayout::claude_user_md()?,
                &user_block,
                "",
            )?);
        }

        Ok(export::ExportReport {
            project_path: project_path.to_path_buf(),
            project_rules,
            user_rules,
            targets: outcomes,
        })
    }

    /// Check the selected entry points for a missing or outdated-format block without writing anything.
    /// A target is stale when it exists and its embedded block-format version differs from
    /// `BLOCK_FORMAT` (e.g. a legacy v1 full-text block). Detection only: the caller prompts a
    /// re-export, never rewrites files silently.
    pub fn export_drift(
        &self,
        project_path: &Path,
        targets: &[export::ExportTarget],
        include_user: bool,
    ) -> Result<export::DriftReport> {
        let mut drifts: Vec<export::TargetDrift> = targets
            .iter()
            .map(|t| target_drift(&t.path(project_path)))
            .collect();

        if include_user {
            drifts.push(target_drift(&DataLayout::claude_user_md()?));
        }

        let stale = drifts.iter().any(|t| t.stale);
        Ok(export::DriftReport {
            project_path: project_path.to_path_buf(),
            stale,
            targets: drifts,
        })
    }

    /// Revoke the pointer block from the selected native entry points — the explicit undo of
    /// `export_native`. Surrounding prose is preserved; a file that held only the block (or only the
    /// generated Cursor frontmatter + block) is deleted. A target with no block is left unchanged.
    pub fn revoke_native(
        &self,
        project_path: &Path,
        targets: &[export::ExportTarget],
        include_user: bool,
    ) -> Result<export::RevokeReport> {
        let mut outcomes = Vec::new();
        for target in targets {
            outcomes.push(revoke_managed_block_file(&target.path(project_path))?);
        }
        if include_user {
            outcomes.push(revoke_managed_block_file(&DataLayout::claude_user_md()?)?);
        }
        Ok(export::RevokeReport {
            project_path: project_path.to_path_buf(),
            targets: outcomes,
        })
    }

    /// Register the Agent Control hook into the selected harnesses' config. Writes both a briefing
    /// hook (reach) and a gate hook (enforce) that run `command` (the base shell invocation of the
    /// hook script; the vendor id and event are appended per entry). Project-scope vendors write the
    /// repo config; user-only vendors (Kimi, Antigravity) write a user-global file whose gate still
    /// only enforces for the matching project (rules resolve by the call's `cwd`). Every other key and
    /// user-authored hook is preserved; re-registering is idempotent and writes are atomic. Targets
    /// that share a config file (Qoder + Qoder CN → `.qoder/settings.json`) are written once, so the
    /// report carries one outcome per file. This is what turns a `Hard` rule into an `Enforced` one:
    /// the harness mediates tool calls through `check_tool_call`.
    pub fn register_hooks(
        &self,
        project_path: &Path,
        targets: &[HookTarget],
        command: &str,
    ) -> Result<HookReport> {
        if command.trim().is_empty() {
            bail!("hook command cannot be empty");
        }
        let mut outcomes = Vec::new();
        for (path, target) in hook_config_files(project_path, targets)? {
            outcomes.push(write_hooks_file(&path, target, command)?);
        }
        Ok(HookReport {
            project_path: project_path.to_path_buf(),
            targets: outcomes,
        })
    }

    /// Revoke the Agent Control hook from the selected harnesses' config — the explicit undo of
    /// `register_hooks`. Other config and user hooks are preserved; a file that held only our hook is
    /// deleted. A target with no managed hook is left unchanged. Revoking either Qoder build removes
    /// the shared `.qoder/settings.json` entry for both (one outcome per file, as in register).
    pub fn revoke_hooks(&self, project_path: &Path, targets: &[HookTarget]) -> Result<HookReport> {
        let mut outcomes = Vec::new();
        for (path, target) in hook_config_files(project_path, targets)? {
            outcomes.push(revoke_hooks_file(&path, target)?);
        }
        Ok(HookReport {
            project_path: project_path.to_path_buf(),
            targets: outcomes,
        })
    }

    /// Detect which harnesses are installed and whether our hook is already registered in each one's
    /// config (project or user level). Read-only: drives the console's one-click register/restore UI.
    /// One-command harness onboarding: a thin, idempotent adapter composing `export_native`
    /// (pointer block reach) and `register_hooks` (start briefing + pre-tool gate). No new
    /// business logic — both halves keep their own managed-block / managed-group semantics.
    pub fn setup(
        &self,
        project_path: &Path,
        hook_targets: &[HookTarget],
        export_targets: &[export::ExportTarget],
        include_user: bool,
    ) -> Result<SetupReport> {
        let command = hooks::default_hook_command().context(
            "hook script not found beside the executable; run setup from a release or dev build",
        )?;
        let export = self.export_native(project_path, export_targets, include_user)?;
        let hooks = self.register_hooks(project_path, hook_targets, &command)?;
        Ok(SetupReport {
            project_path: project_path.to_path_buf(),
            command,
            export,
            hooks,
        })
    }

    pub fn detect_hooks(&self, project_path: &Path) -> Result<Vec<HookStatus>> {
        let mut statuses = Vec::new();
        for target in HookTarget::ALL {
            let settings_path = target.config_path(project_path)?;
            let registered = read_config_text(&settings_path)?
                .map(|text| hooks::registered(target, &text))
                .unwrap_or(false);
            let config_dir = settings_path.parent().map(|p| p.exists()).unwrap_or(false);
            let installed = config_dir || target.user_dir().map(|d| d.exists()).unwrap_or(false);
            statuses.push(HookStatus {
                target,
                label: target.label().to_string(),
                scope: target.scope(),
                installed,
                settings_path,
                registered,
            });
        }
        Ok(statuses)
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
            Scope::Project | Scope::Environment => {
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
        rule.enforcement = edits.enforcement.unwrap_or(suggestion.enforcement);
        rule.source.origin = Some("agent-suggestion".to_string());
        let project_path = match suggestion.scope {
            Scope::User => None,
            Scope::Project | Scope::Environment => {
                if suggestion.project_path.as_os_str().is_empty() {
                    bail!("project/environment-scope suggestion has no project path");
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
            command: "claude".into(),
            args: vec![],
            env: Default::default(),
        },
        AgentRegistration {
            id: "gemini-cli".into(),
            name: "Gemini CLI".into(),
            command: "gemini".into(),
            args: vec![],
            env: Default::default(),
        },
        AgentRegistration {
            id: "qoder".into(),
            name: "Qoder".into(),
            command: "qoder".into(),
            args: vec![],
            env: Default::default(),
        },
        AgentRegistration {
            id: "codebuddy".into(),
            name: "CodeBuddy".into(),
            command: "codebuddy".into(),
            args: vec![],
            env: Default::default(),
        },
        AgentRegistration {
            id: "workbuddy".into(),
            name: "WorkBuddy".into(),
            command: "workbuddy".into(),
            args: vec![],
            env: Default::default(),
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
        Scope::Project | Scope::Environment => SourceKind::Project,
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
        tool_guards: vec![],
        enabled: true,
        enforcement: Default::default(),
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
        enforcement: Default::default(),
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

/// Create the project-local environment dir and its `.gitignore` (ignore everything but itself) so
/// machine-specific notes are never committed alongside the shared contracts. Idempotent; returns
/// the dir path. Called when writing an environment rule and by the project scaffold.
fn ensure_environment_dir(project: &Path) -> Result<PathBuf> {
    let dir = DataLayout::project_environment_dir(project);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating environment dir {}", dir.display()))?;
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, "*\n!.gitignore\n")
            .with_context(|| format!("writing {}", gitignore.display()))?;
    }
    Ok(dir)
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
const TEMPLATE_VERSION: u32 = 2;

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

/// Write `block` into `path` as a managed block, preserving everything outside the markers.
/// `create_prefix` is prepended only when the file does not yet exist (e.g. Cursor frontmatter),
/// never to an existing file. Returns created | updated | unchanged; an unchanged file is not
/// rewritten, so re-exporting an up-to-date set is a no-op on disk.
fn write_managed_block_file(
    path: &Path,
    block: &str,
    create_prefix: &str,
) -> Result<export::TargetOutcome> {
    let existed = path.exists();
    let existing = if existed {
        std::fs::read_to_string(path)
            .with_context(|| format!("reading export target {}", path.display()))?
    } else {
        String::new()
    };
    let base = if existed {
        existing.clone()
    } else {
        format!("{create_prefix}{existing}")
    };
    let new_content = export::upsert_managed_block(&base, block)?;
    let action = if !existed {
        "created"
    } else if new_content == existing {
        "unchanged"
    } else {
        "updated"
    };
    if action != "unchanged" {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        std::fs::write(path, &new_content)
            .with_context(|| format!("writing export target {}", path.display()))?;
    }
    Ok(export::TargetOutcome {
        path: path.to_path_buf(),
        action: action.to_string(),
    })
}

fn target_drift(path: &Path) -> export::TargetDrift {
    let exists = path.exists();
    let exported_format = if exists {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| export::read_exported_format(&raw))
    } else {
        None
    };
    let stale = exists && exported_format != Some(export::BLOCK_FORMAT);
    export::TargetDrift {
        path: path.to_path_buf(),
        exists,
        exported_format,
        current_format: export::BLOCK_FORMAT,
        stale,
    }
}

/// Strip the managed block from `path`. Returns removed (prose kept) | deleted (file was block-only)
/// | unchanged (no block, or file absent). A file left empty — or holding only the Cursor frontmatter
/// we generated — is deleted rather than left as a stub.
fn revoke_managed_block_file(path: &Path) -> Result<export::TargetOutcome> {
    let action = if !path.exists() {
        "unchanged".to_string()
    } else {
        let existing = std::fs::read_to_string(path)
            .with_context(|| format!("reading export target {}", path.display()))?;
        match export::remove_managed_block(&existing)? {
            None => "unchanged".to_string(),
            Some(remaining) => {
                let stub = remaining.trim().is_empty()
                    || remaining.trim() == export::CURSOR_FRONTMATTER.trim();
                if stub {
                    std::fs::remove_file(path)
                        .with_context(|| format!("deleting export target {}", path.display()))?;
                    "deleted".to_string()
                } else {
                    std::fs::write(path, &remaining)
                        .with_context(|| format!("writing export target {}", path.display()))?;
                    "removed".to_string()
                }
            }
        }
    };
    Ok(export::TargetOutcome {
        path: path.to_path_buf(),
        action,
    })
}

/// Read a harness config file as raw text. Returns `None` when the file is absent or empty; the
/// per-vendor codec parses it (JSON or TOML), so registration never silently clobbers a file it could
/// not understand.
fn read_config_text(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(raw))
}

/// Write `contents` to `path` atomically: stage a sibling temp file, then rename over the target so a
/// crash mid-write never leaves a half-written harness config. The temp name carries the pid to avoid
/// collisions between the console and the MCP daemon.
fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "hooks".to_string());
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents).with_context(|| format!("staging config {}", tmp.display()))?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            Err(err).with_context(|| format!("replacing config {}", path.display()))
        }
    }
}

/// Resolve targets to the config files they write, dropping any target that shares a file with an
/// earlier one. Qoder and Qoder CN both read `.qoder/settings.json` at project level, so the pair
/// must be written (and revoked) once — the first target in request order owns the shared entry.
fn hook_config_files(
    project_path: &Path,
    targets: &[HookTarget],
) -> Result<Vec<(PathBuf, HookTarget)>> {
    let mut files: Vec<(PathBuf, HookTarget)> = Vec::new();
    for target in targets {
        let path = target.config_path(project_path)?;
        if files.iter().any(|(p, _)| *p == path) {
            continue;
        }
        files.push((path, *target));
    }
    Ok(files)
}

/// Upsert our managed hook entries into `path` via the target's codec, preserving all other config.
/// Returns created | updated | unchanged; an unchanged file is not rewritten, so re-registering is a
/// no-op on disk.
fn write_hooks_file(
    path: &Path,
    target: HookTarget,
    command: &str,
) -> Result<export::TargetOutcome> {
    let existed = path.exists();
    let existing = read_config_text(path)?.unwrap_or_default();
    let new_raw = hooks::upsert(target, &existing, command)?;
    let action = if !existed {
        "created"
    } else if new_raw == existing {
        "unchanged"
    } else {
        "updated"
    };
    if action != "unchanged" {
        atomic_write(path, &new_raw)?;
    }
    Ok(export::TargetOutcome {
        path: path.to_path_buf(),
        action: action.to_string(),
    })
}

/// Strip our managed hook entries from `path`. Returns removed (other config kept) | deleted (file
/// held only our hook) | unchanged (no managed hook, or file absent).
fn revoke_hooks_file(path: &Path, target: HookTarget) -> Result<export::TargetOutcome> {
    let action = match read_config_text(path)? {
        None => "unchanged".to_string(),
        Some(text) => match hooks::remove(target, &text)? {
            None => "unchanged".to_string(),
            Some(new_text) => {
                let trimmed = new_text.trim();
                if trimmed.is_empty() || trimmed == "{}" {
                    std::fs::remove_file(path)
                        .with_context(|| format!("deleting config {}", path.display()))?;
                    "deleted".to_string()
                } else {
                    atomic_write(path, &new_text)?;
                    "removed".to_string()
                }
            }
        },
    };
    Ok(export::TargetOutcome {
        path: path.to_path_buf(),
        action,
    })
}

const PROJECT_GUIDE: &str = "\
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
- `local/` — environment notes for THIS user × THIS project × THIS machine (e.g. \"agents run inside
  WSL\"). Git-ignored (a `.gitignore` inside ignores everything but itself), so machine-specific facts
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

const EXAMPLE_ENVIRONMENT: &str = "\
---
# Environment notes describe THIS user's CURRENT project on THIS machine. They are descriptive
# context, not competing contracts: always applied, never exported into committed AGENTS.md/CLAUDE.md.
# Copy this file into local/ (git-ignored) to activate it; files in local/ are environment-scope.
id: wsl-build-environment
title: Agents run inside WSL; use Windows-native tools to compile/verify
kind: constraint
scope: environment
enforcement: advisory
source:
  kind: project
  author: your-name
  created_at: 2026-01-01T00:00:00Z
tags: [environment, wsl]
---
This project is checked out on a Windows machine, but agents run inside WSL.

- The repo path under WSL maps to a Windows path; Windows-native toolchains (cargo via mise, rustc)
  are callable for compile/verify even though the shell is Linux.
- Do not assume Linux-only paths or tools; confirm which side a command runs on before relying on it.
- This fact is true only on THIS machine. It is git-ignored and delivered via the MCP/hook briefing,
  never committed — another machine or CI may differ. Put portable personal preferences in user scope
  instead, not here.

本机为 Windows，但代理运行在 WSL 内：仓库路径映射到 Windows 路径，编译/验证可调用 Windows 原生工具链
（如经 mise 的 cargo）。此事实仅对本机成立，已被 git 忽略，仅通过 MCP/hook 简报传递，不会提交；
可跨项目跟随的个人偏好请放在用户级，而不是这里。
";
