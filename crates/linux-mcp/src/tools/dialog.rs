//! prompt_user — zenity (GTK) or kdialog (KDE) for native input dialog.

use crate::display::Helpers;
use crate::error::{ok_json, ToolError};
use rmcp::model::CallToolResult;
use std::process::Command;

pub async fn prompt_user(
    helpers: &Helpers,
    title: &str,
    message: &str,
    default_value: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    let (bin, name) = helpers.require_any(&["zenity", "kdialog"])?;
    let out = if name == "zenity" {
        let mut cmd = Command::new(&bin);
        cmd.args(["--entry", "--title", title, "--text", message]);
        if let Some(d) = default_value {
            cmd.args(["--entry-text", d]);
        }
        cmd.output()
    } else {
        let mut cmd = Command::new(&bin);
        cmd.args(["--title", title, "--inputbox", message]);
        if let Some(d) = default_value {
            cmd.arg(d);
        }
        cmd.output()
    }
    .map_err(|e| ToolError::coded("dialog_failed", e.to_string()))?;
    let confirmed = out.status.success();
    let value = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
    Ok(ok_json(serde_json::json!({
        "confirmed": confirmed,
        "value": value,
        "helper": name,
    })))
}
