//! Shared primitives: filesystem allow-list policy, process exec policy,
//! and the JSONL audit log writer.

pub mod path_policy;
pub mod process_policy;
pub mod audit;

pub use path_policy::{PathPolicy, PathMode, PathPolicyError};
pub use process_policy::ProcessPolicy;
pub use audit::AuditLog;

pub const VERSION: &str = "0.1.0";
