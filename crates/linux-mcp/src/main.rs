//! linux-mcp — native MCP server for Linux.
//!
//! Stdio JSON-RPC. Detects X11 vs Wayland at startup. Tools are typed,
//! allow-listed, and audit-logged. No `run_shell` or `eval_*` escape hatches.

use anyhow::Result;
use linux_mcp_core::AuditLog;
use rmcp::{transport::stdio, ServiceExt};
use std::sync::Arc;
use tracing::info;

mod display;
mod error;
mod schema;
mod server;
mod tools;
mod util;

use server::LinuxMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let log_level = std::env::var("LINUX_MCP_LOG_LEVEL").unwrap_or_else(|_| "info".into());
    let audit = Arc::new(AuditLog::new(&log_level));
    audit.info(
        "linux-mcp starting",
        serde_json::json!({
            "pid": std::process::id(),
            "version": linux_mcp_core::VERSION,
        }),
    );

    let backend = display::DisplayBackend::detect();
    info!(?backend, "display backend detected");
    audit.info(
        "display backend",
        serde_json::json!({ "kind": format!("{:?}", backend) }),
    );

    let server = LinuxMcpServer::new(audit.clone(), backend);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    audit.info("linux-mcp exiting", serde_json::json!({}));
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    // Logs go to stderr so stdout stays clean for the JSON-RPC transport.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("LINUX_MCP_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .try_init();
}
