//! Integration tests for `loom_platform::process`.
//!
//! Note on Windows spawn tests: production defaults include
//! `CREATE_BREAKAWAY_FROM_JOB`, but some sandboxed/job-wrapped CI
//! environments reject breakaway with `ACCESS_DENIED` (same constraint
//! that gates `agent_runtime::tests::windows_spawn_integration`). The
//! spawn tests therefore reset the creation flags to `CREATE_NO_WINDOW`
//! only via the `as_std_mut` / `as_tokio_mut` escape hatches before
//! spawning. The full-flag value is separately asserted in
//! `windows_spawn_flag_constants_match_win32_api`.

use loom_platform::process::{Command, TokioCommand};

#[test]
fn command_new_executes_a_simple_program() {
    #[cfg(windows)]
    let mut cmd = Command::new("cmd.exe");
    #[cfg(windows)]
    {
        cmd.args(["/c", "echo", "loom_platform_process_ok"]);
        use std::os::windows::process::CommandExt;
        cmd.as_std_mut()
            .creation_flags(loom_platform::process::CREATE_NO_WINDOW);
    }

    #[cfg(not(windows))]
    let mut cmd = Command::new("sh");
    #[cfg(not(windows))]
    cmd.args(["-c", "echo loom_platform_process_ok"]);

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = cmd
        .spawn()
        .expect("spawn child")
        .wait_with_output()
        .expect("wait_with_output");

    assert!(
        output.status.success(),
        "child exited non-zero: {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("loom_platform_process_ok"),
        "stdout missing marker: {stdout}"
    );
}

#[test]
fn command_builder_methods_chain() {
    let mut cmd = Command::new("noop");
    cmd.arg("a")
        .args(["b", "c"])
        .env("LOOM_PLATFORM_TEST", "1")
        .env_remove("LOOM_PLATFORM_TEST")
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    drop(cmd);
}

#[tokio::test]
async fn tokio_command_new_executes_a_simple_program() {
    #[cfg(windows)]
    let mut cmd = TokioCommand::new("cmd.exe");
    #[cfg(windows)]
    {
        cmd.args(["/c", "echo", "loom_platform_tokio_ok"]);
        cmd.as_tokio_mut()
            .creation_flags(loom_platform::process::CREATE_NO_WINDOW);
    }

    #[cfg(not(windows))]
    let mut cmd = TokioCommand::new("sh");
    #[cfg(not(windows))]
    cmd.args(["-c", "echo loom_platform_tokio_ok"]);

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let child = cmd.spawn().expect("spawn child");
    let output = child.wait_with_output().await.expect("wait_with_output");
    assert!(
        output.status.success(),
        "child exited non-zero: {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("loom_platform_tokio_ok"),
        "stdout missing marker: {stdout}"
    );
}

#[cfg(windows)]
#[test]
fn windows_spawn_flag_constants_match_win32_api() {
    use loom_platform::process::{
        CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
    };
    assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    assert_eq!(CREATE_BREAKAWAY_FROM_JOB, 0x0100_0000);
    assert_eq!(CREATE_NEW_PROCESS_GROUP, 0x0000_0200);
}
