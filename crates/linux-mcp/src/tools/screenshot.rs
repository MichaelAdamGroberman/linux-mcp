//! screenshot_screen / screenshot_window — grim (Wayland), scrot/maim (X11).

use crate::display::{DisplayBackend, Helpers};
use crate::error::{ok_json, ToolError};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rmcp::model::CallToolResult;
use std::process::Command;

pub async fn screenshot_screen(helpers: &Helpers) -> Result<CallToolResult, ToolError> {
    let tmp = tempfile_path("png");
    match helpers.backend {
        DisplayBackend::Wayland => {
            let bin = helpers.require("grim")?;
            let status = Command::new(&bin)
                .arg(&tmp)
                .status()
                .map_err(|e| ToolError::coded("grim_failed", e.to_string()))?;
            if !status.success() {
                return Err(ToolError::coded("grim_failed", format!("exit {:?}", status.code())));
            }
        }
        DisplayBackend::X11 => {
            let (bin, _) = helpers.require_any(&["maim", "scrot", "import"])?;
            let bin_name = bin.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let mut cmd = Command::new(&bin);
            if bin_name == "import" {
                cmd.args(["-window", "root"]).arg(&tmp);
            } else {
                cmd.arg(&tmp);
            }
            let status = cmd.status()
                .map_err(|e| ToolError::coded("screenshot_failed", e.to_string()))?;
            if !status.success() {
                return Err(ToolError::coded("screenshot_failed", format!("exit {:?}", status.code())));
            }
        }
        DisplayBackend::None => {
            return Err(ToolError::coded("no_display", "no graphical session"));
        }
    }
    let bytes = std::fs::read(&tmp).map_err(|e| ToolError::coded("read_failed", e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(ok_json(serde_json::json!({
        "image_png_b64": B64.encode(&bytes),
        "size": bytes.len(),
    })))
}

pub async fn screenshot_window(
    helpers: &Helpers,
    window_id: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    // Wayland: no portable per-window capture; fall back to screen on most compositors.
    // X11: import or maim with --window.
    let tmp = tempfile_path("png");
    match helpers.backend {
        DisplayBackend::X11 => {
            let id = window_id.ok_or_else(|| ToolError::coded("missing_arg", "window_id required (X11 hex/decimal id from list_windows)"))?;
            let (bin, _) = helpers.require_any(&["maim", "import"])?;
            let bin_name = bin.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let mut cmd = Command::new(&bin);
            if bin_name == "maim" {
                cmd.args(["-i", id]).arg(&tmp);
            } else {
                cmd.args(["-window", id]).arg(&tmp);
            }
            let status = cmd.status()
                .map_err(|e| ToolError::coded("screenshot_failed", e.to_string()))?;
            if !status.success() {
                return Err(ToolError::coded("screenshot_failed", format!("exit {:?}", status.code())));
            }
        }
        DisplayBackend::Wayland => {
            return Err(ToolError::coded(
                "wayland_window_capture_unsupported",
                "Wayland has no portable per-window capture — use screenshot_screen + crop, or call your compositor's specific tool (e.g. swaymsg + grim with -g)",
            ));
        }
        DisplayBackend::None => {
            return Err(ToolError::coded("no_display", "no graphical session"));
        }
    }
    let bytes = std::fs::read(&tmp).map_err(|e| ToolError::coded("read_failed", e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(ok_json(serde_json::json!({
        "image_png_b64": B64.encode(&bytes),
        "size": bytes.len(),
    })))
}

fn tempfile_path(ext: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("linux-mcp-{pid}-{nanos}.{ext}"))
}
