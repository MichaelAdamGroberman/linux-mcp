//! notify — libnotify via notify-send.

use crate::display::Helpers;
use crate::error::{ok_json, ToolError};
use rmcp::model::CallToolResult;
use std::process::Command;

pub async fn notify(
    helpers: &Helpers,
    title: &str,
    body: &str,
    urgency: Option<&str>,
    icon: Option<&str>,
    timeout_ms: Option<i64>,
) -> Result<CallToolResult, ToolError> {
    let bin = helpers.require("notify-send")?;
    let mut cmd = Command::new(&bin);
    cmd.arg(title).arg(body);
    if let Some(u) = urgency {
        cmd.args(["-u", u]);
    }
    if let Some(i) = icon {
        cmd.args(["-i", i]);
    }
    if let Some(t) = timeout_ms {
        cmd.args(["-t", &t.to_string()]);
    }
    let status = cmd
        .status()
        .map_err(|e| ToolError::coded("notify_failed", e.to_string()))?;
    Ok(ok_json(serde_json::json!({
        "delivered": status.success(),
        "exit_code": status.code().unwrap_or(-1),
    })))
}
