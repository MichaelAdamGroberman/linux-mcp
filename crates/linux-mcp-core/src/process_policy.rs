//! Allow-list policy for `process_*` tools.
//!
//! Default state: empty allow-list → every `process_*` call is refused. Users
//! opt in by setting `LINUX_MCP_PROCESS_ALLOW="git:rg:gh:fd:python3:node"`.
//! Match is on the basename of `argv[0]`.

#[derive(Debug, Clone, Default)]
pub struct ProcessPolicy {
    pub allowed: Vec<String>,
}

impl ProcessPolicy {
    pub fn from_environment() -> Self {
        let raw = std::env::var("LINUX_MCP_PROCESS_ALLOW").unwrap_or_default();
        let allowed = raw
            .split(':')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self { allowed }
    }

    pub fn is_allowed(&self, basename: &str) -> bool {
        self.allowed.iter().any(|a| a == basename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allow_refuses_everything() {
        let p = ProcessPolicy { allowed: vec![] };
        assert!(!p.is_allowed("git"));
        assert!(!p.is_allowed(""));
    }

    #[test]
    fn allows_listed_basenames() {
        let p = ProcessPolicy {
            allowed: vec!["git".into(), "rg".into()],
        };
        assert!(p.is_allowed("git"));
        assert!(p.is_allowed("rg"));
        assert!(!p.is_allowed("rm"));
    }
}
