//! rmcp Server wiring. Uses the manual ServerHandler trait so we can register
//! tools by name (mirroring linux-mcp's typed dispatch from main.swift's Server).
//! Each tool reads its args from a JSON Map and produces a CallToolResult.

use crate::display::{DisplayBackend, Helpers};
use crate::error::ToolError;
use crate::schema as sx;
use crate::tools;
use crate::util;
use linux_mcp_core::{AuditLog, PathPolicy, ProcessPolicy};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Implementation, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler};
use serde_json::{json, Map, Value};
use std::future::Future;
use std::sync::Arc;

pub struct LinuxMcpServer {
    audit: Arc<AuditLog>,
    helpers: Arc<Helpers>,
    fs_policy: Arc<PathPolicy>,
    proc_policy: Arc<ProcessPolicy>,
    sessions: Arc<tools::process::SessionRegistry>,
}

impl LinuxMcpServer {
    pub fn new(audit: Arc<AuditLog>, backend: DisplayBackend) -> Self {
        Self {
            audit,
            helpers: Arc::new(Helpers::new(backend)),
            fs_policy: Arc::new(PathPolicy::from_environment()),
            proc_policy: Arc::new(ProcessPolicy::from_environment()),
            sessions: Arc::new(tools::process::SessionRegistry::new()),
        }
    }

    fn tool(name: &str, desc: &str, schema: Value) -> Tool {
        let map = match schema {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        Tool {
            name: name.to_string().into(),
            title: None,
            description: Some(desc.to_string().into()),
            input_schema: Arc::new(map),
            output_schema: None,
            annotations: None,
            execution: None,
            icons: None,
            meta: None,
        }
    }
}

impl ServerHandler for LinuxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "linux-mcp".into(),
                title: Some("Linux Native Control".into()),
                version: linux_mcp_core::VERSION.into(),
                description: Some(
                    "Native Linux MCP server: filesystem, process, clipboard, screenshots, windows, input."
                        .into(),
                ),
                icons: None,
                website_url: Some("https://github.com/MichaelAdamGroberman/linux-mcp".into()),
            },
            instructions: Some(
                "Linux native control. Filesystem + process tools are allow-listed via \
                 LINUX_MCP_FS_ALLOW / LINUX_MCP_PROCESS_ALLOW. Display tools detect X11/Wayland."
                    .into(),
            ),
        }
    }

    fn list_tools(
        &self,
        _params: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            Ok(ListToolsResult {
                next_cursor: None,
                tools: tool_descriptors(),
                meta: None,
            })
        }
    }

    fn call_tool(
        &self,
        params: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let name = params.name.to_string();
            let args = params.arguments.unwrap_or_default();
            let started = std::time::Instant::now();
            let result = self.dispatch(&name, &args).await;
            match result {
                Ok(r) => {
                    self.audit.info(
                        "tool ok",
                        json!({ "tool": &name, "ms": started.elapsed().as_millis() as u64 }),
                    );
                    Ok(r)
                }
                Err(e) => {
                    self.audit.warn(
                        "tool err",
                        json!({ "tool": &name, "err": e.to_string() }),
                    );
                    Ok(e.into_call_result())
                }
            }
        }
    }
}

impl LinuxMcpServer {
    async fn dispatch(
        &self,
        name: &str,
        args: &Map<String, Value>,
    ) -> Result<CallToolResult, ToolError> {
        match name {
            // --- filesystem ---
            "fs_read" => tools::filesystem::fs_read(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::opt_str(args, "as"),
                sx::opt_i64(args, "offset"),
                sx::opt_i64(args, "max_bytes"),
            )
            .await,
            "fs_read_many" => {
                let arr = args.get("paths").and_then(|v| v.as_array()).ok_or_else(|| {
                    ToolError::coded("missing_arg", "paths array required")
                })?;
                let paths: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                tools::filesystem::fs_read_many(&self.fs_policy, &paths, sx::opt_str(args, "as")).await
            }
            "fs_write" => tools::filesystem::fs_write(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::opt_str(args, "text"),
                sx::opt_str(args, "base64"),
                sx::opt_str(args, "mode"),
            )
            .await,
            "fs_edit" => tools::filesystem::fs_edit(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::require_str(args, "find")?,
                sx::opt_str(args, "replace").unwrap_or(""),
                sx::opt_str(args, "occurrences"),
                sx::opt_i64(args, "expect_count"),
            )
            .await,
            "fs_list" => tools::filesystem::fs_list(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::opt_bool(args, "recursive"),
                sx::opt_i64(args, "max_entries"),
                sx::opt_str(args, "glob"),
            )
            .await,
            "fs_stat" => tools::filesystem::fs_stat(&self.fs_policy, sx::require_str(args, "path")?).await,
            "fs_copy" => tools::filesystem::fs_copy(
                &self.fs_policy,
                sx::require_str(args, "src")?,
                sx::require_str(args, "dst")?,
                sx::opt_bool(args, "overwrite"),
            )
            .await,
            "fs_move" => tools::filesystem::fs_move(
                &self.fs_policy,
                sx::require_str(args, "src")?,
                sx::require_str(args, "dst")?,
                sx::opt_bool(args, "overwrite"),
            )
            .await,
            "fs_make_dir" => tools::filesystem::fs_make_dir(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::opt_bool(args, "parents"),
                sx::opt_str(args, "perms_octal"),
            )
            .await,
            "fs_delete" => tools::filesystem::fs_delete(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::opt_bool(args, "permanent"),
            )
            .await,
            "fs_watch_once" => tools::filesystem::fs_watch_once(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::opt_i64(args, "timeout_ms"),
            )
            .await,
            "fs_xattr_get" => tools::filesystem::fs_xattr_get(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::require_str(args, "name")?,
            )
            .await,
            "fs_xattr_set" => tools::filesystem::fs_xattr_set(
                &self.fs_policy,
                sx::require_str(args, "path")?,
                sx::require_str(args, "name")?,
                sx::opt_str(args, "value_text"),
                sx::opt_str(args, "value_b64"),
            )
            .await,

            // --- process ---
            "process_run" => {
                let argv = args
                    .get("argv")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                tools::process::process_run(
                    &self.proc_policy,
                    &self.fs_policy,
                    argv,
                    sx::opt_str(args, "cmd"),
                    sx::opt_bool(args, "shell"),
                    sx::opt_str(args, "cwd"),
                    sx::opt_str(args, "stdin"),
                    sx::opt_i64(args, "timeout_ms"),
                    sx::opt_str(args, "env_extra"),
                )
                .await
            }
            "process_list" => tools::process::process_list(sx::opt_str(args, "filter")).await,
            "process_kill" => {
                let pid = sx::opt_i64(args, "pid")
                    .ok_or_else(|| ToolError::coded("missing_arg", "pid required"))?;
                tools::process::process_kill(pid, sx::opt_str(args, "signal")).await
            }
            "process_start" => {
                let argv = args
                    .get("argv")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                tools::process::process_start(
                    &self.sessions,
                    &self.proc_policy,
                    &self.fs_policy,
                    argv,
                    sx::opt_str(args, "cmd"),
                    sx::opt_bool(args, "shell"),
                    sx::opt_str(args, "cwd"),
                    sx::opt_str(args, "env_extra"),
                )
                .await
            }
            "process_read_output" => tools::process::process_read_output(
                &self.sessions,
                sx::require_str(args, "session_id")?,
                sx::opt_i64(args, "max_bytes"),
            )
            .await,
            "process_write_input" => tools::process::process_write_input(
                &self.sessions,
                sx::require_str(args, "session_id")?,
                sx::require_str(args, "text")?,
                sx::opt_bool(args, "close_after"),
            )
            .await,
            "process_terminate" => tools::process::process_terminate(
                &self.sessions,
                sx::require_str(args, "session_id")?,
            )
            .await,

            // --- input ---
            "mouse_move" => tools::input::mouse_move(
                &self.helpers,
                sx::opt_i64(args, "x").ok_or_else(|| ToolError::coded("missing_arg", "x required"))?,
                sx::opt_i64(args, "y").ok_or_else(|| ToolError::coded("missing_arg", "y required"))?,
            )
            .await,
            "mouse_click" => tools::input::mouse_click(
                &self.helpers,
                sx::opt_str(args, "button"),
                sx::opt_i64(args, "count"),
            )
            .await,
            "mouse_scroll" => tools::input::mouse_scroll(
                &self.helpers,
                sx::opt_i64(args, "dy").unwrap_or(0),
                sx::opt_i64(args, "dx"),
            )
            .await,
            "key_press" => {
                let mods: Option<Vec<String>> = args
                    .get("modifiers")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                tools::input::key_press(
                    &self.helpers,
                    sx::require_str(args, "key")?,
                    mods.as_deref(),
                )
                .await
            }
            "type_text" => {
                tools::input::type_text(&self.helpers, sx::require_str(args, "text")?).await
            }

            // --- clipboard ---
            "clipboard_read" => tools::clipboard::clipboard_read(&self.helpers).await,
            "clipboard_write" => tools::clipboard::clipboard_write(
                &self.helpers,
                sx::opt_str(args, "text"),
                sx::opt_str(args, "image_png_b64"),
            )
            .await,

            // --- screenshots ---
            "screenshot_screen" => tools::screenshot::screenshot_screen(&self.helpers).await,
            "screenshot_window" => {
                tools::screenshot::screenshot_window(&self.helpers, sx::opt_str(args, "window_id"))
                    .await
            }

            // --- notify / dialog ---
            "notify" => tools::notify::notify(
                &self.helpers,
                sx::require_str(args, "title")?,
                sx::require_str(args, "body")?,
                sx::opt_str(args, "urgency"),
                sx::opt_str(args, "icon"),
                sx::opt_i64(args, "timeout_ms"),
            )
            .await,
            "prompt_user" => tools::dialog::prompt_user(
                &self.helpers,
                sx::require_str(args, "title")?,
                sx::require_str(args, "message")?,
                sx::opt_str(args, "default_value"),
            )
            .await,

            // --- windows ---
            "list_windows" => tools::display_tools::list_windows(&self.helpers).await,
            "focus_window" => tools::display_tools::focus_window(
                &self.helpers,
                sx::require_str(args, "window_id")?,
            )
            .await,
            "move_window" => tools::display_tools::move_window(
                &self.helpers,
                sx::require_str(args, "window_id")?,
                sx::opt_i64(args, "x").ok_or_else(|| ToolError::coded("missing_arg", "x required"))?,
                sx::opt_i64(args, "y").ok_or_else(|| ToolError::coded("missing_arg", "y required"))?,
            )
            .await,
            "resize_window" => tools::display_tools::resize_window(
                &self.helpers,
                sx::require_str(args, "window_id")?,
                sx::opt_i64(args, "width").ok_or_else(|| ToolError::coded("missing_arg", "width required"))?,
                sx::opt_i64(args, "height").ok_or_else(|| ToolError::coded("missing_arg", "height required"))?,
            )
            .await,

            // --- util ---
            "wait_ms" => util::wait_ms(
                sx::opt_i64(args, "ms").ok_or_else(|| ToolError::coded("missing_arg", "ms required"))?,
            )
            .await,

            other => Err(ToolError::coded(
                "unknown_tool",
                format!("no tool named '{other}'"),
            )),
        }
    }
}

fn obj(properties: serde_json::Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn tool_descriptors() -> Vec<Tool> {
    use LinuxMcpServer as S;
    vec![
        // ----- filesystem (14) -----
        S::tool("fs_read", "Read a file (UTF-8 text or base64). Cap 10 MB.",
            obj(json!({
                "path": { "type": "string", "description": "Path to read." },
                "as":   { "type": "string", "enum": ["text", "base64"] },
                "offset": { "type": "integer" },
                "max_bytes": { "type": "integer" }
            }), &["path"])),
        S::tool("fs_read_many", "Batch read up to 50 files / 10 MB total.",
            obj(json!({
                "paths": { "type": "array", "items": { "type": "string" } },
                "as": { "type": "string", "enum": ["text", "base64"] }
            }), &["paths"])),
        S::tool("fs_write", "Write a file (text or base64). Modes: create / overwrite / append.",
            obj(json!({
                "path": { "type": "string" },
                "text": { "type": "string" },
                "base64": { "type": "string" },
                "mode": { "type": "string", "enum": ["create", "overwrite", "append"] }
            }), &["path"])),
        S::tool("fs_edit", "Find/replace inside a file with optional expect_count guard.",
            obj(json!({
                "path": { "type": "string" },
                "find": { "type": "string" },
                "replace": { "type": "string" },
                "occurrences": { "type": "string", "enum": ["all", "first", "last"] },
                "expect_count": { "type": "integer" }
            }), &["path", "find", "replace"])),
        S::tool("fs_list", "List directory entries.",
            obj(json!({
                "path": { "type": "string" },
                "recursive": { "type": "boolean" },
                "max_entries": { "type": "integer" },
                "glob": { "type": "string" }
            }), &["path"])),
        S::tool("fs_stat", "Path metadata.",
            obj(json!({ "path": { "type": "string" } }), &["path"])),
        S::tool("fs_copy", "Copy a file or directory.",
            obj(json!({
                "src": { "type": "string" },
                "dst": { "type": "string" },
                "overwrite": { "type": "boolean" }
            }), &["src", "dst"])),
        S::tool("fs_move", "Move or rename a file or directory.",
            obj(json!({
                "src": { "type": "string" },
                "dst": { "type": "string" },
                "overwrite": { "type": "boolean" }
            }), &["src", "dst"])),
        S::tool("fs_make_dir", "Create a directory (parents optional).",
            obj(json!({
                "path": { "type": "string" },
                "parents": { "type": "boolean" },
                "perms_octal": { "type": "string" }
            }), &["path"])),
        S::tool("fs_delete", "Delete a path. Default = freedesktop Trash. permanent=true unlinks.",
            obj(json!({
                "path": { "type": "string" },
                "permanent": { "type": "boolean" }
            }), &["path"])),
        S::tool("fs_watch_once", "Block until next FS event (or timeout).",
            obj(json!({
                "path": { "type": "string" },
                "timeout_ms": { "type": "integer" }
            }), &["path"])),
        S::tool("fs_xattr_get", "Read an xattr (or list names with name='*').",
            obj(json!({
                "path": { "type": "string" },
                "name": { "type": "string" }
            }), &["path", "name"])),
        S::tool("fs_xattr_set", "Write an xattr (text or base64).",
            obj(json!({
                "path": { "type": "string" },
                "name": { "type": "string" },
                "value_text": { "type": "string" },
                "value_b64": { "type": "string" }
            }), &["path", "name"])),

        // ----- process (7) -----
        S::tool("process_run", "Run an allow-listed process synchronously. Allow-list via LINUX_MCP_PROCESS_ALLOW.",
            obj(json!({
                "argv": { "type": "array", "items": { "type": "string" } },
                "cmd": { "type": "string" },
                "shell": { "type": "boolean" },
                "cwd": { "type": "string" },
                "stdin": { "type": "string" },
                "timeout_ms": { "type": "integer" },
                "env_extra": { "type": "string" }
            }), &[])),
        S::tool("process_start", "Start an allow-listed process asynchronously, returning a session_id.",
            obj(json!({
                "argv": { "type": "array", "items": { "type": "string" } },
                "cmd": { "type": "string" },
                "shell": { "type": "boolean" },
                "cwd": { "type": "string" },
                "env_extra": { "type": "string" }
            }), &[])),
        S::tool("process_read_output", "Read stdout/stderr from an async session (non-blocking).",
            obj(json!({
                "session_id": { "type": "string" },
                "max_bytes": { "type": "integer" }
            }), &["session_id"])),
        S::tool("process_write_input", "Write to a session's stdin.",
            obj(json!({
                "session_id": { "type": "string" },
                "text": { "type": "string" },
                "close_after": { "type": "boolean" }
            }), &["session_id", "text"])),
        S::tool("process_terminate", "Terminate an async session.",
            obj(json!({ "session_id": { "type": "string" } }), &["session_id"])),
        S::tool("process_list", "List running processes (read /proc).",
            obj(json!({ "filter": { "type": "string" } }), &[])),
        S::tool("process_kill", "Send a signal to a pid.",
            obj(json!({
                "pid": { "type": "integer" },
                "signal": { "type": "string", "enum": ["TERM", "KILL", "INT", "HUP", "USR1", "USR2"] }
            }), &["pid"])),

        // ----- input (5) -----
        S::tool("mouse_move", "Move the mouse to (x, y).",
            obj(json!({ "x": { "type": "integer" }, "y": { "type": "integer" } }), &["x", "y"])),
        S::tool("mouse_click", "Click at the current cursor position.",
            obj(json!({
                "button": { "type": "string", "enum": ["left", "right", "middle"] },
                "count": { "type": "integer" }
            }), &[])),
        S::tool("mouse_scroll", "Scroll vertically (positive dy = up).",
            obj(json!({ "dy": { "type": "integer" }, "dx": { "type": "integer" } }), &[])),
        S::tool("key_press", "Press a key (with optional modifiers).",
            obj(json!({
                "key": { "type": "string" },
                "modifiers": { "type": "array", "items": { "type": "string" } }
            }), &["key"])),
        S::tool("type_text", "Type a Unicode string (cap 10k chars).",
            obj(json!({ "text": { "type": "string" } }), &["text"])),

        // ----- clipboard (2) -----
        S::tool("clipboard_read", "Read the system clipboard.", obj(json!({}), &[])),
        S::tool("clipboard_write", "Write text or PNG image to the clipboard.",
            obj(json!({
                "text": { "type": "string" },
                "image_png_b64": { "type": "string" }
            }), &[])),

        // ----- screenshots (2) -----
        S::tool("screenshot_screen", "Capture the screen as base64 PNG.", obj(json!({}), &[])),
        S::tool("screenshot_window", "Capture a window by id (X11 only for now).",
            obj(json!({ "window_id": { "type": "string" } }), &[])),

        // ----- notify / dialog (2) -----
        S::tool("notify", "Send a notification via notify-send.",
            obj(json!({
                "title": { "type": "string" },
                "body": { "type": "string" },
                "urgency": { "type": "string", "enum": ["low", "normal", "critical"] },
                "icon": { "type": "string" },
                "timeout_ms": { "type": "integer" }
            }), &["title", "body"])),
        S::tool("prompt_user", "Show a native input dialog (zenity/kdialog).",
            obj(json!({
                "title": { "type": "string" },
                "message": { "type": "string" },
                "default_value": { "type": "string" }
            }), &["title", "message"])),

        // ----- windows (4) -----
        S::tool("list_windows", "List on-screen windows (X11 via wmctrl; Wayland via swaymsg/hyprctl).",
            obj(json!({}), &[])),
        S::tool("focus_window", "Focus a window by id.",
            obj(json!({ "window_id": { "type": "string" } }), &["window_id"])),
        S::tool("move_window", "Move a window to (x, y).",
            obj(json!({
                "window_id": { "type": "string" },
                "x": { "type": "integer" },
                "y": { "type": "integer" }
            }), &["window_id", "x", "y"])),
        S::tool("resize_window", "Resize a window.",
            obj(json!({
                "window_id": { "type": "string" },
                "width": { "type": "integer" },
                "height": { "type": "integer" }
            }), &["window_id", "width", "height"])),

        // ----- util (1) -----
        S::tool("wait_ms", "Sleep for N ms.",
            obj(json!({ "ms": { "type": "integer" } }), &["ms"])),
    ]
}
