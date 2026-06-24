//! Unix implementation of `loom_platform::path`.
//!
//! Internal: must export the exact set of `pub` symbols listed in
//! `super::mod`'s cfg-gated `pub use`. The corresponding `windows.rs`
//! mirrors the same signatures.

use std::path::{Path, PathBuf};

pub(super) fn unc_prefix_path(path: PathBuf) -> PathBuf {
    path
}

pub(super) fn normalize_path_separators(path: PathBuf) -> PathBuf {
    path
}

pub(super) fn create_dir_all(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

pub(super) fn display_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}
