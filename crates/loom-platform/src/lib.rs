//! # loom-platform
//!
//! Workspace-level Platform Abstraction Layer (PAL). See the crate README for
//! design rules and the module-by-module status table.
//!
//! ## Invariants
//!
//! - Every cross-platform difference in the Loom workspace either lives here
//!   or is documented as an explicit, justified residual `cfg` in the
//!   originating business crate (see PRD §3.1 and ARCH §7.1).
//! - Public signatures are **identical** on Windows and Unix. The
//!   workspace-level `cargo check` on all three OSes is the contract check.
//! - There is **no runtime polymorphism**: all platform dispatch happens at
//!   compile time via `#[cfg(...)]`. No `Box<dyn Platform>`, no trait
//!   objects in hot paths.

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod console;
pub mod env;
pub mod fs;
pub mod ipc;
pub mod path;
pub mod process;
pub mod signal;
pub mod time;

use std::path::PathBuf;

/// Resolve the agent data root using the canonical fallback chain.
///
/// Order:
/// 1. `LOOM_AGENT_DATA_ROOT` env var (non-empty)
/// 2. `dirs::data_dir()/loom`
/// 3. `.loom/agents-data` (last-resort relative)
///
/// All agent-scope path resolution (serve, spec, skill, reload,
/// workspace, thread, GUI IPC) MUST route through this function to
/// guarantee a single source of truth for the default data root.
/// Callers are responsible for appending subdirectories such as
/// `agents/<actor_id>` — this function returns the bare data root.
///
/// Uses `var_os` (not `var`) so that non-UTF-8 paths on Windows are
/// handled correctly. An empty env-var value is treated as unset to
/// avoid producing an empty `PathBuf` (defect D2).
pub fn agent_data_root() -> PathBuf {
    if let Some(v) = std::env::var_os("LOOM_AGENT_DATA_ROOT") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    dirs::data_dir()
        .map(|d| d.join("loom"))
        .unwrap_or_else(|| PathBuf::from(".loom").join("agents-data"))
}
