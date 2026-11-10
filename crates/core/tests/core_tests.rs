use std::path::PathBuf;

use agent_control_core::model::{
    AppliesTo, CodeCheck, Decision, EvidenceKind, QueryContext, Rule, RuleKind, Scope,
    SessionStatus, SourceKind, SuggestionEdits, SuggestionStatus,
};
use agent_control_core::{RuleEngine, Store};
use chrono::Utc;
use uuid::Uuid;

fn make_rule(id: &str, scope: Scope, priority: i32, topic: Option<&str>) -> Rule {
    Rule {
        id: id.into(),
        version: 1,
        title: id.into(),
        topic: topic.map(String::from),
        content: format!("content of {id}"),
        kind: RuleKind::Preference,
        scope,
        source: agent_control_core::model::RuleSource {
            kind: SourceKind::User,
            file_path: None,
            author: None,
            created_at: Utc::now(),
            origin: None,
        },
        priority,
        applies_to: AppliesTo::default(),
        tags: vec![],
        conflicts_with: vec![],
        code_checks: vec![],
        enabled: true,
        checksum: String::new(),
    }
    .with_checksum()
}

fn ctx() -> QueryContext {
    QueryContext {
        project_path: PathBuf::from("C:/project"),
        agent_id: "claude-code".into(),
        file_paths: vec!["src/lib.rs".into()],
        languages: vec!["rust".into()],
        task_tags: vec![],
        session_id: None,
    }
}

#[test]
fn project_scope_beats_user_scope() {
    let user = make_rule("indent-user", Scope::User, 999, Some("indentation"));
    let project = make_rule("indent-project", Scope::Project, 1, Some("indentation"));
    let resolution = RuleEngine::resolve(&[user, project], &ctx());
    assert_eq!(resolution.applied.len(), 1);
    assert_eq!(resolution.applied[0].rule.id, "indent-project");
    let superseded = resolution
        .trace
        .iter()
        .find(|m| m.rule.id == "indent-user")
        .unwrap();
    assert_eq!(superseded.decision, Decision::Superseded);
    assert_eq!(superseded.superseded_by.as_deref(), Some("indent-project"));
}

#[test]
fn priority_breaks_ties_within_scope() {
    let low = make_rule("low", Scope::User, 10, Some("style"));
    let high = make_rule("high", Scope::User, 20, Some("style"));
    let resolution = RuleEngine::resolve(&[low, high], &ctx());
    assert_eq!(resolution.applied[0].rule.id, "high");
}

#[test]
fn different_topics_coexist() {
    let a = make_rule("a", Scope::User, 100, Some("indentation"));
    let b = make_rule("b", Scope::User, 100, Some("errors"));
    let resolution = RuleEngine::resolve(&[a, b], &ctx());
    assert_eq!(resolution.applied.len(), 2);
}

#[test]
fn agent_filter_excludes_non_matching() {
    let mut rule = make_rule("gemini-only", Scope::User, 100, None);
    rule.applies_to.agents = vec!["gemini-*".into()];
    let resolution = RuleEngine::resolve(&[rule], &ctx());
    assert!(resolution.applied.is_empty());
    assert_eq!(resolution.trace[0].decision, Decision::FilteredOut);
}

#[test]
fn path_glob_matches() {
    let mut rule = make_rule("rust-only", Scope::User, 100, None);
    rule.applies_to.paths = vec!["src/**/*.rs".into()];
    let resolution = RuleEngine::resolve(&[rule], &ctx());
    assert_eq!(resolution.applied.len(), 1);
}

#[test]
fn explicit_conflict_flagged() {
    let mut a = make_rule("a", Scope::User, 100, None);
    a.conflicts_with = vec!["b".into()];
    let b = make_rule("b", Scope::User, 100, None);
    let resolution = RuleEngine::resolve(&[a, b], &ctx());
    assert_eq!(resolution.conflicts.len(), 1);
    assert!(
        resolution
            .applied
            .iter()
            .all(|m| m.decision == Decision::ConflictFlagged)
    );
}

#[test]
fn code_check_detects_editorconfig_conflict() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".editorconfig"),
        "[*]\nindent_style = space\n",
    )
    .unwrap();
    let mut rule = make_rule("tabs", Scope::Project, 100, None);
    rule.code_checks = vec![CodeCheck {
        file: ".editorconfig".into(),
        key: "indent_style".into(),
        expect: "tab".into(),
        description: None,
    }];
    let mut context = ctx();
    context.project_path = dir.path().to_path_buf();
    let resolution = RuleEngine::resolve(&[rule], &context);
    assert_eq!(resolution.conflicts.len(), 1);
    assert_eq!(
        resolution.conflicts[0].kind,
        agent_control_core::model::ConflictKind::RuleVsCode
    );
}

#[test]
fn evidence_chain_verifies_and_detects_tampering() {
    let store = Store::open_with_root(tempfile::tempdir().unwrap().path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let session = store
        .create_session("test task", project.path(), "tester", vec![])
        .unwrap();
    store
        .record_evidence(
            session.id,
            EvidenceKind::ToolCall,
            "tester",
            serde_json::json!({"tool": "edit"}),
        )
        .unwrap();
    store
        .record_evidence(
            session.id,
            EvidenceKind::TestResult,
            "tester",
            serde_json::json!({"passed": true}),
        )
        .unwrap();
    let report = store.verify_chain(session.id).unwrap();
    assert!(report.valid);
    assert_eq!(report.entries_checked, 3);

    let mut entries = store.get_evidence(session.id).unwrap();
    entries[1].payload = serde_json::json!({"tool": "TAMPERED"});
    assert!(!entries[1].verify(&entries[0].entry_hash));
}

#[test]
fn handoff_roundtrip_preserves_constraints() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".agent-control/rules")).unwrap();
    let rule = make_rule("project-contract", Scope::Project, 100, None);
    store.save_rule(&rule, Some(project.path())).unwrap();

    let session = store
        .create_session("handoff test", project.path(), "harness-a", vec![])
        .unwrap();
    assert_eq!(session.constraint_snapshot.rules.len(), 1);

    let bundle = store
        .export_handoff(
            session.id,
            "harness-b",
            serde_json::json!({"summary": "work so far"}),
        )
        .unwrap();
    assert!(bundle.verify_integrity());

    let resumed = store.import_handoff(&bundle, "harness-b").unwrap();
    assert_eq!(resumed.agent_id, "harness-b");
    assert_eq!(resumed.status, SessionStatus::Active);
    assert_eq!(
        resumed.constraint_snapshot.snapshot_hash,
        session.constraint_snapshot.snapshot_hash
    );

    let rule_ids: Vec<String> = resumed
        .constraint_snapshot
        .rules
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let report = store
        .acknowledge_rules(session.id, "harness-b", &rule_ids)
        .unwrap();
    assert!(report.fully_compliant);

    let bad = store
        .acknowledge_rules(session.id, "harness-b", &[])
        .unwrap();
    assert!(!bad.fully_compliant);
    assert_eq!(bad.missing.len(), 1);
}

#[test]
fn rule_file_roundtrip_markdown_frontmatter() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let rule = agent_control_core::store::new_user_rule(
        "prefer-tabs",
        "Prefer tabs",
        "Use tabs everywhere.",
    );
    let path = store.save_rule(&rule, None).unwrap();
    assert!(path.exists());
    let loaded = store.load_rules(None).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "prefer-tabs");
    assert_eq!(loaded[0].content, "Use tabs everywhere.");
    assert_eq!(loaded[0].scope, Scope::User);
    assert!(store.delete_rule("prefer-tabs", Scope::User, None).unwrap());
    assert!(store.load_rules(None).unwrap().is_empty());
}

#[test]
fn provenance_records_full_trace() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    store
        .save_rule(&make_rule("u", Scope::User, 100, Some("t")), None)
        .unwrap();
    store
        .save_rule(
            &make_rule("p", Scope::Project, 100, Some("t")),
            Some(project.path()),
        )
        .unwrap();
    let mut context = ctx();
    context.project_path = project.path().to_path_buf();
    let (record, resolution) = store.resolve_rules(&context).unwrap();
    assert_eq!(resolution.applied.len(), 1);
    assert_eq!(record.trace.len(), 2);
    let fetched = store.get_provenance(record.id).unwrap().unwrap();
    assert_eq!(fetched.trace.len(), 2);
    assert_eq!(fetched.agent_id, "claude-code");
}

#[test]
fn session_snapshot_hash_stable() {
    let rules = vec![make_rule("a", Scope::User, 100, None)];
    let s1 = agent_control_core::model::ConstraintSnapshot::new(rules.clone(), Uuid::new_v4());
    let s2 = agent_control_core::model::ConstraintSnapshot::new(rules, Uuid::new_v4());
    assert_eq!(s1.snapshot_hash, s2.snapshot_hash);
}

#[test]
fn project_registry_register_dedup_list_remove() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let proj_a = tempfile::tempdir().unwrap();
    let proj_b = tempfile::tempdir().unwrap();

    let a = store
        .register_project("Alpha", proj_a.path(), None)
        .unwrap();
    let b = store
        .register_project("Beta", proj_b.path(), Some("second".into()))
        .unwrap();
    assert_eq!(store.list_projects().unwrap().len(), 2);
    assert!(!a.path.to_string_lossy().starts_with(r"\\?\"));

    // Re-registering the same path dedups: same id, updated name, no extra row.
    let a2 = store
        .register_project("Alpha Renamed", proj_a.path(), None)
        .unwrap();
    assert_eq!(a2.id, a.id);
    assert_eq!(a2.name, "Alpha Renamed");
    assert_eq!(store.list_projects().unwrap().len(), 2);

    let found = store.project_by_path(proj_a.path()).unwrap().unwrap();
    assert_eq!(found.id, a.id);

    // Most-recently-opened ordering.
    store.touch_project(b.id).unwrap();
    assert_eq!(store.list_projects().unwrap()[0].id, b.id);

    store.rename_project(a.id, "Alpha Final").unwrap();
    assert_eq!(
        store.get_project(a.id).unwrap().unwrap().name,
        "Alpha Final"
    );

    // Removing unregisters but never deletes repository files.
    assert!(store.remove_project(a.id).unwrap());
    assert_eq!(store.list_projects().unwrap().len(), 1);
    assert!(proj_a.path().exists());
    assert!(!store.remove_project(a.id).unwrap());
}

#[test]
fn register_project_rejects_missing_path() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let missing = root.path().join("does-not-exist");
    assert!(store.register_project("Ghost", &missing, None).is_err());
    assert!(store.list_projects().unwrap().is_empty());
}

#[test]
fn init_project_contracts_scaffolds_and_preserves_rules() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let rules_dir = store.init_project_contracts(project.path()).unwrap();
    assert!(rules_dir.is_dir());
    let guide = project.path().join(".agent-control/README.md");
    assert!(guide.exists());
    let guide_text = std::fs::read_to_string(&guide).unwrap();
    assert!(guide_text.contains("项目契约"));

    // A contract saved into the scaffold is loaded as project scope; the guide is not a rule.
    store
        .save_rule(
            &make_rule("contract-1", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();
    let guide_before = std::fs::read_to_string(&guide).unwrap();
    store.init_project_contracts(project.path()).unwrap(); // idempotent
    assert_eq!(std::fs::read_to_string(&guide).unwrap(), guide_before);

    let project_rules = store.load_rules(Some(project.path())).unwrap();
    assert_eq!(project_rules.len(), 1);
    assert_eq!(project_rules[0].id, "contract-1");
    assert_eq!(project_rules[0].scope, Scope::Project);
}

#[test]
fn user_and_project_rules_are_separated() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    store
        .save_rule(&make_rule("personal-pref", Scope::User, 100, None), None)
        .unwrap();
    store
        .save_rule(
            &make_rule("project-contract", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();

    // No project context → only user-scope preferences.
    let user_only = store.load_rules(None).unwrap();
    assert_eq!(user_only.len(), 1);
    assert_eq!(user_only[0].id, "personal-pref");
    assert_eq!(user_only[0].scope, Scope::User);

    // With a project context → user preferences plus that project's contracts, distinctly scoped.
    let combined = store.load_rules(Some(project.path())).unwrap();
    assert_eq!(combined.len(), 2);
    assert!(
        combined
            .iter()
            .any(|r| r.id == "personal-pref" && r.scope == Scope::User)
    );
    assert!(
        combined
            .iter()
            .any(|r| r.id == "project-contract" && r.scope == Scope::Project)
    );

    // A different project does not see the first project's contracts.
    let other = tempfile::tempdir().unwrap();
    let other_rules = store.load_rules(Some(other.path())).unwrap();
    assert_eq!(other_rules.len(), 1);
    assert_eq!(other_rules[0].id, "personal-pref");
}

#[test]
fn upsert_rule_creates_then_updates_with_version_bump() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let draft = agent_control_core::store::new_rule(
        "api-isasc",
        "isAsc type",
        "v1 content",
        RuleKind::Contract,
        Scope::Project,
        Some("claude-code".into()),
    );
    let (saved, path) = store.upsert_rule(&draft, Some(project.path())).unwrap();
    assert!(path.exists());
    assert_eq!(saved.version, 1);

    let created = store
        .get_rule("api-isasc", Some(project.path()))
        .unwrap()
        .unwrap();
    assert_eq!(created.version, 1);
    assert_eq!(created.content, "v1 content");
    assert_eq!(created.source.author.as_deref(), Some("claude-code"));
    let original_created_at = created.source.created_at;

    // Reusing the id updates content, bumps version, and preserves the original created_at.
    let update = agent_control_core::store::new_rule(
        "api-isasc",
        "isAsc type",
        "v2 content",
        RuleKind::Contract,
        Scope::Project,
        Some("gemini-cli".into()),
    );
    store.upsert_rule(&update, Some(project.path())).unwrap();
    let updated = store
        .get_rule("api-isasc", Some(project.path()))
        .unwrap()
        .unwrap();
    assert_eq!(updated.version, 2);
    assert_eq!(updated.content, "v2 content");
    assert_eq!(updated.source.created_at, original_created_at);

    // Only one file exists for the id (no duplicate).
    let rules = store.load_rules(Some(project.path())).unwrap();
    assert_eq!(rules.iter().filter(|r| r.id == "api-isasc").count(), 1);
}

#[test]
fn init_project_contracts_scaffolds_non_binding_examples() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    store.init_project_contracts(project.path()).unwrap();
    let examples = project.path().join(".agent-control/examples");
    let contract = examples.join("contract-example.md");
    let constraint = examples.join("constraint-example.md");
    assert!(contract.exists());
    assert!(constraint.exists());

    // Examples live outside rules/, so they are NOT loaded as binding contracts.
    assert!(store.load_rules(Some(project.path())).unwrap().is_empty());

    // Copying a template into rules/ activates it and proves the template parses.
    let rules_dir = store.init_project_contracts(project.path()).unwrap();
    std::fs::copy(&contract, rules_dir.join("contract-example.md")).unwrap();
    let rules = store.load_rules(Some(project.path())).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "upstream-api-isasc-type");
    assert_eq!(rules[0].scope, Scope::Project);
    assert_eq!(rules[0].kind, RuleKind::Contract);
}

fn make_suggestion(
    project: &std::path::Path,
    rule_id: &str,
) -> agent_control_core::model::RuleSuggestion {
    let mut draft = agent_control_core::store::new_suggestion(
        project,
        "claude-code",
        rule_id,
        "isAsc accepts bool or string",
        "Upstream API isAsc may receive bool or string; normalize before calling.",
        RuleKind::Contract,
    );
    draft.rationale = Some("Discovered while integrating the upstream API.".into());
    draft.tags = vec!["api".into()];
    draft
}

#[test]
fn suggestion_submit_approve_writes_project_rule() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let submitted = store
        .submit_suggestion(make_suggestion(project.path(), "api-isasc"))
        .unwrap();
    assert_eq!(submitted.status, SuggestionStatus::Pending);
    assert!(!submitted.id.is_nil());
    assert_eq!(
        store
            .count_suggestions(None, Some(SuggestionStatus::Pending))
            .unwrap(),
        1
    );
    // Nothing is written to the project before review.
    assert!(store.load_rules(Some(project.path())).unwrap().is_empty());

    let edits = SuggestionEdits {
        title: Some("isAsc type contract (reviewed)".into()),
        content: Some("Normalize isAsc to string before calling upstream.".into()),
        ..Default::default()
    };
    let (reviewed, saved, path) = store
        .approve_suggestion(submitted.id, edits, Some("Tightened wording.".into()))
        .unwrap();
    assert!(path.exists());
    assert_eq!(reviewed.status, SuggestionStatus::Approved);
    assert!(reviewed.reviewed_at.is_some());
    assert_eq!(reviewed.review_note.as_deref(), Some("Tightened wording."));
    assert_eq!(reviewed.approved_rule_id.as_deref(), Some("api-isasc"));
    assert_eq!(saved.title, "isAsc type contract (reviewed)");
    assert_eq!(saved.scope, Scope::Project);
    assert_eq!(saved.source.origin.as_deref(), Some("agent-suggestion"));
    assert_eq!(saved.tags, vec!["api".to_string()]);

    let loaded = store
        .get_rule("api-isasc", Some(project.path()))
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded.content,
        "Normalize isAsc to string before calling upstream."
    );

    // A reviewed suggestion cannot be approved again.
    assert!(
        store
            .approve_suggestion(submitted.id, SuggestionEdits::default(), None)
            .is_err()
    );
}

#[test]
fn suggestion_reject_writes_no_rule() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let submitted = store
        .submit_suggestion(make_suggestion(project.path(), "api-isasc"))
        .unwrap();
    let rejected = store
        .reject_suggestion(submitted.id, Some("Out of scope for this project.".into()))
        .unwrap();
    assert_eq!(rejected.status, SuggestionStatus::Rejected);
    assert_eq!(
        rejected.review_note.as_deref(),
        Some("Out of scope for this project.")
    );
    assert!(rejected.approved_rule_id.is_none());
    assert!(store.load_rules(Some(project.path())).unwrap().is_empty());
    assert!(store.reject_suggestion(submitted.id, None).is_err());
    assert!(
        store
            .approve_suggestion(submitted.id, SuggestionEdits::default(), None)
            .is_err()
    );
}

#[test]
fn suggestions_filter_by_project_and_status() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let proj_a = tempfile::tempdir().unwrap();
    let proj_b = tempfile::tempdir().unwrap();

    let a = store
        .submit_suggestion(make_suggestion(proj_a.path(), "rule-a"))
        .unwrap();
    store
        .submit_suggestion(make_suggestion(proj_b.path(), "rule-b"))
        .unwrap();

    assert_eq!(
        store
            .list_suggestions(None, Some(SuggestionStatus::Pending))
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store
            .list_suggestions(Some(proj_a.path()), None)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .count_suggestions(Some(proj_b.path()), Some(SuggestionStatus::Pending))
            .unwrap(),
        1
    );

    store
        .approve_suggestion(a.id, SuggestionEdits::default(), None)
        .unwrap();
    assert_eq!(
        store
            .count_suggestions(None, Some(SuggestionStatus::Pending))
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count_suggestions(None, Some(SuggestionStatus::Approved))
            .unwrap(),
        1
    );
    let approved = store
        .list_suggestions(Some(proj_a.path()), Some(SuggestionStatus::Approved))
        .unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].rule_id, "rule-a");
}

#[test]
fn submit_suggestion_validates_fields_and_path() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let mut empty_title = make_suggestion(project.path(), "api-isasc");
    empty_title.title = "  ".into();
    assert!(store.submit_suggestion(empty_title).is_err());

    let missing = make_suggestion(&project.path().join("does-not-exist"), "api-isasc");
    assert!(store.submit_suggestion(missing).is_err());
}
