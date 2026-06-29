//! Integration tests for `loom_platform::path`.
//!
//! Mirrors the behaviour previously covered by
//! `agent-runtime/tests/unc_path_integration.rs` and adds a
//! `display_path` regression to lock in the dunce-based UNC stripping.

use loom_platform::path::{
    create_dir_all, display_path, normalize_path_separators, unc_prefix_path,
};
use std::path::PathBuf;

#[test]
fn create_dir_all_handles_deep_paths() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut deep = tmp.path().to_path_buf();
    for segment in 0..16 {
        deep.push(format!("loom-platform-path-test-segment-{segment:02}"));
    }
    create_dir_all(&deep).expect("create deep dir");
    assert!(deep.is_dir(), "deep dir not created: {}", deep.display());
}

#[test]
fn normalize_then_unc_is_idempotent() {
    #[cfg(windows)]
    {
        let mixed = PathBuf::from("C:/Users/loom/test/dir");
        let normalized = normalize_path_separators(mixed);
        assert_eq!(normalized, PathBuf::from(r"C:\Users\loom\test\dir"));
        let prefixed = unc_prefix_path(normalized);
        let s = prefixed.to_string_lossy();
        assert!(s.starts_with(r"\\?\"), "missing UNC prefix: {s}");
        assert!(!s.contains('/'), "forward slash leaked: {s}");

        // Idempotence: re-prefixing an already-prefixed path does not double it.
        let again = unc_prefix_path(prefixed.clone());
        let s2 = again.to_string_lossy();
        assert_eq!(s, s2, "UNC prefix doubled");
    }
    #[cfg(not(windows))]
    {
        // On Unix both functions are identity.
        let p = PathBuf::from("/tmp/loom/test");
        assert_eq!(normalize_path_separators(p.clone()), p);
        assert_eq!(unc_prefix_path(p.clone()), p);
    }
}

#[test]
fn display_path_strips_safe_unc_prefix() {
    #[cfg(windows)]
    {
        // A short, well-formed absolute path can be displayed without
        // the verbatim prefix.
        let prefixed = PathBuf::from(r"\\?\C:\Windows");
        let displayed = display_path(&prefixed);
        assert_eq!(
            displayed,
            PathBuf::from(r"C:\Windows"),
            "dunce did not strip the UNC prefix for a short canonical path"
        );
    }
    #[cfg(not(windows))]
    {
        let p = PathBuf::from("/usr/local/bin");
        assert_eq!(display_path(&p), p);
    }
}
