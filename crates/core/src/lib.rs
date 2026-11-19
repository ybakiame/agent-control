pub mod config;
pub mod conflict;
pub mod doctor;
pub mod engine;
pub mod export;
pub mod gate;
pub mod hooks;
pub mod model;
pub mod store;

pub use config::{AppConfig, DEFAULT_MCP_PORT, DataLayout};
pub use doctor::{DaemonHealth, DoctorReport, SessionChainStatus};
pub use engine::RuleEngine;
pub use export::{
    DriftReport, ExportReport, ExportTarget, RevokeReport, TargetDrift, TargetOutcome,
    handoff_tier_warning, render_constraint_briefing,
};
pub use hooks::{HookReport, HookScope, HookStatus, HookTarget};
pub use model::*;
pub use store::{AcknowledgmentReport, ConstraintVerificationReport, RuleExplanation, Store};
