use rmcp::model::{CallToolResult, Content};
use thiserror::Error;

/// All tool errors funnel through here so the MCP response shape is uniform.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("{code}: {message}")]
    Coded { code: &'static str, message: String },
    #[error(transparent)]
    Path(#[from] linux_mcp_core::PathPolicyError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl ToolError {
    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        Self::Coded {
            code,
            message: message.into(),
        }
    }

    pub fn into_call_result(self) -> CallToolResult {
        let body = match &self {
            Self::Coded { code, message } => format!("{code}: {message}"),
            Self::Path(p) => format!("{}: {}", p.code(), p),
            Self::Io(e) => format!("io_error: {e}"),
            Self::Other(e) => format!("internal_error: {e:#}"),
        };
        CallToolResult::error(vec![Content::text(body)])
    }
}

pub fn ok_json(value: serde_json::Value) -> CallToolResult {
    let s = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
    CallToolResult::success(vec![Content::text(s)])
}
