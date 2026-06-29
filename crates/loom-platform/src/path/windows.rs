//! Windows implementation of `loom_platform::path`.
//!
//! Internal: must export the exact set of `pub` symbols listed in
//! `super::mod`'s cfg-gated `pub use`. The corresponding `unix.rs` mirrors
//! the same signatures.

use std::path::{Component, Path, PathBuf};

pub(super) fn unc_prefix_path(path: PathBuf) -> PathBuf {
    let normalized = normalize_path_for_unc(path);
    if normalized.starts_with(r"\\?\") {
        return PathBuf::from(normalized);
    }
    // UNC share path (\\server\share\...) needs the verbatim UNC form
    // \\?\UNC\server\share\... — naively prepending \\?\ would yield the
    // invalid \\?\server\share\... which CreateFileW / CreateDirectoryW
    // reject (ERROR_BAD_PATHNAME).
    if let Some(rest) = normalized.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{}", rest));
    }
    PathBuf::from(format!(r"\\?\{}", normalized))
}

/// Resolve `.` / `..` components and force backslash separators.
///
/// We do this manually (not via `std::fs::canonicalize`) because the
/// purpose is to *generate* a UNC-safe representation; the file does not
/// need to exist yet (e.g. when called from `create_dir_all`).
fn normalize_path_for_unc(path: PathBuf) -> String {
    let mut result = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result.to_string_lossy().replace('/', "\\")
}

pub(super) fn normalize_path_separators(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy().replace('/', "\\");
    PathBuf::from(s)
}

pub(super) fn create_dir_all(path: &Path) -> std::io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let prefixed = unc_prefix_path(absolute);
    std::fs::create_dir_all(&prefixed)
}

/// Strip the `\\?\` UNC prefix for display when it is safe to do so.
///
/// Delegates to `dunce::simplified`, which returns the inner path unchanged
/// when the verbatim prefix is necessary (e.g. paths longer than `MAX_PATH`
/// or with non-canonical components) and strips it otherwise.
pub(super) fn display_path(path: &Path) -> PathBuf {
    dunce::simplified(path).to_path_buf()
}
