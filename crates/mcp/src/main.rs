#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::Arc;

use agent_control_core::Store;
use agent_control_mcp::ControlServer;
use anyhow::Context;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser)]
#[command(
    name = "agent-control-mcp",
    about = "MCP server for the agent-control engineering protocol console"
)]
struct Cli {
    /// Run in stdio mode (for agent harnesses that spawn this process)
    #[arg(long)]
    stdio: bool,

    /// Bind address for Streamable HTTP mode
    #[arg(long, default_value = "127.0.0.1:9870")]
    bind: String,

    /// Data directory (defaults to ~/.agent-control)
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Launch a detached background daemon (no console, survives the parent) and exit.
    /// Ignored in stdio mode. Single-instance: a second launch on a busy address exits cleanly.
    #[arg(long)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.daemon && !cli.stdio {
        spawn_detached()?;
        return Ok(());
    }
    let store = match cli.data_dir {
        Some(dir) => Arc::new(Store::open_with_root(dir)?),
        None => Arc::new(Store::open_default()?),
    };
    let _log_guard = init_logging(&store.layout, "mcp.log");

    if cli.stdio {
        tracing::info!("starting MCP server in stdio mode");
        let server = ControlServer::new(store).serve(stdio()).await?;
        server.waiting().await?;
    } else {
        let shutdown = tokio_util::sync::CancellationToken::new();
        let signal_shutdown = shutdown.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                signal_shutdown.cancel();
            }
        });
        if let Err(e) = agent_control_mcp::serve_http(store, &cli.bind, shutdown).await {
            // Single-instance guard: the TCP bind is the lock. If another daemon already owns the
            // address, exit cleanly instead of failing (idempotent launches).
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse)
            {
                tracing::info!(bind = %cli.bind, "MCP daemon already serving; exiting");
                return Ok(());
            }
            return Err(e);
        }
    }
    Ok(())
}

/// Re-launch this executable detached (no console, survives the parent) and exit. Backs `--daemon`
/// so the MCP server keeps running after the launching shell or console closes.
fn spawn_detached() -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("resolving current executable")?;
    let args: Vec<std::ffi::OsString> = std::env::args_os()
        .skip(1)
        .filter(|arg| arg != "--daemon")
        .collect();
    let mut cmd = std::process::Command::new(exe);
    cmd.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    cmd.spawn().context("spawning detached MCP daemon")?;
    Ok(())
}

/// Log to stderr (dev) and to `<data root>/logs/<name>` (visible even with no console).
/// The returned guard must outlive the process or the file writer thread stops.
fn init_logging(
    layout: &agent_control_core::DataLayout,
    name: &str,
) -> tracing_appender::non_blocking::WorkerGuard {
    let appender = tracing_appender::rolling::never(layout.logs_dir(), name);
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false),
        )
        .init();
    guard
}
