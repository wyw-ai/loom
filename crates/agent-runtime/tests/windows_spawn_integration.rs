//! B-2.2: Windows Spawn Flag Integration Tests
//!
//! Validates that the Windows process creation flags exported from
//! `loom_platform::process` match documented Win32 constants and
//! that spawning with CREATE_NO_WINDOW + CREATE_BREAKAWAY_FROM_JOB
//! succeeds without error.
//! All tests are gated to Windows only.
//!
//! ## Lint exemption
//!
//! This file uses raw `std::process::Command::new` directly. That is the
//! architecturally correct choice for a PAL primitive test: the test exists
//! precisely to verify the underlying Win32 primitives that
//! `loom_platform::process::Command` builds on. The file-level
//! `#![allow(clippy::disallowed_methods)]` below is the documented opt-out for
//! "exception #3" in the workspace-root `clippy.toml` (PAL primitive tests).
#![allow(clippy::disallowed_methods)]

#[cfg(windows)]
use loom_platform::process::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::Command;

/// AC-B2.2a, B2.2b, B2.2c: Spawn cmd.exe /c echo with CREATE_NO_WINDOW,
/// verify exit code 0 and captured stdout.
///
/// Note 1: `CREATE_BREAKAWAY_FROM_JOB` is excluded from the spawn flags
/// because some environments (CI, job-wrapping terminals) reject
/// breakaway with ACCESS_DENIED. The flag value is separately asserted
/// in `create_no_window_value_matches_api`.
///
/// Note 2: Cannot programmatically verify "no console window appeared."
/// Visual verification is a QA manual check step.
#[cfg(windows)]
#[test]
fn spawn_with_no_window_flag() {
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/c", "echo", "hello_from_no_window"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = cmd
        .spawn()
        .expect("spawn cmd.exe")
        .wait_with_output()
        .expect("wait for cmd.exe");

    assert!(
        output.status.success(),
        "cmd.exe should exit 0, got: {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello_from_no_window"),
        "stdout should contain hello_from_no_window, got: {stdout}"
    );
}

/// Robustness: Assert CREATE_NO_WINDOW == 0x08000000 and
/// CREATE_BREAKAWAY_FROM_JOB == 0x01000000 (documented Win32 constants).
#[cfg(windows)]
#[test]
fn create_no_window_value_matches_api() {
    assert_eq!(
        CREATE_NO_WINDOW, 0x08000000u32,
        "CREATE_NO_WINDOW should be 0x08000000"
    );
    assert_eq!(
        CREATE_BREAKAWAY_FROM_JOB, 0x01000000u32,
        "CREATE_BREAKAWAY_FROM_JOB should be 0x01000000"
    );
}
