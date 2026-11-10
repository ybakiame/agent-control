use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{
    CancelNotification, ClientCapabilities, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, FileSystemCapabilities, Implementation, InitializeRequest,
    KillTerminalRequest, McpServer, McpServerHttp, McpServerStdio, NewSessionRequest,
    PermissionOptionKind, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    SystemPrompt, TerminalExitStatus, TerminalOutputRequest, TerminalOutputResponse, TextContent,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client};
use agent_control_core::Store;
use agent_control_core::model::{AgentRegistration, EvidenceKind};
use anyhow::Result;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::events::{
    HarnessCommand, HarnessEvent, McpEndpoint, PermissionDecision, PermissionOptionInfo,
    PermissionPolicy,
};
use crate::terminal::TerminalManager;

pub struct HarnessConfig {
    pub registration: AgentRegistration,
    pub project_path: PathBuf,
    pub task: String,
    pub task_tags: Vec<String>,
    pub mcp_endpoint: Option<McpEndpoint>,
    pub permission_policy: PermissionPolicy,
}

pub fn constraint_briefing(rules: &[agent_control_core::model::Rule]) -> String {
    let mut text = String::from(
        "# Binding Engineering Constraints\n\n\
         The following rules come from the agent-control console with verified provenance. \
         They are binding for this session. Record evidence of constraint-relevant actions \
         via the agent-control MCP server (record_evidence tool).\n\n",
    );
    for rule in rules {
        text.push_str(&format!(
            "## {} [scope={} kind={} priority={}]\n{}\n\n",
            rule.title, rule.scope, rule.kind, rule.priority, rule.content
        ));
    }
    text
}

pub async fn run_harness(
    config: HarnessConfig,
    store: Arc<Store>,
    mut command_rx: mpsc::Receiver<HarnessCommand>,
    event_tx: mpsc::Sender<HarnessEvent>,
) -> Result<()> {
    let session = store.create_session(
        &config.task,
        &config.project_path,
        &config.registration.id,
        config.task_tags.clone(),
    )?;
    let control_session_id = session.id;
    let briefing = constraint_briefing(&session.constraint_snapshot.rules);
    let agent_id = config.registration.id.clone();
    let project_path = config.project_path.clone();
    let terminals = TerminalManager::new();

    let mut agent_config = AcpAgentConfig::new(config.registration.command.clone());
    for arg in &config.registration.args {
        agent_config = agent_config.arg(arg.clone());
    }
    for (k, v) in &config.registration.env {
        agent_config = agent_config.env(k.clone(), v.clone());
    }
    let agent = AcpAgent::new(agent_config);

    let notify_store = store.clone();
    let notify_events = event_tx.clone();
    let notify_agent = agent_id.clone();

    let perm_events = event_tx.clone();
    let perm_policy = config.permission_policy;

    let fs_root = project_path.clone();

    let write_store = store.clone();
    let write_root = project_path.clone();
    let write_agent = agent_id.clone();
    let write_events = event_tx.clone();

    let term_create = terminals.clone();
    let term_output = terminals.clone();
    let term_wait = terminals.clone();
    let term_kill = terminals.clone();
    let term_release = terminals.clone();
    let term_root = project_path.clone();

    let mcp_endpoint = config.mcp_endpoint.clone();
    let main_events = event_tx.clone();
    let main_briefing = briefing.clone();
    let main_project = project_path.clone();
    let snapshot_hash = session.constraint_snapshot.snapshot_hash.clone();
    let rule_count = session.constraint_snapshot.rules.len();

    let result = Client
        .v1()
        .name("agent-control")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                handle_session_update(
                    &notification,
                    &notify_store,
                    control_session_id,
                    &notify_agent,
                    &notify_events,
                )
                .await;
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                let decision = match perm_policy {
                    PermissionPolicy::AutoApprove => PermissionDecision::AllowOnce,
                    PermissionPolicy::AutoReject => PermissionDecision::RejectOnce,
                    PermissionPolicy::Ask => {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        let options: Vec<PermissionOptionInfo> = request
                            .options
                            .iter()
                            .map(|o| PermissionOptionInfo {
                                option_id: o.option_id.0.to_string(),
                                name: o.name.clone(),
                                kind: format!("{:?}", o.kind),
                            })
                            .collect();
                        let title = request
                            .tool_call
                            .fields
                            .title
                            .clone()
                            .unwrap_or_else(|| "tool call".into());
                        let _ = perm_events
                            .send(HarnessEvent::PermissionRequest {
                                acp_session_id: request.session_id.0.to_string(),
                                tool_title: title,
                                options,
                                responder: tx,
                            })
                            .await;
                        rx.await.unwrap_or(PermissionDecision::Cancel)
                    }
                };
                let outcome = match decision {
                    PermissionDecision::Cancel => RequestPermissionOutcome::Cancelled,
                    decision => {
                        let wanted = match decision {
                            PermissionDecision::AllowOnce => PermissionOptionKind::AllowOnce,
                            PermissionDecision::AllowAlways => PermissionOptionKind::AllowAlways,
                            PermissionDecision::RejectOnce => PermissionOptionKind::RejectOnce,
                            PermissionDecision::RejectAlways => PermissionOptionKind::RejectAlways,
                            PermissionDecision::Cancel => unreachable!(),
                        };
                        match request.options.iter().find(|o| o.kind == wanted) {
                            Some(option) => RequestPermissionOutcome::Selected(
                                SelectedPermissionOutcome::new(option.option_id.clone()),
                            ),
                            None => RequestPermissionOutcome::Cancelled,
                        }
                    }
                };
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, _cx| match crate::fs::read_file(
                &fs_root,
                &request.path,
            ) {
                Ok(content) => responder.respond(ReadTextFileResponse::new(content)),
                Err(e) => responder
                    .respond_with_error(agent_client_protocol::Error::new(-32602, e.to_string())),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, _cx| {
                let result = crate::fs::write_file_with_evidence(
                    &write_store,
                    control_session_id,
                    &write_agent,
                    &write_root,
                    &request.path,
                    &request.content,
                );
                match result {
                    Ok(()) => {
                        let _ = write_events
                            .send(HarnessEvent::EvidenceRecorded {
                                seq: 0,
                                kind: "file_change".into(),
                            })
                            .await;
                        responder.respond(WriteTextFileResponse::new())
                    }
                    Err(e) => responder.respond_with_error(agent_client_protocol::Error::new(
                        -32602,
                        e.to_string(),
                    )),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateTerminalRequest, responder, _cx| {
                let env: Vec<(String, String)> = request
                    .env
                    .iter()
                    .map(|e| (e.name.clone(), e.value.clone()))
                    .collect();
                let cwd = request.cwd.clone().unwrap_or_else(|| term_root.clone());
                match term_create
                    .create(
                        &request.command,
                        &request.args,
                        &env,
                        Some(&cwd),
                        request.output_byte_limit,
                    )
                    .await
                {
                    Ok(id) => responder.respond(CreateTerminalResponse::new(id)),
                    Err(e) => responder.respond_with_error(agent_client_protocol::Error::new(
                        -32603,
                        e.to_string(),
                    )),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: TerminalOutputRequest, responder, _cx| match term_output
                .output(&request.terminal_id.0)
                .await
            {
                Ok((output, truncated, exit_code)) => {
                    let response = TerminalOutputResponse::new(output, truncated);
                    let response = match exit_code {
                        Some(code) => {
                            response.exit_status(TerminalExitStatus::new().exit_code(code))
                        }
                        None => response,
                    };
                    responder.respond(response)
                }
                Err(e) => responder
                    .respond_with_error(agent_client_protocol::Error::new(-32602, e.to_string())),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WaitForTerminalExitRequest, responder, _cx| match term_wait
                .wait_exit(&request.terminal_id.0)
                .await
            {
                Ok(code) => responder.respond(WaitForTerminalExitResponse::new(
                    TerminalExitStatus::new().exit_code(code.unwrap_or(0)),
                )),
                Err(e) => responder
                    .respond_with_error(agent_client_protocol::Error::new(-32602, e.to_string())),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: KillTerminalRequest, responder, _cx| {
                let _ = term_kill.kill(&request.terminal_id.0).await;
                responder.respond(agent_client_protocol::v1::schema::KillTerminalResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReleaseTerminalRequest, responder, _cx| {
                let _ = term_release.release(&request.terminal_id.0).await;
                responder.respond(agent_client_protocol::v1::schema::ReleaseTerminalResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
            let init = cx
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(
                            ClientCapabilities::new()
                                .fs(FileSystemCapabilities::new()
                                    .read_text_file(true)
                                    .write_text_file(true))
                                .terminal(true),
                        )
                        .client_info(Implementation::new(
                            "agent-control",
                            env!("CARGO_PKG_VERSION"),
                        )),
                )
                .block_task()
                .await?;

            let supports_system_prompt = init
                .agent_capabilities
                .session_capabilities
                .system_prompt
                .is_some();
            let agent_name = init
                .agent_info
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "unknown".into());
            let _ = main_events
                .send(HarnessEvent::Connected {
                    agent_name,
                    protocol_version: 1,
                    supports_system_prompt,
                })
                .await;

            let mut session_req = NewSessionRequest::new(main_project.clone());
            if let Some(endpoint) = &mcp_endpoint {
                let server = match endpoint {
                    McpEndpoint::Http { url } => {
                        McpServer::Http(McpServerHttp::new("agent-control", url.clone()))
                    }
                    McpEndpoint::Stdio { command, args } => McpServer::Stdio(
                        McpServerStdio::new("agent-control", command.clone()).args(args.clone()),
                    ),
                };
                session_req = session_req.mcp_servers(vec![server]);
            }
            if supports_system_prompt {
                session_req =
                    session_req.system_prompt(SystemPrompt::new(vec![ContentBlock::Text(
                        TextContent::new(main_briefing.clone()),
                    )]));
            }
            let new_session = cx.send_request(session_req).block_task().await?;
            let acp_session_id = new_session.session_id.clone();
            let _ = main_events
                .send(HarnessEvent::SessionCreated {
                    control_session_id,
                    acp_session_id: acp_session_id.0.to_string(),
                    snapshot_hash,
                    rule_count,
                })
                .await;

            let mut first_prompt = true;
            while let Some(command) = command_rx.recv().await {
                match command {
                    HarnessCommand::Prompt { text } => {
                        let prompt_text = if first_prompt && !supports_system_prompt {
                            format!("{main_briefing}\n\n---\n\n{text}")
                        } else {
                            text
                        };
                        first_prompt = false;
                        let cx2 = cx.clone();
                        let sid = acp_session_id.clone();
                        let events = main_events.clone();
                        tokio::spawn(async move {
                            let result = cx2
                                .send_request(PromptRequest::new(
                                    sid,
                                    vec![ContentBlock::Text(TextContent::new(prompt_text))],
                                ))
                                .block_task()
                                .await;
                            match result {
                                Ok(response) => {
                                    let _ = events
                                        .send(HarnessEvent::PromptComplete {
                                            stop_reason: format!("{:?}", response.stop_reason),
                                        })
                                        .await;
                                }
                                Err(e) => {
                                    let _ = events
                                        .send(HarnessEvent::Error { message: e.message })
                                        .await;
                                }
                            }
                        });
                    }
                    HarnessCommand::Cancel => {
                        let _ =
                            cx.send_notification(CancelNotification::new(acp_session_id.clone()));
                    }
                    HarnessCommand::Shutdown => break,
                }
            }
            Ok(())
        })
        .await;

    let _ = event_tx.send(HarnessEvent::Disconnected).await;
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = event_tx
                .send(HarnessEvent::Error {
                    message: e.message.clone(),
                })
                .await;
            Err(anyhow::anyhow!("ACP connection failed: {e}"))
        }
    }
}

async fn handle_session_update(
    notification: &SessionNotification,
    store: &Store,
    session_id: Uuid,
    agent_id: &str,
    events: &mpsc::Sender<HarnessEvent>,
) {
    match &notification.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(text) = &chunk.content {
                let _ = events
                    .send(HarnessEvent::AgentMessage {
                        text: text.text.clone(),
                    })
                    .await;
            }
        }
        SessionUpdate::ToolCall(tool_call) => {
            let _ = events
                .send(HarnessEvent::ToolCall {
                    tool_call_id: tool_call.tool_call_id.0.to_string(),
                    title: tool_call.title.clone(),
                    kind: format!("{:?}", tool_call.kind),
                    status: format!("{:?}", tool_call.status),
                })
                .await;
            if let Ok(entry) = store.record_evidence(
                session_id,
                EvidenceKind::ToolCall,
                agent_id,
                serde_json::json!({
                    "tool_call_id": tool_call.tool_call_id.0.to_string(),
                    "title": tool_call.title,
                    "kind": format!("{:?}", tool_call.kind),
                    "status": format!("{:?}", tool_call.status),
                    "raw_input": tool_call.raw_input,
                }),
            ) {
                let _ = events
                    .send(HarnessEvent::EvidenceRecorded {
                        seq: entry.seq,
                        kind: "tool_call".into(),
                    })
                    .await;
            }
        }
        SessionUpdate::ToolCallUpdate(update) => {
            if let Ok(entry) = store.record_evidence(
                session_id,
                EvidenceKind::ToolCall,
                agent_id,
                serde_json::json!({
                    "tool_call_id": update.tool_call_id.0.to_string(),
                    "update": true,
                    "status": update.fields.status.map(|s| format!("{s:?}")),
                    "raw_output": update.fields.raw_output,
                }),
            ) {
                let _ = events
                    .send(HarnessEvent::EvidenceRecorded {
                        seq: entry.seq,
                        kind: "tool_call_update".into(),
                    })
                    .await;
            }
        }
        SessionUpdate::Plan(plan) => {
            let entries: Vec<String> = plan.entries.iter().map(|e| format!("{:?}", e)).collect();
            let _ = events.send(HarnessEvent::Plan { entries }).await;
        }
        _ => {}
    }
}
