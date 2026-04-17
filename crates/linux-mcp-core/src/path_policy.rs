//! Filesystem allow-list / deny-list policy.
//!
//! Resolves every requested path through canonicalisation (with fallback for
//! write-to-nonexistent-leaf) before comparing to allow + deny roots. Symlinks
//! are followed so that an allow-listed root cannot be subverted by a symlink
//! into `/etc` — the explicit defense against the bypass that
//! [Desktop Commander's FAQ](https://github.com/wonderwhy-er/DesktopCommanderMCP/blob/main/FAQ.md)
//! admits its allow-list does *not* prevent.

use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    Read,
    Write,
}

#[derive(Debug, Error)]
pub enum PathPolicyError {
    #[error("fs_denied: {0}")]
    Denied(String),
    #[error("fs_not_allowed: {0}")]
    NotAllowed(String),
    #[error("fs_invalid_path: {0}")]
    Invalid(String),
}

impl PathPolicyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Denied(_) => "fs_denied",
            Self::NotAllowed(_) => "fs_not_allowed",
            Self::Invalid(_) => "fs_invalid_path",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PathPolicy {
    allow: Vec<PathBuf>,
    deny: Vec<PathBuf>,
}

/// Default deny roots for Linux. Block the kernel/init/system surfaces; allow
/// `/tmp` since users frequently want scratch there.
pub const DEFAULT_DENY: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/boot",
    "/etc",
    "/var/lib",
    "/var/log",
    "/run",
    "/usr/sbin",
    "/sbin",
    "/root",
];

impl PathPolicy {
    pub fn new(allow: Vec<PathBuf>, deny: Vec<PathBuf>) -> Self {
        Self {
            allow: allow.into_iter().map(canonicalize_existing_or_self).collect(),
            deny: deny.into_iter().map(canonicalize_existing_or_self).collect(),
        }
    }

    pub fn from_environment() -> Self {
        let home = home_dir();
        let allow: Vec<PathBuf> = match std::env::var("LINUX_MCP_FS_ALLOW") {
            Ok(s) if !s.is_empty() => s.split(':').map(|p| expand_tilde(p, &home)).collect(),
            _ => vec![home.clone()],
        };
        let mut deny: Vec<PathBuf> = DEFAULT_DENY.iter().map(PathBuf::from).collect();
        if let Ok(extra) = std::env::var("LINUX_MCP_FS_DENY_EXTRA") {
            for p in extra.split(':').filter(|s| !s.is_empty()) {
                deny.push(expand_tilde(p, &home));
            }
        }
        Self::new(allow, deny)
    }

    pub fn allow_roots(&self) -> &[PathBuf] {
        &self.allow
    }

    pub fn deny_roots(&self) -> &[PathBuf] {
        &self.deny
    }

    /// Canonicalise `requested`, then check against deny-then-allow.
    /// Returns the canonical path on success.
    pub fn check(&self, requested: &str, mode: PathMode) -> Result<PathBuf, PathPolicyError> {
        let canonical = canonicalize_for_mode(requested, mode)?;

        for d in &self.deny {
            if path_starts_with(&canonical, d) {
                return Err(PathPolicyError::Denied(format!(
                    "path '{}' resolves under denied root '{}'",
                    canonical.display(),
                    d.display()
                )));
            }
        }
        for a in &self.allow {
            if path_starts_with(&canonical, a) {
                return Ok(canonical);
            }
        }
        Err(PathPolicyError::NotAllowed(format!(
            "path '{}' is not under any allow root: {:?}",
            canonical.display(),
            self.allow
        )))
    }
}

fn home_dir() -> PathBuf {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    PathBuf::from("/")
}

fn expand_tilde(s: &str, home: &Path) -> PathBuf {
    if let Some(stripped) = s.strip_prefix("~/") {
        home.join(stripped)
    } else if s == "~" {
        home.to_path_buf()
    } else {
        PathBuf::from(s)
    }
}

fn canonicalize_existing_or_self(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

fn canonicalize_for_mode(requested: &str, mode: PathMode) -> Result<PathBuf, PathPolicyError> {
    if requested.is_empty() {
        return Err(PathPolicyError::Invalid("empty path".into()));
    }
    let home = home_dir();
    let expanded = expand_tilde(requested, &home);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|e| PathPolicyError::Invalid(e.to_string()))?
            .join(expanded)
    };
    if absolute.exists() || mode == PathMode::Read {
        return std::fs::canonicalize(&absolute).or(Ok(absolute));
    }
    // Write to a path that doesn't exist yet — canonicalise the parent only.
    let parent = absolute
        .parent()
        .ok_or_else(|| PathPolicyError::Invalid(format!("path '{}' has no parent", absolute.display())))?;
    if !parent.exists() {
        return Err(PathPolicyError::Invalid(format!(
            "parent directory does not exist: {}",
            parent.display()
        )));
    }
    let canonical_parent = std::fs::canonicalize(parent)
        .map_err(|e| PathPolicyError::Invalid(e.to_string()))?;
    Ok(canonical_parent.join(absolute.file_name().unwrap()))
}

fn path_starts_with(child: &Path, parent: &Path) -> bool {
    let parent = parent.to_string_lossy();
    let child = child.to_string_lossy();
    child == parent || child.starts_with(&format!("{}/", parent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn allow_root_matches_and_denies_siblings() {
        let dir = tempdir().unwrap();
        let p = PathPolicy::new(vec![dir.path().to_path_buf()], vec![]);
        let inside = dir.path().join("file.txt");
        std::fs::write(&inside, b"hi").unwrap();
        assert!(p.check(inside.to_str().unwrap(), PathMode::Read).is_ok());
        assert!(p.check("/etc/passwd", PathMode::Read).is_err());
    }

    #[test]
    fn deny_root_beats_allow() {
        let p = PathPolicy::new(vec![PathBuf::from("/")], vec![PathBuf::from("/etc")]);
        assert!(p.check("/etc/passwd", PathMode::Read).is_err());
    }

    #[test]
    fn write_to_nonexistent_leaf_checks_parent() {
        let dir = tempdir().unwrap();
        let p = PathPolicy::new(vec![dir.path().to_path_buf()], vec![]);
        let target = dir.path().join("new-file.txt");
        assert!(p.check(target.to_str().unwrap(), PathMode::Write).is_ok());
    }

    #[test]
    fn write_to_unreal_parent_fails() {
        let p = PathPolicy::new(vec![PathBuf::from("/tmp")], vec![]);
        assert!(p.check("/this/path/does/not/exist/file.txt", PathMode::Write).is_err());
    }
}
