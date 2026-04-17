//! mouse + keyboard input. xdotool (X11), ydotool/wtype (Wayland).
//!
//! Wayland note: ydotool requires a privileged ydotoold daemon (uinput access).
//! wtype works without root for keyboard but not mouse on most compositors.

use crate::display::{DisplayBackend, Helpers};
use crate::error::{ok_json, ToolError};
use rmcp::model::CallToolResult;
use std::process::Command;

pub async fn mouse_move(helpers: &Helpers, x: i64, y: i64) -> Result<CallToolResult, ToolError> {
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("xdotool")?;
            run_status(&bin, &["mousemove", &x.to_string(), &y.to_string()], "xdotool_failed")?;
            Ok(ok_json(serde_json::json!({ "moved": true, "x": x, "y": y })))
        }
        DisplayBackend::Wayland => {
            let bin = helpers.require("ydotool")?;
            run_status(&bin, &["mousemove", "--absolute", "--", &x.to_string(), &y.to_string()], "ydotool_failed")?;
            Ok(ok_json(serde_json::json!({ "moved": true, "x": x, "y": y })))
        }
        DisplayBackend::None => Err(ToolError::coded("no_display", "no graphical session")),
    }
}

pub async fn mouse_click(
    helpers: &Helpers,
    button: Option<&str>,
    count: Option<i64>,
) -> Result<CallToolResult, ToolError> {
    let button_num = match button.unwrap_or("left") {
        "right" => "3",
        "middle" => "2",
        _ => "1",
    };
    let count = count.unwrap_or(1).clamp(1, 3);
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("xdotool")?;
            for _ in 0..count {
                run_status(&bin, &["click", button_num], "xdotool_failed")?;
            }
        }
        DisplayBackend::Wayland => {
            let bin = helpers.require("ydotool")?;
            // ydotool button mapping: 0xC0 left, 0xC1 right, 0xC2 middle (down+up = 0x40 mask)
            let yd = match button.unwrap_or("left") {
                "right" => "0xC1",
                "middle" => "0xC2",
                _ => "0xC0",
            };
            for _ in 0..count {
                run_status(&bin, &["click", yd], "ydotool_failed")?;
            }
        }
        DisplayBackend::None => return Err(ToolError::coded("no_display", "no graphical session")),
    }
    Ok(ok_json(serde_json::json!({ "clicked": true, "count": count })))
}

pub async fn mouse_scroll(
    helpers: &Helpers,
    dy: i64,
    _dx: Option<i64>,
) -> Result<CallToolResult, ToolError> {
    let direction = if dy > 0 { "4" } else { "5" }; // X11 button 4 = up, 5 = down
    let ticks = dy.unsaturated_abs().clamp(1, 50);
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("xdotool")?;
            for _ in 0..ticks {
                run_status(&bin, &["click", direction], "xdotool_failed")?;
            }
        }
        DisplayBackend::Wayland => {
            let bin = helpers.require("ydotool")?;
            run_status(&bin, &["mousemove_relative", "0", &dy.to_string()], "ydotool_failed")?;
        }
        DisplayBackend::None => return Err(ToolError::coded("no_display", "no graphical session")),
    }
    Ok(ok_json(serde_json::json!({ "scrolled": true, "dy": dy })))
}

pub async fn key_press(
    helpers: &Helpers,
    key: &str,
    modifiers: Option<&[String]>,
) -> Result<CallToolResult, ToolError> {
    let mut chord = String::new();
    if let Some(mods) = modifiers {
        for m in mods {
            let xname = match m.as_str() {
                "cmd" | "super" => "super",
                "shift" => "shift",
                "option" | "alt" => "alt",
                "control" | "ctrl" => "ctrl",
                _ => continue,
            };
            chord.push_str(xname);
            chord.push('+');
        }
    }
    chord.push_str(key);
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("xdotool")?;
            run_status(&bin, &["key", &chord], "xdotool_failed")?;
        }
        DisplayBackend::Wayland => {
            let (bin, name) = helpers.require_any(&["wtype", "ydotool"])?;
            if name == "wtype" {
                run_status(&bin, &["-k", key], "wtype_failed")?;
            } else {
                run_status(&bin, &["key", key], "ydotool_failed")?;
            }
        }
        DisplayBackend::None => return Err(ToolError::coded("no_display", "no graphical session")),
    }
    Ok(ok_json(serde_json::json!({ "pressed": chord })))
}

pub async fn type_text(helpers: &Helpers, text: &str) -> Result<CallToolResult, ToolError> {
    if text.len() > 10_000 {
        return Err(ToolError::coded("text_too_long", "type_text capped at 10,000 chars"));
    }
    match helpers.backend {
        DisplayBackend::X11 => {
            let bin = helpers.require("xdotool")?;
            run_status(&bin, &["type", "--", text], "xdotool_failed")?;
        }
        DisplayBackend::Wayland => {
            let (bin, name) = helpers.require_any(&["wtype", "ydotool"])?;
            if name == "wtype" {
                run_status(&bin, &[text], "wtype_failed")?;
            } else {
                run_status(&bin, &["type", text], "ydotool_failed")?;
            }
        }
        DisplayBackend::None => return Err(ToolError::coded("no_display", "no graphical session")),
    }
    Ok(ok_json(serde_json::json!({ "typed": text.chars().count() })))
}

fn run_status(bin: &std::path::Path, args: &[&str], err_code: &'static str) -> Result<(), ToolError> {
    let status = Command::new(bin)
        .args(args)
        .status()
        .map_err(|e| ToolError::coded(err_code, e.to_string()))?;
    if !status.success() {
        return Err(ToolError::coded(err_code, format!("exit {:?}", status.code())));
    }
    Ok(())
}

trait UnsatAbs {
    fn unsaturated_abs(self) -> Self;
}
impl UnsatAbs for i64 {
    fn unsaturated_abs(self) -> Self {
        if self == i64::MIN { i64::MAX } else { self.abs() }
    }
}
