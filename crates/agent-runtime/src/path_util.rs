//! Platform path utilities — re-export bridge to `loom_platform::path`.
//!
//! The canonical implementation lives in
//! [`loom_platform::path`](https://docs.rs/loom-platform). This module exists
//! only as a transition bridge so external consumers continue to compile
//! without churn during the P0-PAL migration. New code should import from
//! `loom_platform::path` directly.
//!
//! ## Migration status
//!
//! - **P0-PAL-2**: logic moved into `loom_platform::path`; this file
//!   re-exports it under the historical name.
//! - **P0-PAL-4 / 4b** (current): internal call sites — `command.rs`,
//!   `interactive.rs`, `acp.rs`, `cli`, `loom-shell` — are migrated to
//!   `loom_platform::*` directly. This module is `#[deprecated]` from here
//!   onwards; the bridge is retained only for any straggling external
//!   consumers we have not yet enumerated.
//! - **P0-PAL-9**: this module is removed (ARCH §6.1).
//!
//! Note: the `CREATE_NO_WINDOW` / `CREATE_BREAKAWAY_FROM_JOB` constants
//! historically lived here because of file-locality with `unc_prefix_path`;
//! they are *process* spawn flags and moved to
//! [`loom_platform::process`] in **P0-PAL-3**.

use std::path::{Path, PathBuf};

/// Windows `CREATE_NO_WINDOW` — prevents a console window from appearing.
///
/// Re-export of [`loom_platform::process::CREATE_NO_WINDOW`].
#[cfg(windows)]
#[deprecated(
    since = "0.1.0",
    note = "use loom_platform::process::CREATE_NO_WINDOW directly; the agent-runtime bridge is removed in P0-PAL-9"
)]
pub use loom_platform::process::CREATE_NO_WINDOW;

/// Windows `CREATE_BREAKAWAY_FROM_JOB` — lets the child escape a parent's
/// job object (fixes `ERROR_PRIVILEGE_NOT_HELD` when spawning from the
/// Tauri GUI).
///
/// Re-export of [`loom_platform::process::CREATE_BREAKAWAY_FROM_JOB`].
#[cfg(windows)]
#[deprecated(
    since = "0.1.0",
    note = "use loom_platform::process::CREATE_BREAKAWAY_FROM_JOB directly; the agent-runtime bridge is removed in P0-PAL-9"
)]
pub use loom_platform::process::CREATE_BREAKAWAY_FROM_JOB;

/// Prefix a path with the Windows `\\?\` UNC prefix to bypass the
/// 260-character `MAX_PATH` limit. No-op on non-Windows.
///
/// Bridge for [`loom_platform::path::unc_prefix_path`].
#[deprecated(
    since = "0.1.0",
    note = "use loom_platform::path::unc_prefix_path directly; the agent-runtime bridge is removed in P0-PAL-9"
)]
pub fn unc_prefix_path(path: PathBuf) -> PathBuf {
    loom_platform::path::unc_prefix_path(path)
}

/// Normalise path separators to the platform-native form (Windows: `/` →
/// `\`). No-op on Unix.
///
/// Bridge for [`loom_platform::path::normalize_path_separators`].
#[deprecated(
    since = "0.1.0",
    note = "use loom_platform::path::normalize_path_separators directly; the agent-runtime bridge is removed in P0-PAL-9"
)]
pub fn normalize_path_separators(path: PathBuf) -> PathBuf {
    loom_platform::path::normalize_path_separators(path)
}

/// Create a directory (and all parents), applying the Windows UNC prefix
/// when needed to bypass `MAX_PATH`. Plain `std::fs::create_dir_all` on
/// Unix.
///
/// Bridge for [`loom_platform::path::create_dir_all`].
#[deprecated(
    since = "0.1.0",
    note = "use loom_platform::path::create_dir_all directly; the agent-runtime bridge is removed in P0-PAL-9"
)]
pub fn create_dir_all_unc(path: &Path) -> std::io::Result<()> {
    loom_platform::path::create_dir_all(path)
}
