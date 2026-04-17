//! 7 process tools: run (sync), start (async session) + read_output / write_input
//! / terminate, list, kill. Strict allow-list via LINUX_MCP_PROCESS_ALLOW.

use crate::error::{ok_json, ToolError};
use linux_mcp_core::{PathMode, PathPolicy, ProcessPolicy};
use rmcp::model::CallToolResult;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

pub const MAX_STDOUT: usize = 1 * 1024 * 1024;
pub const MAX_STDERR: usize = 256 * 1024;
pub const DEFAULT_TIMEOUT_MS: i64 = 30_000;
pub const MAX_TIMEOUT_MS: i64 = 300_000;

pub struct SessionRegistry {
    inner: Mutex<std::collections::HashMap<String, Arc<Mutex<Session>>>>,
}

pub struct Session {
    pub id: String,
    pub child: Child,
    pub buffered_stdout: Vec<u8>,
    pub buffered_stderr: Vec<u8>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(std::collections::HashMap::new()),
        }
    }
    pub async fn insert(&self, s: Arc<Mutex<Session>>) {
        let id = s.lock().await.id.clone();
        self.inner.lock().await.insert(id, s);
    }
    pub async fn get(&self, id: &str) -> Option<Arc<Mutex<Session>>> {
        self.inner.lock().await.get(id).cloned()
    }
    pub async fn remove(&self, id: &str) {
        self.inner.lock().await.remove(id);
    }
}

fn resolve_argv(
    argv: Option<Vec<String>>,
    cmd: Option<&str>,
    shell: bool,
    policy: &ProcessPolicy,
) -> Result<Vec<String>, ToolError> {
    let mut argv = if shell {
        let cmd = cmd.ok_or_else(|| ToolError::coded("missing_arg", "shell=true requires cmd"))?;
        vec!["/bin/sh".to_string(), "-c".to_string(), cmd.to_string()]
    } else {
        argv.ok_or_else(|| ToolError::coded("missing_arg", "argv (array) or shell=true + cmd required"))?
    };
    if argv.is_empty() {
        return Err(ToolError::coded("missing_arg", "argv must be non-empty"));
    }
    let basename = std::path::Path::new(&argv[0])
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| argv[0].clone());
    if !policy.is_allowed(&basename) {
        return Err(ToolError::coded(
            "process_not_allowed",
            format!(
                "executable '{basename}' not in LINUX_MCP_PROCESS_ALLOW (current: {:?})",
                policy.allowed
            ),
        ));
    }
    if !std::path::Path::new(&argv[0]).is_absolute() {
        if let Ok(resolved) = which::which(&basename) {
            argv[0] = resolved.display().to_string();
        }
    }
    Ok(argv)
}

fn resolve_cwd(cwd: Option<&str>, fs_policy: &PathPolicy) -> Result<PathBuf, ToolError> {
    let p = cwd
        .map(|s| s.to_string())
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    fs_policy
        .check(&p, PathMode::Read)
        .map_err(|e| ToolError::coded(e.code(), format!("cwd: {e}")))
}

fn merged_env(extra: Option<&str>) -> Vec<(String, String)> {
    let mut env: std::collections::HashMap<String, String> =
        std::env::vars().collect();
    if let Some(extra) = extra {
        for line in extra.split('\n') {
            if let Some((k, v)) = line.split_once('=') {
                env.insert(k.to_string(), v.to_string());
            }
        }
    }
    env.into_iter().collect()
}

// ----- process_run -----
pub async fn process_run(
    process_policy: &ProcessPolicy,
    fs_policy: &PathPolicy,
    argv: Option<Vec<String>>,
    cmd: Option<&str>,
    shell: Option<bool>,
    cwd: Option<&str>,
    stdin: Option<&str>,
    timeout_ms: Option<i64>,
    env_extra: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    let argv = resolve_argv(argv, cmd, shell.unwrap_or(false), process_policy)?;
    let cwd = resolve_cwd(cwd, fs_policy)?;
    let timeout = Duration::from_millis(
        timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).clamp(100, MAX_TIMEOUT_MS) as u64,
    );

    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(merged_env(env_extra));
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }

    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| ToolError::coded("spawn_failed", e.to_string()))?;

    if let (Some(stdin_text), Some(child_stdin)) = (stdin, child.stdin.as_mut()) {
        child_stdin.write_all(stdin_text.as_bytes()).await?;
        let _ = child_stdin.shutdown().await;
    }

    let wait = tokio::time::timeout(timeout, child.wait()).await;
    let timed_out = wait.is_err();
    if timed_out {
        let _ = child.start_kill();
    }
    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => return Err(ToolError::coded("wait_failed", e.to_string())),
    };

    let mut stdout_buf = Vec::with_capacity(MAX_STDOUT.min(64 * 1024));
    let mut stderr_buf = Vec::with_capacity(MAX_STDERR.min(64 * 1024));
    if let Some(mut out) = child.stdout.take() {
        let mut tmp = vec![0u8; 64 * 1024];
        while stdout_buf.len() < MAX_STDOUT {
            match out.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let take = n.min(MAX_STDOUT - stdout_buf.len());
                    stdout_buf.extend_from_slice(&tmp[..take]);
                }
            }
        }
    }
    if let Some(mut err) = child.stderr.take() {
        let mut tmp = vec![0u8; 32 * 1024];
        while stderr_buf.len() < MAX_STDERR {
            match err.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let take = n.min(MAX_STDERR - stderr_buf.len());
                    stderr_buf.extend_from_slice(&tmp[..take]);
                }
            }
        }
    }

    Ok(ok_json(serde_json::json!({
        "argv": argv,
        "cwd": cwd.display().to_string(),
        "exit_code": status.code().unwrap_or(-1),
        "timed_out": timed_out,
        "elapsed_ms": started.elapsed().as_millis() as u64,
        "stdout": String::from_utf8_lossy(&stdout_buf).to_string(),
        "stdout_truncated": stdout_buf.len() >= MAX_STDOUT,
        "stderr": String::from_utf8_lossy(&stderr_buf).to_string(),
        "stderr_truncated": stderr_buf.len() >= MAX_STDERR,
    })))
}

// ----- process_list -----
pub async fn process_list(filter: Option<&str>) -> Result<CallToolResult, ToolError> {
    let needle = filter.map(|s| s.to_lowercase());
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let proc_dir = std::fs::read_dir("/proc")
        .map_err(|e| ToolError::coded("proc_read_failed", e.to_string()))?;
    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        let Ok(pid) = name_str.parse::<i32>() else { continue };
        let status = std::fs::read_to_string(entry.path().join("status")).unwrap_or_default();
        let comm = status
            .lines()
            .find_map(|l| l.strip_prefix("Name:\t"))
            .unwrap_or("?")
            .trim()
            .to_string();
        let ppid = status
            .lines()
            .find_map(|l| l.strip_prefix("PPid:\t"))
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(-1);
        let uid = status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:\t"))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        if let Some(n) = &needle {
            if !comm.to_lowercase().contains(n) {
                continue;
            }
        }
        rows.push(serde_json::json!({
            "pid": pid,
            "ppid": ppid,
            "uid": uid,
            "command": comm,
        }));
    }
    Ok(ok_json(serde_json::json!({
        "count": rows.len(),
        "processes": rows,
    })))
}

// ----- process_kill -----
pub async fn process_kill(pid: i64, signal: Option<&str>) -> Result<CallToolResult, ToolError> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::{Pid, Uid};
    let signal = signal.unwrap_or("TERM");
    let signo = match signal {
        "KILL" => Signal::SIGKILL,
        "INT" => Signal::SIGINT,
        "HUP" => Signal::SIGHUP,
        "USR1" => Signal::SIGUSR1,
        "USR2" => Signal::SIGUSR2,
        _ => Signal::SIGTERM,
    };
    let pid_i32 = pid as i32;
    if pid_i32 <= 1 {
        return Err(ToolError::coded("process_protected", format!("refusing to signal pid {pid}")));
    }
    if let Some(owner) = uid_of_pid(pid_i32) {
        let me = Uid::current().as_raw();
        if owner != me && std::env::var("LINUX_MCP_PROCESS_KILL_ANY").as_deref() != Ok("1") {
            return Err(ToolError::coded(
                "process_cross_user",
                format!("refusing pid {pid} owned by uid {owner} (current uid {me}); set LINUX_MCP_PROCESS_KILL_ANY=1 to override"),
            ));
        }
    }
    kill(Pid::from_raw(pid_i32), signo)
        .map_err(|e| ToolError::coded("kill_failed", e.to_string()))?;
    Ok(ok_json(serde_json::json!({ "killed": pid, "signal": signal })))
}

fn uid_of_pid(pid: i32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:\t"))?
        .split_whitespace()
        .next()?
        .parse::<u32>()
        .ok()
}

// ----- process_start (async session) -----
pub async fn process_start(
    registry: &SessionRegistry,
    process_policy: &ProcessPolicy,
    fs_policy: &PathPolicy,
    argv: Option<Vec<String>>,
    cmd: Option<&str>,
    shell: Option<bool>,
    cwd: Option<&str>,
    env_extra: Option<&str>,
) -> Result<CallToolResult, ToolError> {
    let argv = resolve_argv(argv, cmd, shell.unwrap_or(false), process_policy)?;
    let cwd = resolve_cwd(cwd, fs_policy)?;
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .envs(merged_env(env_extra));
    let child = command
        .spawn()
        .map_err(|e| ToolError::coded("spawn_failed", e.to_string()))?;
    let pid = child.id().unwrap_or(0);
    let session = Arc::new(Mutex::new(Session {
        id: Uuid::new_v4().to_string(),
        child,
        buffered_stdout: Vec::new(),
        buffered_stderr: Vec::new(),
    }));
    let id = session.lock().await.id.clone();
    registry.insert(session).await;
    Ok(ok_json(serde_json::json!({
        "session_id": id,
        "pid": pid,
    })))
}

pub async fn process_read_output(
    registry: &SessionRegistry,
    session_id: &str,
    max_bytes: Option<i64>,
) -> Result<CallToolResult, ToolError> {
    let max = max_bytes.unwrap_or(65_536).clamp(1, MAX_STDOUT as i64) as usize;
    let session = registry.get(session_id).await.ok_or_else(|| {
        ToolError::coded("session_not_found", format!("no session {session_id}"))
    })?;
    let mut s = session.lock().await;
    let mut new_out = Vec::new();
    let mut new_err = Vec::new();
    if let Some(stdout) = s.child.stdout.as_mut() {
        let mut tmp = vec![0u8; max];
        match tokio::time::timeout(Duration::from_millis(50), stdout.read(&mut tmp)).await {
            Ok(Ok(n)) if n > 0 => new_out.extend_from_slice(&tmp[..n]),
            _ => {}
        }
    }
    if let Some(stderr) = s.child.stderr.as_mut() {
        let mut tmp = vec![0u8; max];
        match tokio::time::timeout(Duration::from_millis(50), stderr.read(&mut tmp)).await {
            Ok(Ok(n)) if n > 0 => new_err.extend_from_slice(&tmp[..n]),
            _ => {}
        }
    }
    let running = s.child.try_wait().map(|o| o.is_none()).unwrap_or(false);
    let exit_code = s.child.try_wait().ok().flatten().and_then(|st| st.code()).unwrap_or(-1);
    Ok(ok_json(serde_json::json!({
        "session_id": session_id,
        "running": running,
        "exit_code": exit_code,
        "stdout": String::from_utf8_lossy(&new_out).to_string(),
        "stderr": String::from_utf8_lossy(&new_err).to_string(),
    })))
}

pub async fn process_write_input(
    registry: &SessionRegistry,
    session_id: &str,
    text: &str,
    close_after: Option<bool>,
) -> Result<CallToolResult, ToolError> {
    let session = registry.get(session_id).await.ok_or_else(|| {
        ToolError::coded("session_not_found", format!("no session {session_id}"))
    })?;
    let mut s = session.lock().await;
    if let Some(stdin) = s.child.stdin.as_mut() {
        stdin.write_all(text.as_bytes()).await?;
        if close_after.unwrap_or(false) {
            let _ = stdin.shutdown().await;
        }
    } else {
        return Err(ToolError::coded("stdin_closed", "session stdin is closed"));
    }
    Ok(ok_json(serde_json::json!({ "wrote": text.len() })))
}

pub async fn process_terminate(
    registry: &SessionRegistry,
    session_id: &str,
) -> Result<CallToolResult, ToolError> {
    let session = registry.get(session_id).await.ok_or_else(|| {
        ToolError::coded("session_not_found", format!("no session {session_id}"))
    })?;
    let exit_code = {
        let mut s = session.lock().await;
        let _ = s.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(1), s.child.wait()).await;
        s.child.try_wait().ok().flatten().and_then(|st| st.code()).unwrap_or(-1)
    };
    registry.remove(session_id).await;
    Ok(ok_json(serde_json::json!({
        "terminated": session_id,
        "exit_code": exit_code,
    })))
}
