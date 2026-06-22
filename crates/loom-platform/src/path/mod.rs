//! Path utilities — UNC prefix, normalisation, `create_dir_all` wrapper.
//!
//! On Windows, paths longer than 260 characters (MAX_PATH) require the `\\?\`
//! UNC prefix and exclusive backslash separators. These utilities are the
//! single source of truth for that logic across the workspace.
//!
//! On non-Windows platforms, these are transparent no-ops so callers can
//! invoke them unconditionally without `#[cfg]` gating at the call site.
//!
//! ## Migration history
//!
//! Migrated from `agent-runtime/src/path_util.rs` in P0-PAL-2
//! (ARCH §2.1 / §6.1). The original bridge module was removed in
//! P0-PAL-9; all consumers now import from `loom_platform::path`
//! directly.

use std::path::{Path, PathBuf};

#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
mod unix;

/// Prefix a path with the Windows `\\?\` UNC prefix to bypass the 260-char
/// `MAX_PATH` limit.
///
/// UNC paths bypass Win32 path normalisation, so they must use backslashes
/// exclusively and cannot contain `.` or `..` components. The path is
/// normalised before the prefix is applied.
///
/// On non-Windows platforms this returns the input path unchanged.
///
/// # Idempotence
///
/// If the path already starts with `\\?\` this returns it unchanged — the
/// prefix is never doubled.
pub fn unc_prefix_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        windows::unc_prefix_path(path)
    }
    #[cfg(not(windows))]
    {
        unix::unc_prefix_path(path)
    }
}

/// Normalise path separators to the platform-native separator.
///
/// On Windows forward slashes are replaced with backslashes. On Unix this is
/// a no-op.
pub fn normalize_path_separators(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        windows::normalize_path_separators(path)
    }
    #[cfg(not(windows))]
    {
        unix::normalize_path_separators(path)
    }
}

/// `std::fs::create_dir_all` that transparently applies the Windows UNC
/// prefix when needed to bypass the 260-char `MAX_PATH` limit.
///
/// Relative paths are resolved to absolute first — UNC `\\?\` paths must be
/// absolute. On Unix this delegates to plain `std::fs::create_dir_all`.
pub fn create_dir_all(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        windows::create_dir_all(path)
    }
    #[cfg(not(windows))]
    {
        unix::create_dir_all(path)
    }
}

/// Return a human-friendly form of `path` suitable for display (logs, error
/// messages, UI).
///
/// On Windows, this strips the `\\?\` UNC prefix when the underlying path
/// is representable without it (via the `dunce` crate). On Unix this is the
/// identity function.
pub fn display_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        windows::display_path(path)
    }
    #[cfg(not(windows))]
    {
        unix::display_path(path)
    }
}
