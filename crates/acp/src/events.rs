use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum HarnessCommand {
    Prompt { text: String },
    Cancel,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    #[default]
    Ask,
    AutoApprove,
    AutoReject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionOptionInfo {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug)]
pub enum HarnessEvent {
    Connected {
        agent_name: String,
        protocol_version: u16,
        supports_system_prompt: bool,
    },
    SessionCreated {
        control_session_id: Uuid,
        acp_session_id: String,
        snapshot_hash: String,
        rule_count: usize,
    },
    AgentMessage {
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        title: String,
        kind: String,
        status: String,
    },
    Plan {
        entries: Vec<String>,
    },
    PermissionRequest {
        acp_session_id: String,
        tool_title: String,
        options: Vec<PermissionOptionInfo>,
        responder: oneshot::Sender<PermissionDecision>,
    },
    PromptComplete {
        stop_reason: String,
    },
    EvidenceRecorded {
        seq: u64,
        kind: String,
    },
    Error {
        message: String,
    },
    Disconnected,
}

impl HarnessEvent {
    pub fn summary(&self) -> String {
        match self {
            HarnessEvent::Connected { agent_name, .. } => format!("connected to {agent_name}"),
            HarnessEvent::SessionCreated { rule_count, .. } => {
                format!("session created with {rule_count} binding rules")
            }
            HarnessEvent::AgentMessage { text } => {
                let short: String = text.chars().take(80).collect();
                format!("agent: {short}")
            }
            HarnessEvent::ToolCall { title, status, .. } => format!("tool: {title} [{status}]"),
            HarnessEvent::Plan { entries } => format!("plan: {} entries", entries.len()),
            HarnessEvent::PermissionRequest { tool_title, .. } => {
                format!("permission requested: {tool_title}")
            }
            HarnessEvent::PromptComplete { stop_reason } => {
                format!("prompt complete: {stop_reason}")
            }
            HarnessEvent::EvidenceRecorded { seq, kind } => format!("evidence #{seq}: {kind}"),
            HarnessEvent::Error { message } => format!("error: {message}"),
            HarnessEvent::Disconnected => "disconnected".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpEndpoint {
    Http { url: String },
    Stdio { command: String, args: Vec<String> },
}
