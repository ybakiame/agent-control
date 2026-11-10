pub mod connection;
pub mod events;
pub mod fs;
pub mod terminal;

use std::sync::Arc;

use agent_control_core::Store;
use anyhow::Result;
use tokio::sync::mpsc;
use uuid::Uuid;

pub use connection::{HarnessConfig, constraint_briefing, run_harness};
pub use events::{
    HarnessCommand, HarnessEvent, McpEndpoint, PermissionDecision, PermissionOptionInfo,
    PermissionPolicy,
};

pub struct HarnessChannels {
    pub control_session_id_hint: Option<Uuid>,
    pub command_tx: mpsc::Sender<HarnessCommand>,
    pub event_rx: mpsc::Receiver<HarnessEvent>,
}

pub struct HarnessManager {
    runtime: Arc<tokio::runtime::Runtime>,
    store: Arc<Store>,
}

impl HarnessManager {
    pub fn new(store: Arc<Store>) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("agent-control-tokio")
            .build()?;
        Ok(Self {
            runtime: Arc::new(runtime),
            store,
        })
    }

    pub fn runtime(&self) -> &Arc<tokio::runtime::Runtime> {
        &self.runtime
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn start(&self, config: HarnessConfig) -> Result<HarnessChannels> {
        let (command_tx, command_rx) = mpsc::channel(64);
        let (event_tx, event_rx) = mpsc::channel(512);
        let store = self.store.clone();
        let fail_tx = event_tx.clone();
        self.runtime.spawn(async move {
            if let Err(e) = run_harness(config, store, command_rx, event_tx).await {
                let _ = fail_tx
                    .send(HarnessEvent::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
        });
        Ok(HarnessChannels {
            control_session_id_hint: None,
            command_tx,
            event_rx,
        })
    }

    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }
}
