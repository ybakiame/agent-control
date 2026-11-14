use std::path::PathBuf;

use agent_control_core::model::{
    AppliesTo, CodeCheck, Decision, EvidenceKind, QueryContext, Rule, RuleKind, Scope,
    SessionStatus, SourceKind, SuggestionEdits, SuggestionStatus, WorkItem, WorkItemStatus,
};
use agent_control_core::{
    AppConfig, DEFAULT_MCP_PORT, DataLayout, ExportTarget, RuleEngine, Store,
};
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
        tool_guards: vec![],
        enabled: true,
        enforcement: Default::default(),
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
        .unwrap()
        .0;
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
        .unwrap()
        .0;
    assert_eq!(session.constraint_snapshot.rules.len(), 1);

    let bundle = store
        .export_handoff(
            session.id,
            Some("harness-b"),
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
fn work_state_roundtrip_and_handoff_carry() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let session = store
        .create_session("work state", project.path(), "harness-a", vec![])
        .unwrap()
        .0;

    let items = vec![
        WorkItem {
            title: "done thing".into(),
            status: WorkItemStatus::Done,
        },
        WorkItem {
            title: "doing thing".into(),
            status: WorkItemStatus::Doing,
        },
        WorkItem {
            title: "todo thing".into(),
            status: WorkItemStatus::Todo,
        },
    ];
    store
        .set_session_work_state(session.id, "the goal".into(), items.clone())
        .unwrap();
    let loaded = store.get_session(session.id).unwrap().unwrap();
    assert_eq!(loaded.goal, "the goal");
    assert_eq!(loaded.work_items.len(), 3);
    assert_eq!(loaded.work_items[0].status, WorkItemStatus::Done);

    let bundle = store
        .export_handoff(session.id, Some("harness-b"), serde_json::json!({}))
        .unwrap();
    assert_eq!(bundle.goal, "the goal");
    assert_eq!(bundle.work_items.len(), 3);
    let latest =
        std::fs::read_to_string(project.path().join(".agent-control/handoff/latest.md")).unwrap();
    assert!(latest.contains("## Goal"), "{latest}");
    assert!(latest.contains("the goal"), "{latest}");
    assert!(latest.contains("**doing**: doing thing"), "{latest}");

    let resumed = store.import_handoff(&bundle, "harness-b").unwrap();
    assert_eq!(resumed.goal, "the goal");
    assert_eq!(resumed.work_items.len(), 3);
    assert_eq!(resumed.work_items[1].status, WorkItemStatus::Doing);
}

#[test]
fn create_session_reuses_active_same_task() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let (first, reused) = store
        .create_session("same task", project.path(), "harness-a", vec![])
        .unwrap();
    assert!(!reused);
    let (again, reused) = store
        .create_session("same task", project.path(), "harness-a", vec![])
        .unwrap();
    assert!(reused);
    assert_eq!(again.id, first.id);
    assert_eq!(store.list_sessions().unwrap().len(), 1);

    let (other, reused) = store
        .create_session("other task", project.path(), "harness-a", vec![])
        .unwrap();
    assert!(!reused);
    assert_ne!(other.id, first.id);

    // A closed session no longer absorbs new calls for the same task.
    store
        .set_session_status(first.id, SessionStatus::Completed)
        .unwrap();
    let (fresh, reused) = store
        .create_session("same task", project.path(), "harness-a", vec![])
        .unwrap();
    assert!(!reused);
    assert_ne!(fresh.id, first.id);
}

#[test]
fn set_session_status_closes_and_reopens_with_evidence() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let session = store
        .create_session("lifecycle", project.path(), "tester", vec![])
        .unwrap()
        .0;

    let closed = store
        .set_session_status(session.id, SessionStatus::Completed)
        .unwrap();
    assert_eq!(closed.status, SessionStatus::Completed);
    let chain = store.get_evidence(session.id).unwrap();
    assert!(chain.iter().any(|e| e.kind == EvidenceKind::SessionEnd));
    assert!(store.verify_chain(session.id).unwrap().valid);

    let reopened = store
        .set_session_status(session.id, SessionStatus::Active)
        .unwrap();
    assert_eq!(reopened.status, SessionStatus::Active);
}

#[test]
fn export_handoff_without_to_agent() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let session = store
        .create_session("no target", project.path(), "harness-a", vec![])
        .unwrap()
        .0;

    let bundle = store
        .export_handoff(session.id, None, serde_json::json!({}))
        .unwrap();
    let loaded = store.get_session(session.id).unwrap().unwrap();
    assert_eq!(loaded.status, SessionStatus::HandedOff);
    assert_eq!(loaded.handoffs.last().unwrap().to_agent, None);
    assert!(
        project
            .path()
            .join(".agent-control/handoff/latest.md")
            .exists()
    );
    let resumed = store.import_handoff(&bundle, "harness-b").unwrap();
    assert_eq!(resumed.agent_id, "harness-b");
    assert_eq!(resumed.status, SessionStatus::Active);
}

#[test]
fn delete_session_cascades_evidence_and_provenance() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let session = store
        .create_session("to delete", project.path(), "tester", vec![])
        .unwrap()
        .0;
    store
        .record_evidence(
            session.id,
            EvidenceKind::ToolCall,
            "tester",
            serde_json::json!({"tool": "x"}),
        )
        .unwrap();
    let provenance_id = session.constraint_snapshot.provenance_id;
    assert!(!store.get_evidence(session.id).unwrap().is_empty());
    assert!(store.get_provenance(provenance_id).unwrap().is_some());

    store.delete_session(session.id).unwrap();
    assert!(store.get_session(session.id).unwrap().is_none());
    assert!(store.get_evidence(session.id).unwrap().is_empty());
    assert!(store.get_provenance(provenance_id).unwrap().is_none());
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
fn disabled_rule_is_excluded_from_resolution() {
    let mut rule = make_rule("paused", Scope::User, 100, Some("style"));
    rule.enabled = false;
    let resolution = RuleEngine::resolve(&[rule.clone()], &ctx());
    assert!(resolution.applied.is_empty());
    assert_eq!(resolution.trace[0].decision, Decision::Disabled);

    // A disabled rule neither applies nor competes: an enabled peer wins outright.
    let peer = make_rule("active", Scope::User, 1, Some("style"));
    let resolution = RuleEngine::resolve(&[rule, peer], &ctx());
    assert_eq!(resolution.applied.len(), 1);
    assert_eq!(resolution.applied[0].rule.id, "active");
}

#[test]
fn set_rule_enabled_round_trips_and_bumps_version() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let rule = agent_control_core::store::new_rule(
        "toggle-me",
        "Toggle me",
        "content",
        RuleKind::Preference,
        Scope::User,
        None,
    );
    store.save_rule(&rule, None).unwrap();
    let initial = store.get_rule("toggle-me", None).unwrap().unwrap();
    assert!(initial.enabled);

    let disabled = store
        .set_rule_enabled("toggle-me", Scope::User, None, false)
        .unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.version, initial.version + 1);
    let reloaded = store.get_rule("toggle-me", None).unwrap().unwrap();
    assert!(!reloaded.enabled);
    assert_eq!(reloaded.content, "content");

    let enabled = store
        .set_rule_enabled("toggle-me", Scope::User, None, true)
        .unwrap();
    assert!(enabled.enabled);
    assert_eq!(enabled.version, initial.version + 2);
    assert!(store.get_rule("toggle-me", None).unwrap().unwrap().enabled);
}

#[test]
fn upsert_rule_preserves_disabled_state() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let rule = agent_control_core::store::new_rule(
        "paused-contract",
        "Paused",
        "v1",
        RuleKind::Contract,
        Scope::User,
        None,
    );
    store.save_rule(&rule, None).unwrap();
    store
        .set_rule_enabled("paused-contract", Scope::User, None, false)
        .unwrap();

    // A content update via upsert (enabled defaults true) must NOT re-enable the rule.
    let update = agent_control_core::store::new_rule(
        "paused-contract",
        "Paused",
        "v2",
        RuleKind::Contract,
        Scope::User,
        None,
    );
    assert!(update.enabled);
    store.upsert_rule(&update, None).unwrap();
    let after = store.get_rule("paused-contract", None).unwrap().unwrap();
    assert!(!after.enabled, "upsert must preserve the disabled state");
    assert_eq!(after.content, "v2");
}

#[test]
fn set_rule_enabled_project_scope_and_missing_rule() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let rule = agent_control_core::store::new_rule(
        "proj-rule",
        "Project rule",
        "content",
        RuleKind::Contract,
        Scope::Project,
        None,
    );
    store.save_rule(&rule, Some(project.path())).unwrap();

    let disabled = store
        .set_rule_enabled("proj-rule", Scope::Project, Some(project.path()), false)
        .unwrap();
    assert!(!disabled.enabled);
    assert!(
        !store
            .get_rule("proj-rule", Some(project.path()))
            .unwrap()
            .unwrap()
            .enabled
    );

    // Project scope requires a project path.
    assert!(
        store
            .set_rule_enabled("proj-rule", Scope::Project, None, true)
            .is_err()
    );
    // Unknown id errors instead of silently creating a rule.
    assert!(
        store
            .set_rule_enabled("nope", Scope::User, None, false)
            .is_err()
    );
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

fn scaffold_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn read_marker(project: &std::path::Path) -> serde_json::Value {
    let path = project.join(".agent-control/.scaffold.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn init_project_contracts_preserves_user_edited_scaffold() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    store.init_project_contracts(project.path()).unwrap();

    let guide = project.path().join(".agent-control/README.md");
    let edited = format!(
        "{}\n\nMy custom note.",
        std::fs::read_to_string(&guide).unwrap()
    );
    std::fs::write(&guide, &edited).unwrap();

    store.init_project_contracts(project.path()).unwrap();
    assert_eq!(std::fs::read_to_string(&guide).unwrap(), edited);
    // The edited file is no longer managed, so future upgrades never clobber it.
    assert!(
        read_marker(project.path())["files"]
            .get("README.md")
            .is_none()
    );
}

#[test]
fn init_project_contracts_migrates_untouched_scaffold() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    store.init_project_contracts(project.path()).unwrap();

    let guide = project.path().join(".agent-control/README.md");
    let marker_path = project.path().join(".agent-control/.scaffold.json");

    // Simulate an older template we wrote that the user never touched: file content and the
    // recorded hash agree, but both differ from the current template.
    let old = "# old guide\n";
    std::fs::write(&guide, old).unwrap();
    let mut marker = read_marker(project.path());
    marker["files"]["README.md"] = serde_json::Value::String(scaffold_sha256(old.as_bytes()));
    std::fs::write(&marker_path, serde_json::to_string(&marker).unwrap()).unwrap();

    store.init_project_contracts(project.path()).unwrap();
    let migrated = std::fs::read_to_string(&guide).unwrap();
    assert_ne!(migrated, old);
    assert!(migrated.contains("项目契约"));
    assert_eq!(
        read_marker(project.path())["files"]["README.md"]
            .as_str()
            .unwrap(),
        scaffold_sha256(migrated.as_bytes())
    );
}

#[test]
fn init_project_contracts_adopts_legacy_unmodified_scaffold() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    store.init_project_contracts(project.path()).unwrap();

    let guide = project.path().join(".agent-control/README.md");
    let current = std::fs::read_to_string(&guide).unwrap();
    // Legacy project: content matches the current template but there is no marker yet.
    std::fs::remove_file(project.path().join(".agent-control/.scaffold.json")).unwrap();

    store.init_project_contracts(project.path()).unwrap();
    assert_eq!(std::fs::read_to_string(&guide).unwrap(), current);
    assert_eq!(
        read_marker(project.path())["files"]["README.md"]
            .as_str()
            .unwrap(),
        scaffold_sha256(current.as_bytes())
    );
}

#[test]
fn init_project_contracts_preserves_legacy_modified_scaffold() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    store.init_project_contracts(project.path()).unwrap();

    let guide = project.path().join(".agent-control/README.md");
    std::fs::write(&guide, "# hand written guide\n").unwrap();
    std::fs::remove_file(project.path().join(".agent-control/.scaffold.json")).unwrap();

    store.init_project_contracts(project.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(&guide).unwrap(),
        "# hand written guide\n"
    );
    assert!(
        read_marker(project.path())["files"]
            .get("README.md")
            .is_none()
    );
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
        Scope::Project,
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
fn user_suggestion_submit_approve_writes_user_rule() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let draft = agent_control_core::store::new_suggestion(
        std::path::Path::new(""),
        "claude-code",
        "prefer-tabs",
        "Prefer tabs over spaces",
        "I prefer tabs for indentation across all my projects; this is a portable personal preference.",
        RuleKind::Preference,
        Scope::User,
    );
    let submitted = store.submit_suggestion(draft).unwrap();
    assert_eq!(submitted.scope, Scope::User);
    assert!(submitted.project_path.as_os_str().is_empty());
    assert!(store.load_rules(None).unwrap().is_empty());

    let (reviewed, saved, path) = store
        .approve_suggestion(submitted.id, SuggestionEdits::default(), None)
        .unwrap();
    assert_eq!(reviewed.status, SuggestionStatus::Approved);
    assert_eq!(saved.scope, Scope::User);
    assert!(path.starts_with(store.layout.user_rules_dir()));
    assert!(
        store
            .load_rules(None)
            .unwrap()
            .iter()
            .any(|r| r.id == "prefer-tabs" && r.scope == Scope::User)
    );
    // A user-scope suggestion is global: a project filter must not return it.
    assert!(
        store
            .list_suggestions(Some(project.path()), None)
            .unwrap()
            .is_empty()
    );
    let all = store.list_suggestions(None, None).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].scope, Scope::User);
}

#[test]
fn environment_suggestion_submit_approve_writes_local_rule() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    // WSL is a machine/project fact, NOT a portable preference: it belongs in environment scope.
    let draft = agent_control_core::store::new_suggestion(
        project.path(),
        "claude-code",
        "wsl-env",
        "Agents run inside WSL",
        "Harnesses run in WSL on Windows; Windows-native tools are reachable via interop.",
        RuleKind::Constraint,
        Scope::Environment,
    );
    let submitted = store.submit_suggestion(draft).unwrap();
    assert_eq!(submitted.scope, Scope::Environment);
    assert!(!submitted.project_path.as_os_str().is_empty());
    // Environment notes are project-local: never loaded without a project path.
    assert!(store.load_rules(None).unwrap().is_empty());

    let (reviewed, saved, path) = store
        .approve_suggestion(submitted.id, SuggestionEdits::default(), None)
        .unwrap();
    assert_eq!(reviewed.status, SuggestionStatus::Approved);
    assert_eq!(saved.scope, Scope::Environment);
    assert!(
        path.starts_with(agent_control_core::DataLayout::project_environment_dir(
            project.path()
        ))
    );
    assert!(
        store
            .load_rules(Some(project.path()))
            .unwrap()
            .iter()
            .any(|r| r.id == "wsl-env" && r.scope == Scope::Environment)
    );
    // An environment suggestion is project-bound: a project filter returns it.
    let filtered = store.list_suggestions(Some(project.path()), None).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].scope, Scope::Environment);
}

#[test]
fn environment_rule_is_non_competitive_context() {
    // An environment note sharing a topic with a project contract must NOT compete: both apply.
    let contract = make_rule("build-contract", Scope::Project, 200, Some("build"));
    let env = make_rule("wsl-build", Scope::Environment, 1, Some("build"));
    let res = RuleEngine::resolve(&[contract, env], &ctx());
    let ids: Vec<_> = res.applied.iter().map(|m| m.rule.id.as_str()).collect();
    assert!(ids.contains(&"build-contract"), "{ids:?}");
    assert!(ids.contains(&"wsl-build"), "{ids:?}");
    assert!(res.applied.iter().all(|m| m.decision == Decision::Applied));
    // Two environment notes on the same topic also coexist (descriptive context, no supersession).
    let env2 = make_rule("wsl-tools", Scope::Environment, 1, Some("build"));
    let res2 = RuleEngine::resolve(
        &[
            make_rule("wsl-build", Scope::Environment, 1, Some("build")),
            env2,
        ],
        &ctx(),
    );
    assert_eq!(res2.applied.len(), 2);
    assert!(
        res2.trace
            .iter()
            .all(|m| m.decision != Decision::Superseded)
    );
}

#[test]
fn environment_rule_load_save_toggle_delete_and_gitignore() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let env = make_rule("wsl-env", Scope::Environment, 100, None);
    let path = store.save_rule(&env, Some(project.path())).unwrap();
    let env_dir = agent_control_core::DataLayout::project_environment_dir(project.path());
    assert!(path.starts_with(&env_dir));
    // Saving creates the git-ignored local dir: everything ignored but the .gitignore itself.
    let gitignore = std::fs::read_to_string(env_dir.join(".gitignore")).unwrap();
    assert!(gitignore.contains("*"));
    assert!(gitignore.contains("!.gitignore"));

    // Loaded only with a project path, tagged Environment.
    assert!(store.load_rules(None).unwrap().is_empty());
    let loaded = store.load_rules(Some(project.path())).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].scope, Scope::Environment);

    // Toggle and delete operate on the local dir.
    let toggled = store
        .set_rule_enabled("wsl-env", Scope::Environment, Some(project.path()), false)
        .unwrap();
    assert!(!toggled.enabled);
    assert!(
        store
            .delete_rule("wsl-env", Scope::Environment, Some(project.path()))
            .unwrap()
    );
    assert!(store.load_rules(Some(project.path())).unwrap().is_empty());
}

#[test]
fn environment_excluded_from_native_export() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    store
        .save_rule(
            &make_rule("proj-contract", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();
    let mut env = make_rule("wsl-secret-env", Scope::Environment, 100, None);
    env.content = "MACHINE-ONLY-FACT-WSL".into();
    store.save_rule(&env, Some(project.path())).unwrap();

    let report = store
        .export_native(project.path(), &[ExportTarget::Agents], false)
        .unwrap();
    // Environment rules are never counted as exported project/user rules.
    assert_eq!(report.project_rules, 1);
    assert_eq!(report.user_rules, 0);
    // The committed pointer block must not inline machine-specific environment content.
    let agents = std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
    assert!(!agents.contains("MACHINE-ONLY-FACT-WSL"), "{agents}");
}

#[test]
fn scaffold_creates_environment_dir_and_example() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    store.init_project_contracts(project.path()).unwrap();
    let env_dir = agent_control_core::DataLayout::project_environment_dir(project.path());
    assert!(env_dir.join(".gitignore").exists());
    assert!(
        agent_control_core::DataLayout::project_examples_dir(project.path())
            .join("environment-example.md")
            .exists()
    );
    // The example is NOT binding: it lives in examples/, not local/ or rules/.
    assert!(store.load_rules(Some(project.path())).unwrap().is_empty());
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

#[test]
fn portable_layout_isolates_database_under_data_db() {
    let root = tempfile::tempdir().unwrap();
    let layout = agent_control_core::DataLayout::portable_with_root(root.path().to_path_buf());
    assert_eq!(layout.db_path(), root.path().join("db").join("control.db"));
    layout.ensure_dirs().unwrap();
    assert!(root.path().join("db").is_dir());

    let store = Store::open(layout).unwrap();
    store
        .register_project("Portable", root.path(), None)
        .unwrap();
    // The database lives under data/db, never at the root (global-style) location.
    assert!(root.path().join("db").join("control.db").exists());
    assert!(!root.path().join("control.db").exists());
}

#[test]
fn portable_env_flag_routes_default_layout_away_from_global() {
    // Scoped env mutation; other tests open explicit roots, not the default layout.
    unsafe { std::env::set_var("AGENT_CONTROL_PORTABLE", "1") };
    let layout = agent_control_core::DataLayout::default_layout().unwrap();
    unsafe { std::env::remove_var("AGENT_CONTROL_PORTABLE") };
    assert!(layout.portable);
    assert!(layout.root.ends_with("data"));
    assert_eq!(layout.db_path(), layout.root.join("db").join("control.db"));
}

#[test]
fn export_native_injects_pointer_block_preserving_prose() {
    use agent_control_core::export::{BLOCK_BEGIN, BLOCK_END, CURSOR_FRONTMATTER};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    // A hand-written AGENTS.md must survive export; only the managed block is ours.
    let agents = project.path().join("AGENTS.md");
    std::fs::write(&agents, "# My repo\n\nHand-written guidance. Keep me.\n").unwrap();

    store
        .save_rule(
            &make_rule("contract-1", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();
    let mut disabled = make_rule("off-rule", Scope::Project, 100, None);
    disabled.enabled = false;
    store.save_rule(&disabled, Some(project.path())).unwrap();

    let report = store
        .export_native(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    assert_eq!(report.project_rules, 1, "disabled rules are not counted");
    assert_eq!(report.user_rules, 0);
    assert_eq!(report.targets.len(), 4, "AGENTS + CLAUDE + GEMINI + cursor");

    // The block is a pointer: it names the rules dir but copies no rule text.
    let agents_text = std::fs::read_to_string(&agents).unwrap();
    assert!(agents_text.starts_with("# My repo\n\nHand-written guidance. Keep me.\n"));
    assert!(agents_text.contains(BLOCK_BEGIN) && agents_text.contains(BLOCK_END));
    assert!(agents_text.contains(".agent-control/rules/"));
    assert!(
        !agents_text.contains("contract-1"),
        "no rule text is inlined"
    );
    assert!(!agents_text.contains("off-rule"));

    let cursor = project.path().join(".cursor/rules/agent-control.mdc");
    let cursor_text = std::fs::read_to_string(&cursor).unwrap();
    assert!(cursor_text.starts_with(CURSOR_FRONTMATTER));
    assert!(cursor_text.contains(".agent-control/rules/"));

    let claude = std::fs::read_to_string(project.path().join("CLAUDE.md")).unwrap();
    assert!(claude.contains(BLOCK_BEGIN) && claude.contains(".agent-control/rules/"));
    let gemini = std::fs::read_to_string(project.path().join("GEMINI.md")).unwrap();
    assert!(gemini.contains(BLOCK_BEGIN) && gemini.contains(".agent-control/rules/"));
}

#[test]
fn export_native_default_targets_agents_only() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let report = store
        .export_native(project.path(), &ExportTarget::default_targets(), false)
        .unwrap();
    assert_eq!(report.targets.len(), 1, "default writes AGENTS.md only");
    assert!(project.path().join("AGENTS.md").exists());
    assert!(!project.path().join("CLAUDE.md").exists());
    assert!(!project.path().join("GEMINI.md").exists());
    assert!(
        !project
            .path()
            .join(".cursor/rules/agent-control.mdc")
            .exists()
    );
}

#[test]
fn export_native_reexport_idempotent_even_after_rule_edit() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    store
        .save_rule(
            &make_rule("r1", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();

    let targets = ExportTarget::default_targets();
    let first = store
        .export_native(project.path(), &targets, false)
        .unwrap();
    assert!(first.targets.iter().all(|t| t.action == "created"));

    let second = store
        .export_native(project.path(), &targets, false)
        .unwrap();
    assert!(
        second.targets.iter().all(|t| t.action == "unchanged"),
        "idempotent re-export must not rewrite files"
    );

    // Edit the rule and add user prose outside the block. The pointer body is static, so re-export
    // stays unchanged (a rule edit never dirties the export) and the hand-written prose survives.
    let mut updated = make_rule("r1", Scope::Project, 100, None);
    updated.content = "revised contract body".into();
    store.save_rule(&updated, Some(project.path())).unwrap();
    let agents = project.path().join("AGENTS.md");
    let mut text = std::fs::read_to_string(&agents).unwrap();
    text.push_str("\n## User notes\n\nadded by hand\n");
    std::fs::write(&agents, &text).unwrap();

    let third = store
        .export_native(project.path(), &targets, false)
        .unwrap();
    let agents_outcome = third.targets.iter().find(|t| t.path == agents).unwrap();
    assert_eq!(
        agents_outcome.action, "unchanged",
        "static pointer never varies"
    );
    let out = std::fs::read_to_string(&agents).unwrap();
    assert!(out.contains("## User notes\n\nadded by hand"));
    assert!(
        !out.contains("revised contract body"),
        "rule text is not inlined"
    );
}

#[test]
fn export_drift_flags_legacy_format_and_clears_on_reexport() {
    use agent_control_core::export::{BLOCK_BEGIN, BLOCK_END, BLOCK_FORMAT};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    store
        .save_rule(
            &make_rule("r1", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();

    // Never exported: targets do not exist, so nothing is "stale" yet.
    let fresh = store
        .export_drift(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    assert!(!fresh.stale);
    assert_eq!(fresh.targets.len(), 4);
    assert!(fresh.targets.iter().all(|t| !t.exists));

    store
        .export_native(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    let synced = store
        .export_drift(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    assert!(!synced.stale, "freshly exported files are in sync");
    assert!(synced.targets.iter().all(|t| t.exists));
    assert!(
        synced
            .targets
            .iter()
            .all(|t| t.exported_format == Some(t.current_format))
    );

    // Editing a rule does NOT cause drift: the pointer block is static.
    let mut updated = make_rule("r1", Scope::Project, 100, None);
    updated.content = "revised body".into();
    store.save_rule(&updated, Some(project.path())).unwrap();
    let after_edit = store
        .export_drift(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    assert!(
        !after_edit.stale,
        "a rule edit never dirties the pointer block"
    );

    // Simulate a legacy v1 full-text block (ruleset hash, no format marker) in AGENTS.md.
    let agents = project.path().join("AGENTS.md");
    let legacy = format!(
        "# Repo\n\n{BLOCK_BEGIN}\n<!-- agent-control:ruleset sha256:deadbeef -->\n## Contracts\n\nold full text\n{BLOCK_END}\n"
    );
    std::fs::write(&agents, legacy).unwrap();
    let drifted = store
        .export_drift(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    assert!(drifted.stale, "legacy format is flagged for migration");
    let agents_drift = drifted.targets.iter().find(|t| t.path == agents).unwrap();
    assert!(agents_drift.stale);
    assert_eq!(agents_drift.exported_format, None);
    assert_eq!(agents_drift.current_format, BLOCK_FORMAT);

    // Re-export migrates the block and clears the drift.
    store
        .export_native(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    let resynced = store
        .export_drift(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    assert!(!resynced.stale);
}

#[test]
fn revoke_native_removes_block_preserves_prose_deletes_stub() {
    use agent_control_core::export::{BLOCK_BEGIN, BLOCK_END};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    // AGENTS.md and CLAUDE.md carry hand-written prose; GEMINI.md and cursor will be block-only.
    let agents = project.path().join("AGENTS.md");
    let claude = project.path().join("CLAUDE.md");
    std::fs::write(&agents, "# My repo\n\nKeep this prose.\n").unwrap();
    std::fs::write(&claude, "# Claude notes\n\nKeep me too.\n").unwrap();

    store
        .export_native(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    let gemini = project.path().join("GEMINI.md");
    let cursor = project.path().join(".cursor/rules/agent-control.mdc");
    assert!(gemini.exists() && cursor.exists());

    let report = store
        .revoke_native(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    let action = |p: &std::path::Path| {
        report
            .targets
            .iter()
            .find(|t| t.path == p)
            .map(|t| t.action.clone())
            .unwrap()
    };
    assert_eq!(action(&agents), "removed");
    assert_eq!(action(&claude), "removed");
    assert_eq!(action(&gemini), "deleted");
    assert_eq!(action(&cursor), "deleted");

    let agents_text = std::fs::read_to_string(&agents).unwrap();
    assert!(agents_text.contains("Keep this prose."));
    assert!(!agents_text.contains(BLOCK_BEGIN) && !agents_text.contains(BLOCK_END));
    let claude_text = std::fs::read_to_string(&claude).unwrap();
    assert!(claude_text.contains("Keep me too."));
    assert!(!gemini.exists(), "block-only file is deleted");
    assert!(!cursor.exists(), "frontmatter+block-only file is deleted");

    // Revoking again is a no-op.
    let again = store
        .revoke_native(project.path(), &ExportTarget::ALL, false)
        .unwrap();
    assert!(again.targets.iter().all(|t| t.action == "unchanged"));
}

#[test]
fn verify_constraints_computes_achieved_tiers_and_records_evidence() {
    use agent_control_core::model::{AssuranceTier, Enforcement};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join(".editorconfig"),
        "[*]\nindent_style = space\n",
    )
    .unwrap();

    let mut passing = make_rule("pass-check", Scope::Project, 100, None);
    passing.code_checks = vec![CodeCheck {
        file: ".editorconfig".into(),
        key: "indent_style".into(),
        expect: "space".into(),
        description: None,
    }];
    // A semantic rule declared `hard` still caps at Advisory: no chokepoint, no check.
    let mut semantic = make_rule("semantic", Scope::Project, 90, None);
    semantic.enforcement = Enforcement::Hard;
    let mut failing = make_rule("fail-check", Scope::Project, 80, None);
    failing.code_checks = vec![CodeCheck {
        file: ".editorconfig".into(),
        key: "indent_style".into(),
        expect: "tab".into(),
        description: None,
    }];
    for rule in [&passing, &semantic, &failing] {
        store.save_rule(rule, Some(project.path())).unwrap();
    }

    let session = store
        .create_session("verify", project.path(), "tester", vec![])
        .unwrap()
        .0;
    assert_eq!(session.constraint_snapshot.rules.len(), 3);

    let report = store.verify_constraints(session.id, "tester").unwrap();
    assert_eq!(report.verified, 1);
    assert_eq!(report.advisory, 2);
    assert_eq!(report.enforced, 0);

    let pass = report
        .assurances
        .iter()
        .find(|a| a.rule_id == "pass-check")
        .unwrap();
    assert_eq!(pass.achieved, AssuranceTier::Verified);
    let sem = report
        .assurances
        .iter()
        .find(|a| a.rule_id == "semantic")
        .unwrap();
    assert_eq!(sem.declared, Enforcement::Hard);
    assert_eq!(sem.achieved, AssuranceTier::Advisory);
    let fail = report
        .assurances
        .iter()
        .find(|a| a.rule_id == "fail-check")
        .unwrap();
    assert_eq!(fail.achieved, AssuranceTier::Advisory);

    // Achieved tiers persist on the session, separate from the frozen snapshot.
    let reloaded = store.get_session(session.id).unwrap().unwrap();
    assert_eq!(reloaded.assurance.len(), 3);
    assert_eq!(
        reloaded.constraint_snapshot.snapshot_hash,
        session.constraint_snapshot.snapshot_hash
    );

    // Verification is recorded in the evidence chain, including the failure.
    assert!(store.verify_chain(session.id).unwrap().valid);
    let evidence = store.get_evidence(session.id).unwrap();
    assert!(
        evidence
            .iter()
            .any(|e| e.kind == EvidenceKind::ConstraintViolation)
    );
}

#[test]
fn handoff_carries_assurance_baseline() {
    use agent_control_core::model::AssuranceTier;

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join(".editorconfig"),
        "[*]\nindent_style = space\n",
    )
    .unwrap();
    let mut rule = make_rule("checked", Scope::Project, 100, None);
    rule.code_checks = vec![CodeCheck {
        file: ".editorconfig".into(),
        key: "indent_style".into(),
        expect: "space".into(),
        description: None,
    }];
    store.save_rule(&rule, Some(project.path())).unwrap();

    let session = store
        .create_session("handoff", project.path(), "harness-a", vec![])
        .unwrap()
        .0;
    store.verify_constraints(session.id, "harness-a").unwrap();

    let bundle = store
        .export_handoff(
            session.id,
            Some("harness-b"),
            serde_json::json!({"summary": "verified"}),
        )
        .unwrap();
    assert!(bundle.verify_integrity());
    assert_eq!(bundle.assurance.len(), 1);
    assert_eq!(bundle.assurance[0].achieved, AssuranceTier::Verified);

    let resumed = store.import_handoff(&bundle, "harness-b").unwrap();
    assert_eq!(resumed.agent_id, "harness-b");
    assert_eq!(
        resumed.assurance.len(),
        1,
        "source baseline carries across the handoff"
    );
}

#[test]
fn export_handoff_writes_project_latest_md() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    store
        .save_rule(
            &make_rule("c1", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();
    let session = store
        .create_session("layer 1 continuity", project.path(), "harness-a", vec![])
        .unwrap()
        .0;
    let bundle = store
        .export_handoff(session.id, Some("harness-b"), serde_json::json!({}))
        .unwrap();

    let latest = project.path().join(".agent-control/handoff/latest.md");
    assert!(
        latest.exists(),
        "export_handoff must write the project-local continuity brief"
    );
    let text = std::fs::read_to_string(&latest).unwrap();
    assert!(text.contains("layer 1 continuity"), "{text}");
    assert!(text.contains("harness-a"));
    assert!(text.contains("c1"));
    assert!(text.contains(&bundle.evidence_head_hash));
    // The JSON bundle still lands in the global bundles dir.
    assert!(
        root.path()
            .join("bundles")
            .join(format!("{}.json", session.id))
            .exists()
    );
}

#[test]
fn import_handoff_refreshes_latest_md_with_resumed_state() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join(".editorconfig"),
        "[*]\nindent_style = space\n",
    )
    .unwrap();
    let mut rule = make_rule("checked", Scope::Project, 100, None);
    rule.code_checks = vec![CodeCheck {
        file: ".editorconfig".into(),
        key: "indent_style".into(),
        expect: "space".into(),
        description: None,
    }];
    store.save_rule(&rule, Some(project.path())).unwrap();

    let session = store
        .create_session("resume brief", project.path(), "harness-a", vec![])
        .unwrap()
        .0;
    store.verify_constraints(session.id, "harness-a").unwrap();
    let bundle = store
        .export_handoff(session.id, Some("harness-b"), serde_json::json!({}))
        .unwrap();

    let latest = project.path().join(".agent-control/handoff/latest.md");
    let exported = std::fs::read_to_string(&latest).unwrap();
    assert!(
        exported.contains("Generated by `export_handoff`"),
        "{exported}"
    );
    assert!(!exported.contains("Resumed under"), "{exported}");

    store.import_handoff(&bundle, "harness-b").unwrap();
    let resumed = std::fs::read_to_string(&latest).unwrap();
    assert!(
        resumed.contains("Refreshed by `import_handoff`"),
        "{resumed}"
    );
    assert!(
        resumed.contains("**Resumed under**: harness-b"),
        "{resumed}"
    );
    assert!(resumed.contains("harness-a"), "{resumed}");
    // A carried Verified tier triggers the re-verify warning in the resumed brief.
    assert!(resumed.contains("SOURCE harness"), "{resumed}");
    assert!(resumed.contains("verify_constraints"), "{resumed}");
}

#[test]
fn export_native_project_block_carries_continuity_guidance() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    store
        .save_rule(
            &make_rule("c1", Scope::Project, 100, None),
            Some(project.path()),
        )
        .unwrap();

    store
        .export_native(project.path(), &ExportTarget::default_targets(), false)
        .unwrap();
    let agents = std::fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("Continuity"), "{agents}");
    assert!(agents.contains(".agent-control/handoff/latest.md"));
    // The pointer block carries the current format, so drift agrees the fresh export is in sync.
    let drift = store
        .export_drift(project.path(), &ExportTarget::default_targets(), false)
        .unwrap();
    assert!(!drift.stale);
}

#[test]
fn tool_guards_roundtrip_through_frontmatter() {
    use agent_control_core::model::{Enforcement, ToolGuard};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut rule = make_rule("guard-rule", Scope::Project, 100, None);
    rule.enforcement = Enforcement::Hard;
    rule.tool_guards = vec![ToolGuard {
        tool: "Bash".into(),
        pattern: r"rm\s+-rf".into(),
        reason: "no destructive rm".into(),
    }];
    store.save_rule(&rule, Some(project.path())).unwrap();

    let loaded = store
        .load_rules(Some(project.path()))
        .unwrap()
        .into_iter()
        .find(|r| r.id == "guard-rule")
        .unwrap();
    assert_eq!(loaded.enforcement, Enforcement::Hard);
    assert_eq!(loaded.tool_guards.len(), 1);
    assert_eq!(loaded.tool_guards[0].tool, "Bash");
    assert_eq!(loaded.tool_guards[0].pattern, r"rm\s+-rf");
    assert_eq!(loaded.tool_guards[0].reason, "no destructive rm");
}

#[test]
fn check_tool_call_repo_gate_denies_while_hard_check_fails() {
    use agent_control_core::model::{Enforcement, GateSource};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join(".editorconfig"),
        "[*]\nindent_style = space\n",
    )
    .unwrap();
    let mut rule = make_rule("repo-gate", Scope::Project, 100, None);
    rule.enforcement = Enforcement::Hard;
    rule.code_checks = vec![CodeCheck {
        file: ".editorconfig".into(),
        key: "indent_style".into(),
        expect: "tab".into(),
        description: None,
    }];
    store.save_rule(&rule, Some(project.path())).unwrap();

    let report = store
        .check_tool_call(
            project.path(),
            "Bash",
            &serde_json::json!({"command": "ls"}),
            "tester",
            None,
        )
        .unwrap();
    assert!(!report.allowed);
    assert_eq!(report.hard_rules_checked, 1);
    assert!(report.enforced_rules.is_empty());
    assert_eq!(report.denials[0].source, GateSource::RepoGate);
    assert!(report.denial_reason().contains("repo-gate"));

    // Fixing the repo lifts the gate.
    std::fs::write(
        project.path().join(".editorconfig"),
        "[*]\nindent_style = tab\n",
    )
    .unwrap();
    let ok = store
        .check_tool_call(
            project.path(),
            "Bash",
            &serde_json::json!({"command": "ls"}),
            "tester",
            None,
        )
        .unwrap();
    assert!(ok.allowed);
    assert_eq!(ok.enforced_rules, vec!["repo-gate".to_string()]);
}

#[test]
fn check_tool_call_tool_guard_denies_matching_command_only() {
    use agent_control_core::model::{Enforcement, GateSource, ToolGuard};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut rule = make_rule("no-rm", Scope::Project, 100, None);
    rule.enforcement = Enforcement::Hard;
    rule.tool_guards = vec![ToolGuard {
        tool: "Bash".into(),
        pattern: r"rm\s+-rf".into(),
        reason: "destructive rm blocked".into(),
    }];
    store.save_rule(&rule, Some(project.path())).unwrap();

    let denied = store
        .check_tool_call(
            project.path(),
            "Bash",
            &serde_json::json!({"command": "rm -rf /tmp/x"}),
            "tester",
            None,
        )
        .unwrap();
    assert!(!denied.allowed);
    assert_eq!(denied.denials[0].source, GateSource::ToolGuard);
    assert_eq!(denied.denials[0].reason, "destructive rm blocked");

    // A non-matching command on the same tool is allowed and mediated.
    let allowed = store
        .check_tool_call(
            project.path(),
            "Bash",
            &serde_json::json!({"command": "ls -la"}),
            "tester",
            None,
        )
        .unwrap();
    assert!(allowed.allowed);
    assert_eq!(allowed.enforced_rules, vec!["no-rm".to_string()]);

    // A different tool is outside the guard's glob.
    let write = store
        .check_tool_call(
            project.path(),
            "Write",
            &serde_json::json!({"file_path": "a.txt", "content": "rm -rf"}),
            "tester",
            None,
        )
        .unwrap();
    assert!(write.allowed);
}

#[test]
fn check_tool_call_marks_enforced_and_records_evidence() {
    use agent_control_core::model::{AssuranceTier, Enforcement};

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join(".editorconfig"),
        "[*]\nindent_style = space\n",
    )
    .unwrap();
    let mut rule = make_rule("hard-checked", Scope::Project, 100, None);
    rule.enforcement = Enforcement::Hard;
    rule.code_checks = vec![CodeCheck {
        file: ".editorconfig".into(),
        key: "indent_style".into(),
        expect: "space".into(),
        description: None,
    }];
    store.save_rule(&rule, Some(project.path())).unwrap();

    let session = store
        .create_session("gate", project.path(), "tester", vec![])
        .unwrap()
        .0;
    let report = store
        .check_tool_call(
            project.path(),
            "Bash",
            &serde_json::json!({"command": "cargo test"}),
            "tester",
            Some(session.id),
        )
        .unwrap();
    assert!(report.allowed);
    assert_eq!(report.enforced_rules, vec!["hard-checked".to_string()]);

    // The chokepoint produces the Enforced tier (verify_constraints never does).
    let reloaded = store.get_session(session.id).unwrap().unwrap();
    let assurance = reloaded
        .assurance
        .iter()
        .find(|a| a.rule_id == "hard-checked")
        .unwrap();
    assert_eq!(assurance.declared, Enforcement::Hard);
    assert_eq!(assurance.achieved, AssuranceTier::Enforced);

    assert!(store.verify_chain(session.id).unwrap().valid);
    let evidence = store.get_evidence(session.id).unwrap();
    assert!(evidence.iter().any(|e| e.kind == EvidenceKind::ToolGate));
}

#[test]
fn check_tool_call_allows_when_no_enabled_hard_rules() {
    use agent_control_core::model::Enforcement;

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    // An advisory rule and a disabled hard rule: neither gates the call.
    let advisory = make_rule("soft", Scope::Project, 100, None);
    let mut disabled = make_rule("disabled-hard", Scope::Project, 100, None);
    disabled.enforcement = Enforcement::Hard;
    disabled.enabled = false;
    store.save_rule(&advisory, Some(project.path())).unwrap();
    store.save_rule(&disabled, Some(project.path())).unwrap();

    let report = store
        .check_tool_call(
            project.path(),
            "Bash",
            &serde_json::json!({"command": "rm -rf /"}),
            "tester",
            None,
        )
        .unwrap();
    assert!(report.allowed);
    assert_eq!(report.hard_rules_checked, 0);
    assert!(report.enforced_rules.is_empty());
}

#[test]
fn register_hooks_preserves_config_and_is_idempotent() {
    use agent_control_core::HookTarget;

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let settings = project.path().join(".claude").join("settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(
        &settings,
        r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo user"}]}]}}"#,
    )
    .unwrap();

    let cmd = "node \"C:/repo/scripts/agent-control-hook.mjs\"";
    let report = store
        .register_hooks(project.path(), &[HookTarget::Claude], cmd)
        .unwrap();
    assert_eq!(report.targets[0].action, "updated");

    let raw = std::fs::read_to_string(&settings).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["model"], "opus", "other config preserved");
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2, "user hook kept beside ours");
    assert_eq!(pre[0]["hooks"][0]["command"], "echo user");
    assert!(v["hooks"]["SessionStart"].as_array().unwrap().len() == 1);

    // Re-registering an up-to-date file is a no-op on disk.
    let again = store
        .register_hooks(project.path(), &[HookTarget::Claude], cmd)
        .unwrap();
    assert_eq!(again.targets[0].action, "unchanged");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), raw);
}

#[test]
fn revoke_hooks_preserves_user_config_and_deletes_stub() {
    use agent_control_core::HookTarget;

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();
    let settings = project.path().join(".claude").join("settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(
        &settings,
        r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo user"}]}]}}"#,
    )
    .unwrap();
    let cmd = "node \"C:/repo/scripts/agent-control-hook.mjs\"";
    store
        .register_hooks(project.path(), &[HookTarget::Claude], cmd)
        .unwrap();

    let report = store
        .revoke_hooks(project.path(), &[HookTarget::Claude])
        .unwrap();
    assert_eq!(report.targets[0].action, "removed");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v["model"], "opus");
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0]["hooks"][0]["command"], "echo user");
    assert!(
        v["hooks"].get("SessionStart").is_none(),
        "our-only event pruned"
    );

    // A settings file that held only our hook is deleted on revoke.
    let buddy = project.path().join(".codebuddy").join("settings.json");
    store
        .register_hooks(project.path(), &[HookTarget::CodeBuddy], cmd)
        .unwrap();
    assert!(buddy.exists());
    let deleted = store
        .revoke_hooks(project.path(), &[HookTarget::CodeBuddy])
        .unwrap();
    assert_eq!(deleted.targets[0].action, "deleted");
    assert!(!buddy.exists());

    // WorkBuddy is a separate product with its own .workbuddy/settings.json.
    let work = project.path().join(".workbuddy").join("settings.json");
    store
        .register_hooks(project.path(), &[HookTarget::WorkBuddy], cmd)
        .unwrap();
    assert!(work.exists());
    assert!(
        !buddy.exists(),
        "registering WorkBuddy must not touch CodeBuddy"
    );
    let deleted = store
        .revoke_hooks(project.path(), &[HookTarget::WorkBuddy])
        .unwrap();
    assert_eq!(deleted.targets[0].action, "deleted");
    assert!(!work.exists());
}

#[test]
fn detect_hooks_reports_registration() {
    use agent_control_core::HookTarget;

    let root = tempfile::tempdir().unwrap();
    let store = Store::open_with_root(root.path().to_path_buf()).unwrap();
    let project = tempfile::tempdir().unwrap();

    let before = store.detect_hooks(project.path()).unwrap();
    assert_eq!(before.len(), 4);
    assert!(before.iter().all(|s| !s.registered));

    let cmd = "node \"C:/repo/scripts/agent-control-hook.mjs\"";
    store
        .register_hooks(project.path(), &[HookTarget::Qoder], cmd)
        .unwrap();
    let after = store.detect_hooks(project.path()).unwrap();
    let qoder = after
        .iter()
        .find(|s| s.target == HookTarget::Qoder)
        .unwrap();
    assert!(qoder.registered);
    assert!(qoder.installed, "project .qoder dir now exists");
    assert!(
        after
            .iter()
            .filter(|s| s.target != HookTarget::Qoder)
            .all(|s| !s.registered)
    );
}

#[test]
fn app_config_toml_roundtrip_and_defaults() {
    let root = tempfile::tempdir().unwrap();
    let layout = DataLayout::new(root.path().to_path_buf());
    let cfg = AppConfig::load(&layout);
    assert_eq!(cfg, AppConfig::default());
    assert_eq!(cfg.mcp_port, DEFAULT_MCP_PORT);

    let custom = AppConfig {
        locale: "zh".into(),
        mcp_port: 9871,
    };
    custom.save(&layout).unwrap();
    assert!(layout.config_file().exists());
    assert_eq!(AppConfig::load(&layout), custom);

    // A partial TOML file fills missing fields from the defaults.
    std::fs::write(layout.config_file(), "locale = \"en\"\n").unwrap();
    assert_eq!(AppConfig::load(&layout).mcp_port, DEFAULT_MCP_PORT);
}

#[test]
fn app_config_migrates_legacy_ui_json_once() {
    let root = tempfile::tempdir().unwrap();
    let layout = DataLayout::new(root.path().to_path_buf());
    let legacy = root.path().join("ui.json");
    std::fs::write(&legacy, r#"{"locale":"zh"}"#).unwrap();

    let cfg = AppConfig::load(&layout);
    assert_eq!(cfg.locale, "zh");
    assert_eq!(cfg.mcp_port, DEFAULT_MCP_PORT);
    assert!(layout.config_file().exists(), "toml written");
    assert!(!legacy.exists(), "legacy json removed after conversion");
    assert_eq!(AppConfig::load(&layout).locale, "zh");
}
