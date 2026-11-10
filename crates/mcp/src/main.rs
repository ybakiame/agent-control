use std::path::PathBuf;
use std::sync::Arc;

use agent_control_core::Store;
use agent_control_mcp::ControlServer;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let store = match cli.data_dir {
        Some(dir) => Arc::new(Store::open_with_root(dir)?),
        None => Arc::new(Store::open_default()?),
    };

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
        agent_control_mcp::serve_http(store, &cli.bind, shutdown).await?;
    }
    Ok(())
}
