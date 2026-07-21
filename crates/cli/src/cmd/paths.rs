//! Path resolution helpers for agent-scope data.
//!
//! This module is the CLI-side entry point for the canonical agent data
//! root. The actual logic lives in [`loom_platform::agent_data_root`] so
//! that both the CLI and GUI crates share a single implementation.

use std::path::PathBuf;

/// Resolve the agent data root using the canonical fallback chain.
///
/// Delegates to [`loom_platform::agent_data_root`]. See that function's
/// documentation for the fallback order and rationale.
///
/// All CLI commands that need the agent data root (serve, spec, workspace,
/// thread, channel, agent, skill, reload) should call this function instead
/// of duplicating the env-var + fallback logic inline.
pub fn agent_data_root() -> PathBuf {
    loom_platform::agent_data_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    // SAFETY: env var mutation is process-local; each test saves and
    // restores LOOM_AGENT_DATA_ROOT to avoid leaking state.

    #[test]
    fn agent_data_root_honors_env_override() {
        let key = "LOOM_AGENT_DATA_ROOT";
        let saved = std::env::var_os(key);
        std::env::set_var(key, "/tmp/loom-cli-test-data-root");
        let root = agent_data_root();
        assert_eq!(root, PathBuf::from("/tmp/loom-cli-test-data-root"));
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn agent_data_root_ignores_empty_env() {
        // An empty LOOM_AGENT_DATA_ROOT must fall through to the default
        // (covers defect D2 — previously spec.rs produced an empty PathBuf).
        let key = "LOOM_AGENT_DATA_ROOT";
        let saved = std::env::var_os(key);
        std::env::set_var(key, "");
        let root = agent_data_root();
        assert_ne!(root, PathBuf::from(""));
        assert!(
            root.ends_with("agents"),
            "expected <data_dir>/loom/agents, got {}",
            root.display()
        );
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn agent_data_root_falls_back_to_data_dir() {
        // With no env var set, the result should end with loom/agents.
        let key = "LOOM_AGENT_DATA_ROOT";
        let saved = std::env::var_os(key);
        std::env::remove_var(key);
        let root = agent_data_root();
        assert!(
            root.ends_with("agents"),
            "expected <data_dir>/loom/agents, got {}",
            root.display()
        );
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
