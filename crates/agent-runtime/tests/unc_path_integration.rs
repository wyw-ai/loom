//! B-2.1: UNC Path Round-Trip Integration Tests
//!
//! Validates that the Windows UNC path utilities in `agent_runtime::path_util`
//! correctly handle deep paths, mixed separators, and avoid double-prefixing.
//! All tests are gated to Windows only.

#[cfg(windows)]
use agent_runtime::path_util::{create_dir_all_unc, normalize_path_separators, unc_prefix_path};
#[cfg(windows)]
use std::io::Write;

/// AC-B2.1a, B2.1b: Create a UNC-prefixed deep directory, write a file,
/// read it back, and verify content integrity.
#[cfg(windows)]
#[test]
fn unc_path_write_and_read_roundtrip() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let base = dir.path().join("deep").join("nested").join("subdir");
    create_dir_all_unc(&base).expect("create UNC-prefixed deep dir");

    let file_path = base.join("test.txt");
    {
        let mut f = std::fs::File::create(&file_path).expect("create file");
        f.write_all(b"hello unc path roundtrip").expect("write");
    }

    let content = std::fs::read_to_string(&file_path).expect("read back");
    assert_eq!(content, "hello unc path roundtrip");
}

/// AC-B2.1c: Path with mixed \ and / separators →
/// normalize_path_separators → unc_prefix_path → verify output is all-\
/// and contains no forward slashes.
#[cfg(windows)]
#[test]
fn unc_path_mixed_separators() {
    let mixed = std::path::PathBuf::from("C:/Users/test/dir\\sub/foo");
    let normalized = normalize_path_separators(mixed);
    let normalized_str = normalized.to_string_lossy();
    // After normalization, all separators should be \
    assert!(
        !normalized_str.contains('/'),
        "should have no forward slashes: {normalized_str}"
    );

    let prefixed = unc_prefix_path(normalized);
    let prefixed_str = prefixed.to_string_lossy();
    assert!(
        prefixed_str.starts_with(r"\\?\"),
        "should start with UNC prefix: {prefixed_str}"
    );
    assert!(
        !prefixed_str.contains('/'),
        "prefixed path should have no forward slashes: {prefixed_str}"
    );
}

/// Robustness: unc_prefix_path on an already-prefixed path → no double `\\?\`.
#[cfg(windows)]
#[test]
fn unc_path_no_double_prefix() {
    let already_prefixed = std::path::PathBuf::from(r"\\?\C:\Users\test");
    let result = unc_prefix_path(already_prefixed);
    let result_str = result.to_string_lossy();
    assert!(
        result_str.starts_with(r"\\?\"),
        "should start with UNC prefix"
    );
    // Should NOT have two UNC prefixes
    let prefix_count = result_str.match_indices(r"\\?\").count();
    assert_eq!(
        prefix_count, 1,
        "should have exactly one UNC prefix, got {prefix_count}: {result_str}"
    );
}
