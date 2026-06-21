//! Platform path utilities — re-export bridge to `loom_platform::path`.
//!
//! The canonical implementation lives in
//! [`loom_platform::path`](https://docs.rs/loom-platform). This module exists
//! only as a transition bridge so existing consumers continue to compile
//! without churn during P0-PAL migration. New code should import from
//! `loom_platform::path` directly.
//!
//! ## Migration status
//!
//! - **P0-PAL-2** (current): logic lives in `loom_platform::path`; this file
//!   re-exports it under the historical name. Internal call sites
//!   (`command.rs` / `interactive.rs` / `acp.rs` / `cli::agent_serve`) are
//!   updated in **P0-PAL-4**.
//! - **Future**: this module gets `#[deprecated]` after P0-PAL-4 and is
//!   removed one minor version after consumers are migrated (ARCH §6.1).
//!
//! Note: the `CREATE_NO_WINDOW` / `CREATE_BREAKAWAY_FROM_JOB` constants
//! historically lived here because of file-locality with `unc_prefix_path`;
//! they are *process* spawn flags and moved to
//! [`loom_platform::process`] in **P0-PAL-3**. They are still re-exported
//! here verbatim so the spawn-integration tests
//! (`tests/windows_spawn_integration.rs`) and any external consumers
//! continue to compile during the P0-PAL-4 migration window.

use std::path::{Path, PathBuf};

/// Windows `CREATE_NO_WINDOW` — prevents a console window from appearing.
///
/// Re-export of [`loom_platform::process::CREATE_NO_WINDOW`]. The
/// canonical home is `loom_platform::process`; this bridge entry is
/// retained for the P0-PAL-4 migration window.
#[cfg(windows)]
pub use loom_platform::process::CREATE_NO_WINDOW;

/// Windows `CREATE_BREAKAWAY_FROM_JOB` — lets the child escape a parent's
/// job object (fixes `ERROR_PRIVILEGE_NOT_HELD` when spawning from the
/// Tauri GUI).
///
/// Re-export of [`loom_platform::process::CREATE_BREAKAWAY_FROM_JOB`]. The
/// canonical home is `loom_platform::process`; this bridge entry is
/// retained for the P0-PAL-4 migration window.
#[cfg(windows)]
pub use loom_platform::process::CREATE_BREAKAWAY_FROM_JOB;

/// Prefix a path with the Windows `\\?\` UNC prefix to bypass the
/// 260-character `MAX_PATH` limit. No-op on non-Windows.
///
/// Bridge for [`loom_platform::path::unc_prefix_path`].
pub fn unc_prefix_path(path: PathBuf) -> PathBuf {
    loom_platform::path::unc_prefix_path(path)
}

/// Normalise path separators to the platform-native form (Windows: `/` →
/// `\`). No-op on Unix.
///
/// Bridge for [`loom_platform::path::normalize_path_separators`].
pub fn normalize_path_separators(path: PathBuf) -> PathBuf {
    loom_platform::path::normalize_path_separators(path)
}

/// Create a directory (and all parents), applying the Windows UNC prefix
/// when needed to bypass `MAX_PATH`. Plain `std::fs::create_dir_all` on
/// Unix.
///
/// Bridge for [`loom_platform::path::create_dir_all`].
pub fn create_dir_all_unc(path: &Path) -> std::io::Result<()> {
    loom_platform::path::create_dir_all(path)
}
