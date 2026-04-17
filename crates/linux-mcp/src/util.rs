use crate::error::{ok_json, ToolError};
use rmcp::model::CallToolResult;

/// `wait_ms` lives at the top level since it's neither display-server nor
/// per-app. Useful between focus changes / iphone_mirror / window manipulations.
pub async fn wait_ms(ms: i64) -> Result<CallToolResult, ToolError> {
    let clamped = ms.clamp(1, 60_000);
    tokio::time::sleep(std::time::Duration::from_millis(clamped as u64)).await;
    Ok(ok_json(serde_json::json!({ "waited_ms": clamped })))
}
