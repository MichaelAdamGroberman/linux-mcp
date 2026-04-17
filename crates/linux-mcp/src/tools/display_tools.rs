//! Window/app management. wmctrl + xdotool (X11). Wayland: best-effort via
//! compositor IPC (sway/hyprland have CLIs); falls back to "unsupported" cleanly.

use crate::display::{DisplayBackend, Helpers};
use crate::error::{ok_json, ToolError};
use rmcp::model::CallToolResult;
use std::process::Command;

pub async fn list_windows(helpers: &Helpers) -> Result<CallToolResult, ToolError> {
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("wmctrl")?;
            let out = Command::new(&bin).args(["-lpG"]).output()
                .map_err(|e| ToolError::coded("wmctrl_failed", e.to_string()))?;
            let text = String::from_utf8_lossy(&out.stdout);
            let mut windows = Vec::new();
            for line in text.lines() {
                // wmctrl -lpG: <id> <desktop> <pid> <x> <y> <w> <h> <hostname> <title>
                let parts: Vec<&str> = line.splitn(9, char::is_whitespace).filter(|s| !s.is_empty()).collect();
                if parts.len() < 9 { continue; }
                windows.push(serde_json::json!({
                    "id": parts[0],
                    "desktop": parts[1].parse::<i32>().unwrap_or(-1),
                    "pid": parts[2].parse::<i32>().unwrap_or(-1),
                    "x": parts[3].parse::<i32>().unwrap_or(0),
                    "y": parts[4].parse::<i32>().unwrap_or(0),
                    "width": parts[5].parse::<i32>().unwrap_or(0),
                    "height": parts[6].parse::<i32>().unwrap_or(0),
                    "host": parts[7],
                    "title": parts[8].trim(),
                }));
            }
            Ok(ok_json(serde_json::json!({ "count": windows.len(), "windows": windows })))
        }
        DisplayBackend::Wayland => {
            // Try sway first, then hyprctl
            if let Ok(bin) = which::which("swaymsg") {
                let out = Command::new(&bin).args(["-t", "get_tree"]).output()
                    .map_err(|e| ToolError::coded("swaymsg_failed", e.to_string()))?;
                return Ok(ok_json(serde_json::json!({
                    "compositor": "sway",
                    "raw_tree": String::from_utf8_lossy(&out.stdout).to_string()
                })));
            }
            if let Ok(bin) = which::which("hyprctl") {
                let out = Command::new(&bin).args(["clients", "-j"]).output()
                    .map_err(|e| ToolError::coded("hyprctl_failed", e.to_string()))?;
                return Ok(ok_json(serde_json::json!({
                    "compositor": "hyprland",
                    "clients_json": String::from_utf8_lossy(&out.stdout).to_string()
                })));
            }
            Err(ToolError::coded(
                "wayland_no_compositor_ipc",
                "no swaymsg/hyprctl found — listing windows on Wayland needs compositor-specific IPC",
            ))
        }
        DisplayBackend::None => Err(ToolError::coded("no_display", "no graphical session")),
    }
}

pub async fn focus_window(helpers: &Helpers, window_id: &str) -> Result<CallToolResult, ToolError> {
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("wmctrl")?;
            let status = Command::new(&bin).args(["-i", "-a", window_id]).status()
                .map_err(|e| ToolError::coded("wmctrl_failed", e.to_string()))?;
            if !status.success() {
                return Err(ToolError::coded("focus_failed", format!("exit {:?}", status.code())));
            }
            Ok(ok_json(serde_json::json!({ "focused": window_id })))
        }
        DisplayBackend::Wayland => {
            if let Ok(bin) = which::which("swaymsg") {
                let cmd = format!("[con_id={window_id}] focus");
                let status = Command::new(&bin).arg(&cmd).status()
                    .map_err(|e| ToolError::coded("swaymsg_failed", e.to_string()))?;
                if !status.success() {
                    return Err(ToolError::coded("focus_failed", format!("exit {:?}", status.code())));
                }
                return Ok(ok_json(serde_json::json!({ "focused": window_id, "compositor": "sway" })));
            }
            Err(ToolError::coded("wayland_no_compositor_ipc", "no compositor IPC available"))
        }
        DisplayBackend::None => Err(ToolError::coded("no_display", "no graphical session")),
    }
}

pub async fn move_window(
    helpers: &Helpers,
    window_id: &str,
    x: i64,
    y: i64,
) -> Result<CallToolResult, ToolError> {
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("wmctrl")?;
            let geom = format!("0,{x},{y},-1,-1");
            let status = Command::new(&bin).args(["-i", "-r", window_id, "-e", &geom]).status()
                .map_err(|e| ToolError::coded("wmctrl_failed", e.to_string()))?;
            if !status.success() {
                return Err(ToolError::coded("move_failed", format!("exit {:?}", status.code())));
            }
            Ok(ok_json(serde_json::json!({ "moved": true, "x": x, "y": y })))
        }
        _ => Err(ToolError::coded(
            "unsupported",
            "move_window currently requires X11 + wmctrl",
        )),
    }
}

pub async fn resize_window(
    helpers: &Helpers,
    window_id: &str,
    width: i64,
    height: i64,
) -> Result<CallToolResult, ToolError> {
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("wmctrl")?;
            let geom = format!("0,-1,-1,{width},{height}");
            let status = Command::new(&bin).args(["-i", "-r", window_id, "-e", &geom]).status()
                .map_err(|e| ToolError::coded("wmctrl_failed", e.to_string()))?;
            if !status.success() {
                return Err(ToolError::coded("resize_failed", format!("exit {:?}", status.code())));
            }
            Ok(ok_json(serde_json::json!({ "resized": true, "width": width, "height": height })))
        }
        _ => Err(ToolError::coded(
            "unsupported",
            "resize_window currently requires X11 + wmctrl",
        )),
    }
}
