//! Display-server detection and capability dispatch.
//!
//! Strategy: detect by env var presence at startup, then route window/input/
//! clipboard/screenshot tools to the appropriate backend (or refuse with a
//! clear error if the backend is missing required helpers).

use std::process::Command;
use which::which;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayBackend {
    Wayland,
    X11,
    None,
}

impl DisplayBackend {
    pub fn detect() -> Self {
        let wayland = std::env::var("WAYLAND_DISPLAY").map(|s| !s.is_empty()).unwrap_or(false);
        let x11 = std::env::var("DISPLAY").map(|s| !s.is_empty()).unwrap_or(false);
        match (wayland, x11) {
            (true, _) => Self::Wayland,
            (false, true) => Self::X11,
            _ => Self::None,
        }
    }

    pub fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Helper-binary inventory. We prefer pure-Rust where reasonable, but for
/// X11/Wayland tools the well-maintained CLIs (xdotool, wmctrl, wl-clipboard,
/// grim, slurp, ydotool/wtype, scrot, xclip, notify-send, zenity) are the
/// pragmatic answer for a v0.1.0 — implementing native X11/Wayland clients in
/// Rust would multiply the line count.
pub struct Helpers {
    pub backend: DisplayBackend,
}

impl Helpers {
    pub fn new(backend: DisplayBackend) -> Self {
        Self { backend }
    }

    pub fn require(&self, name: &str) -> Result<std::path::PathBuf, crate::error::ToolError> {
        which(name).map_err(|_| {
            crate::error::ToolError::coded(
                "helper_missing",
                format!("required helper '{name}' not on PATH; install via your package manager"),
            )
        })
    }

    pub fn require_any(&self, names: &[&str]) -> Result<(std::path::PathBuf, &'static str), crate::error::ToolError> {
        for n in names {
            if let Ok(p) = which(n) {
                let leaked: &'static str = Box::leak(n.to_string().into_boxed_str());
                return Ok((p, leaked));
            }
        }
        Err(crate::error::ToolError::coded(
            "helper_missing",
            format!("none of {:?} found on PATH; install one of them", names),
        ))
    }

    /// Run a helper synchronously and return (exit_code, stdout, stderr).
    pub fn run(&self, cmd: &mut Command) -> std::io::Result<(i32, Vec<u8>, Vec<u8>)> {
        let out = cmd.output()?;
        Ok((
            out.status.code().unwrap_or(-1),
            out.stdout,
            out.stderr,
        ))
    }
}
