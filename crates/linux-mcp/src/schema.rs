//! Shared JSON-schema helpers and small extractors for tool argument maps.
//!
//! rmcp lets us expose strongly-typed parameter structs via #[derive(JsonSchema)],
//! but a few of our tools need free-form maps (e.g. mixed text/base64 inputs)
//! where a typed Args struct would be awkward. For those we use these helpers.

pub fn require_str<'a>(map: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Result<&'a str, crate::error::ToolError> {
    map.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::error::ToolError::coded("missing_arg", format!("required string '{key}' missing or empty")))
}

pub fn opt_str<'a>(map: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(|v| v.as_str())
}

pub fn opt_bool(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(|v| v.as_bool())
}

pub fn opt_i64(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<i64> {
    map.get(key).and_then(|v| v.as_i64())
}
