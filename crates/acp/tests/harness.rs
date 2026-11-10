use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agent_control_acp::{
    HarnessCommand, HarnessConfig, HarnessEvent, HarnessManager, PermissionDecision,
    PermissionPolicy,
};
use agent_control_core::Store;
use agent_control_core::model::{AgentProtocol, AgentRegistration, EvidenceKind};
use tokio::sync::mpsc;
use tokio::time::timeout;
use uuid::Uuid;

const EVENT_TIMEOUT: Duration = Duration::from_secs(30);

fn mock_script() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/mock-acp-agent.mjs")
        .canonicalize()
        .expect("mock-acp-agent.mjs must exist")
        .to_string_lossy()
        .into_owned()
}

fn setup(
    permission: bool,
) -> (
    tempfile::TempDir,
    Arc<Store>,
    HarnessManager,
    mpsc::Sender<HarnessCommand>,
    mpsc::Receiver<HarnessEvent>,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_with_root(dir.path().join("data")).unwrap());
    let rule = agent_control_core::store::new_user_rule("test-rule", "Test rule", "Always test.");
    store.save_rule(&rule, None).unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let mut env = HashMap::new();
    if permission {
        env.insert("MOCK_PERMISSION".to_string(), "1".to_string());
    }
    let registration = AgentRegistration {
        id: "mock".into(),
        name: "Mock agent".into(),
        command: "node".into(),
        args: vec![mock_script()],
        env,
        protocol: Some(AgentProtocol::Acp),
    };
    let manager = HarnessManager::new(store.clone()).unwrap();
    let channels = manager
        .start(HarnessConfig {
            registration,
            project_path: project,
            task: "integration test".into(),
            task_tags: vec![],
            mcp_endpoint: None,
            permission_policy: PermissionPolicy::Ask,
        })
        .unwrap();
    (dir, store, manager, channels.command_tx, channels.event_rx)
}

async fn next_event(rx: &mut mpsc::Receiver<HarnessEvent>) -> HarnessEvent {
    timeout(EVENT_TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for harness event")
        .expect("harness event channel closed")
}

async fn wait_session(rx: &mut mpsc::Receiver<HarnessEvent>) -> (Uuid, usize, bool) {
    let mut supports_system_prompt = false;
    loop {
        match next_event(rx).await {
            HarnessEvent::Connected {
                supports_system_prompt: sp,
                ..
            } => {
                supports_system_prompt = sp;
            }
            HarnessEvent::SessionCreated {
                control_session_id,
                rule_count,
                ..
            } => {
                return (control_session_id, rule_count, supports_system_prompt);
            }
            HarnessEvent::Error { message } => panic!("harness error: {message}"),
            _ => {}
        }
    }
}

#[test]
fn harness_prompt_records_verifiable_evidence() {
    let (dir, store, manager, tx, mut rx) = setup(false);
    manager.runtime().block_on(async {
        let (session_id, rule_count, supports_system_prompt) = wait_session(&mut rx).await;
        assert!(
            supports_system_prompt,
            "mock advertises system prompt support"
        );
        assert_eq!(rule_count, 1, "snapshot must bind the seeded rule");

        tx.send(HarnessCommand::Prompt {
            text: "do the work".into(),
        })
        .await
        .unwrap();

        let mut message = String::new();
        let mut saw_evidence = false;
        let mut completed = false;
        while !saw_evidence || !completed || message.is_empty() {
            match next_event(&mut rx).await {
                HarnessEvent::AgentMessage { text } => message.push_str(&text),
                HarnessEvent::EvidenceRecorded { .. } => saw_evidence = true,
                HarnessEvent::PromptComplete { stop_reason } => {
                    assert!(
                        stop_reason.contains("EndTurn"),
                        "stop reason: {stop_reason}"
                    );
                    completed = true;
                }
                HarnessEvent::Error { message } => panic!("harness error: {message}"),
                _ => {}
            }
        }
        assert!(
            message.contains("systemPrompt=true"),
            "briefing must reach the agent: {message}"
        );

        let entries = store.get_evidence(session_id).unwrap();
        assert!(
            entries.iter().any(|e| e.kind == EvidenceKind::FileChange),
            "agent file write must be recorded as evidence"
        );
        assert!(store.verify_chain(session_id).unwrap().valid);
        assert!(dir.path().join("project/mock-output.txt").exists());

        tx.send(HarnessCommand::Shutdown).await.unwrap();
        loop {
            if matches!(next_event(&mut rx).await, HarnessEvent::Disconnected) {
                break;
            }
        }
    });
}

#[test]
fn harness_permission_request_round_trip() {
    let (_dir, _store, manager, tx, mut rx) = setup(true);
    manager.runtime().block_on(async {
        let (_session_id, _, _) = wait_session(&mut rx).await;
        tx.send(HarnessCommand::Prompt {
            text: "needs permission".into(),
        })
        .await
        .unwrap();

        let mut approved = false;
        let mut outcome_seen = false;
        let mut completed = false;
        while !outcome_seen || !completed {
            match next_event(&mut rx).await {
                HarnessEvent::PermissionRequest {
                    tool_title,
                    options,
                    responder,
                    ..
                } => {
                    assert_eq!(tool_title, "Run mock tests");
                    assert!(options.iter().any(|o| o.kind == "AllowOnce"));
                    responder.send(PermissionDecision::AllowOnce).unwrap();
                    approved = true;
                }
                HarnessEvent::AgentMessage { text } => {
                    if text.contains("opt-allow") {
                        assert!(approved, "decision must precede agent outcome");
                        outcome_seen = true;
                    }
                }
                HarnessEvent::PromptComplete { .. } => completed = true,
                HarnessEvent::Error { message } => panic!("harness error: {message}"),
                _ => {}
            }
        }
        tx.send(HarnessCommand::Shutdown).await.unwrap();
    });
}
