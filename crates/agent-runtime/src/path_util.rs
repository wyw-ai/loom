//! Platform path utilities — UNC prefix helpers and Windows process flags.
//!
//! On Windows, paths longer than 260 characters (MAX_PATH) require the `\\?\`
//! UNC prefix. These utilities are the single source of truth for UNC prefix
//! logic used across the agent-runtime crate.
//!
//! On non-Windows platforms, these are transparent no-ops.

use std::path::{Path, PathBuf};

/// Windows `CREATE_NO_WINDOW` — prevents a console window from appearing.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Windows `CREATE_BREAKAWAY_FROM_JOB` — lets the child escape a parent's job
/// object (fixes ERROR_PRIVILEGE_NOT_HELD when spawning from Tauri GUI).
#[cfg(windows)]
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;

/// Prefix a path with the Windows `\\?\` UNC prefix to bypass the 260-char
/// MAX_PATH limit. Only compiled on Windows.
///
/// UNC paths (`\\?\...`) bypass Win32 path normalization, so they must use
/// backslashes exclusively and cannot contain `.` or `..` components.
/// We normalize the path before applying the prefix.
#[cfg(windows)]
pub fn unc_prefix_path(path: PathBuf) -> PathBuf {
    let normalized = normalize_path_for_unc(path);
    // Already prefixed — don't double it.
    if normalized.starts_with(r"\\?\") {
        return PathBuf::from(normalized);
    }
    let prefixed = format!(r"\\?\{}", normalized);
    PathBuf::from(prefixed)
}

/// Normalize a PathBuf for UNC use: resolve `.` and `..` components,
/// convert `/` to `\`. Returns a string.
#[cfg(windows)]
fn normalize_path_for_unc(path: PathBuf) -> String {
    use std::path::Component;
    let mut result = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {} // skip .
            Component::ParentDir => {
                result.pop();
            }
            other => {
                result.push(other);
            }
        }
    }
    result.to_string_lossy().replace('/', "\\")
}

#[cfg(not(windows))]
pub fn unc_prefix_path(path: PathBuf) -> PathBuf {
    path
}

/// Create a directory (and all parents), applying the Windows UNC prefix
/// to bypass MAX_PATH (260 char) when the path is long.
/// Relative paths are resolved to absolute before UNC prefixing — UNC `\\?\`
/// requires an absolute path.
#[cfg(windows)]
pub fn create_dir_all_unc(path: &Path) -> std::io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let prefixed = unc_prefix_path(absolute);
    std::fs::create_dir_all(&prefixed)
}

#[cfg(not(windows))]
pub fn create_dir_all_unc(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Normalize path separators to the platform-native separator.
/// On Windows, forward slashes are replaced with backslashes.
/// On Unix, this is a no-op.
#[cfg(windows)]
pub fn normalize_path_separators(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy().replace('/', "\\");
    PathBuf::from(s)
}

#[cfg(not(windows))]
pub fn normalize_path_separators(path: PathBuf) -> PathBuf {
    path
}
