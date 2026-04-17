//! JSONL audit log. One line per tool invocation, rotated at 10 MB.
//! Default location: $XDG_STATE_HOME/linux-mcp/audit.log (falls back to
//! ~/.local/state/linux-mcp/audit.log).

use chrono::Utc;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_BYTES: u64 = 10 * 1024 * 1024;

pub struct AuditLog {
    inner: Mutex<Inner>,
}

struct Inner {
    handle: Option<File>,
    path: Option<PathBuf>,
    min_level: u8,
}

impl AuditLog {
    pub fn new(min_level: &str) -> Self {
        let path = log_path();
        if let Some(p) = path.as_ref() {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let handle = path
            .as_ref()
            .and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok());
        Self {
            inner: Mutex::new(Inner {
                handle,
                path,
                min_level: parse_level(min_level),
            }),
        }
    }

    pub fn debug(&self, msg: &str, meta: serde_json::Value) {
        self.write(0, msg, meta);
    }
    pub fn info(&self, msg: &str, meta: serde_json::Value) {
        self.write(1, msg, meta);
    }
    pub fn warn(&self, msg: &str, meta: serde_json::Value) {
        self.write(2, msg, meta);
    }
    pub fn error(&self, msg: &str, meta: serde_json::Value) {
        self.write(3, msg, meta);
    }

    fn write(&self, level: u8, msg: &str, meta: serde_json::Value) {
        let Ok(mut inner) = self.inner.lock() else { return };
        if level < inner.min_level {
            return;
        }
        let level_str = match level {
            0 => "debug",
            1 => "info",
            2 => "warn",
            _ => "error",
        };
        let record = json!({
            "ts": Utc::now().to_rfc3339(),
            "level": level_str,
            "msg": msg,
            "meta": meta,
        });
        let line = format!("{}\n", record);
        if let Some(handle) = inner.handle.as_mut() {
            let _ = handle.write_all(line.as_bytes());
            let _ = handle.flush();
        }
        Self::rotate_if_needed_locked(&mut inner);
    }

    fn rotate_if_needed_locked(inner: &mut Inner) {
        let Some(path) = inner.path.clone() else { return };
        let Ok(meta) = std::fs::metadata(&path) else { return };
        if meta.len() < MAX_BYTES {
            return;
        }
        let rotated = path.with_extension(format!("{}.log", Utc::now().timestamp()));
        if std::fs::rename(&path, &rotated).is_ok() {
            if let Ok(new_handle) = OpenOptions::new().create(true).append(true).open(&path) {
                let _ = new_handle.metadata();
                inner.handle = Some(new_handle);
                if let Some(h) = inner.handle.as_mut() {
                    let _ = h.seek(SeekFrom::End(0));
                }
            }
        }
    }
}

fn log_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("linux-mcp").join("audit.log"))
}

fn parse_level(s: &str) -> u8 {
    match s.to_ascii_lowercase().as_str() {
        "debug" => 0,
        "warn" => 2,
        "error" => 3,
        _ => 1,
    }
}
