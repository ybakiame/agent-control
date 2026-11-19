//! Read-only aggregated diagnostics: hooks registration, export drift, evidence-chain integrity,
//! rule conflicts and daemon reachability in one report. Performs no writes.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::engine::RuleEngine;
use crate::export::{DriftReport, ExportTarget};
use crate::hooks::HookStatus;
use crate::model::QueryContext;
use crate::store::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionChainStatus {
    pub session_id: Uuid,
    pub task: String,
    pub valid: bool,
    pub entries_checked: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub broken_at: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonHealth {
    pub bind: String,
    pub reachable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub project_path: String,
    pub generated_at: String,
    /// True when no evidence chain is broken, no export target is stale and no rule conflicts.
    pub healthy: bool,
    pub rules_applied: usize,
    pub conflicts: usize,
    pub hooks_registered: usize,
    pub hooks_installed: usize,
    pub harnesses: Vec<HookStatus>,
    pub stale_exports: usize,
    pub drift: DriftReport,
    pub sessions_checked: usize,
    pub broken_chains: usize,
    pub chains: Vec<SessionChainStatus>,
    pub daemon: DaemonHealth,
}

impl Store {
    pub fn doctor(&self, project_path: &Path, daemon_bind: Option<&str>) -> Result<DoctorReport> {
        let project = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());
        let rules = self.load_rules(Some(&project))?;
        let ctx = QueryContext {
            project_path: project.clone(),
            agent_id: "doctor".into(),
            file_paths: Vec::new(),
            languages: Vec::new(),
            task_tags: Vec::new(),
            session_id: None,
        };
        let resolution = RuleEngine::resolve(&rules, &ctx);
        let harnesses = self.detect_hooks(&project)?;
        let drift = self.export_drift(&project, &ExportTarget::ALL, false)?;
        let mut chains = Vec::new();
        for session in self.list_sessions()? {
            let report = self.verify_chain(session.id)?;
            chains.push(SessionChainStatus {
                session_id: session.id,
                task: session.task,
                valid: report.valid,
                entries_checked: report.entries_checked,
                broken_at: report.broken_at,
            });
        }
        let bind = daemon_bind
            .map(str::to_owned)
            .unwrap_or_else(|| format!("127.0.0.1:{}", AppConfig::load(&self.layout).mcp_port));
        let daemon = DaemonHealth {
            reachable: probe_daemon(&bind),
            bind,
        };
        let hooks_registered = harnesses.iter().filter(|h| h.registered).count();
        let hooks_installed = harnesses.iter().filter(|h| h.installed).count();
        let stale_exports = drift.targets.iter().filter(|t| t.stale).count();
        let broken_chains = chains.iter().filter(|c| !c.valid).count();
        let conflicts = resolution.conflicts.len();
        Ok(DoctorReport {
            project_path: project.display().to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            healthy: broken_chains == 0 && stale_exports == 0 && conflicts == 0,
            rules_applied: resolution.applied.len(),
            conflicts,
            hooks_registered,
            hooks_installed,
            harnesses,
            stale_exports,
            drift,
            sessions_checked: chains.len(),
            broken_chains,
            chains,
            daemon,
        })
    }
}

/// Loopback TCP probe with a short timeout; a bind that cannot be parsed reads as unreachable.
pub fn probe_daemon(bind: &str) -> bool {
    bind.to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .map(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok())
        .unwrap_or(false)
}
