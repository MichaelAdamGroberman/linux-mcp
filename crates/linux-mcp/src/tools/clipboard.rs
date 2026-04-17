//! clipboard_read / clipboard_write — wl-clipboard on Wayland, xclip/xsel on X11.

use crate::display::{DisplayBackend, Helpers};
use crate::error::{ok_json, ToolError};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rmcp::model::CallToolResult;
use std::process::{Command, Stdio};

pub async fn clipboard_read(helpers: &Helpers) -> Result<CallToolResult, ToolError> {
    match helpers.backend {
        DisplayBackend::Wayland => {
            let bin = helpers.require("wl-paste")?;
            let out = Command::new(&bin).output()
                .map_err(|e| ToolError::coded("wl_paste_failed", e.to_string()))?;
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            Ok(ok_json(serde_json::json!({ "text": text })))
        }
        DisplayBackend::X11 => {
            let (bin, name) = helpers.require_any(&["xclip", "xsel"])?;
            let out = if name == "xclip" {
                Command::new(&bin).args(["-selection", "clipboard", "-o"]).output()
            } else {
                Command::new(&bin).args(["--clipboard", "--output"]).output()
            }
            .map_err(|e| ToolError::coded("clipboard_read_failed", e.to_string()))?;
            Ok(ok_json(serde_json::json!({
                "text": String::from_utf8_lossy(&out.stdout).to_string(),
                "helper": name,
            })))
        }
        DisplayBackend::None => Err(ToolError::coded(
            "no_display",
            "no $DISPLAY or $WAYLAND_DISPLAY — clipboard requires a graphical session",
        )),
    }
}

pub async fn clipboard_write(
    helpers: &Helpers,
    text: Option<&str>,
    image_b64: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    let payload: Vec<u8> = if let Some(t) = text {
        t.as_bytes().to_vec()
    } else if let Some(b) = image_b64 {
        B64.decode(b).map_err(|e| ToolError::coded("bad_base64", e.to_string()))?
    } else {
        return Err(ToolError::coded("missing_arg", "provide text or image_png_b64"));
    };
    let mime = if image_b64.is_some() { "image/png" } else { "text/plain" };

    match helpers.backend {
        DisplayBackend::Wayland => {
            let bin = helpers.require("wl-copy")?;
            let mut child = Command::new(&bin)
                .args(["-t", mime])
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| ToolError::coded("wl_copy_failed", e.to_string()))?;
            child.stdin.as_mut().unwrap().write_all(&payload)?;
            child.wait()?;
        }
        DisplayBackend::X11 => {
            let (bin, name) = helpers.require_any(&["xclip", "xsel"])?;
            let mut cmd = Command::new(&bin);
            if name == "xclip" {
                cmd.args(["-selection", "clipboard", "-t", mime]);
            } else {
                cmd.args(["--clipboard", "--input"]);
            }
            cmd.stdin(Stdio::piped());
            let mut child = cmd.spawn()
                .map_err(|e| ToolError::coded("clipboard_write_failed", e.to_string()))?;
            child.stdin.as_mut().unwrap().write_all(&payload)?;
            child.wait()?;
        }
        DisplayBackend::None => {
            return Err(ToolError::coded("no_display", "clipboard requires a graphical session"));
        }
    }
    Ok(ok_json(serde_json::json!({
        "wrote_bytes": payload.len(),
        "mime": mime,
    })))
}

use std::io::Write as _;
