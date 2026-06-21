//! Path utilities — UNC prefix, normalisation, `create_dir_all` wrapper.
//!
//! Skeleton (P0-PAL-1). Real implementation lands in P0-PAL-2:
//! `agent-runtime::path_util` is migrated here verbatim and then deleted,
//! with a `#[deprecated]` re-export bridge in `agent-runtime` during the
//! transition.
//!
//! Public API will be:
//!
//! ```ignore
//! pub fn unc_prefix_path(path: PathBuf) -> PathBuf;
//! pub fn normalize_path_separators(path: PathBuf) -> PathBuf;
//! pub fn create_dir_all(path: &Path) -> std::io::Result<()>;
//! ```
//!
//! Internal split: `windows.rs` (with `dunce`) + `unix.rs` (transparent).
