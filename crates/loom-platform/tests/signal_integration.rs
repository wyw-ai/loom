//! Integration tests for `loom_platform::signal`.
//!
//! Each test spawns a long-lived child via `loom_platform::process` and
//! verifies the corresponding `signal_child` / `force_kill_pid` call
//! actually terminates it. The spawn defaults applied by
//! `loom_platform::process::Command` (CREATE_NO_WINDOW |
//! CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP on Windows;
//! `process_group(0)` on Unix) are themselves part of the contract being
//! exercised — the signal calls below all depend on those defaults
//! holding.
//!
//! Windows note: as in `process_integration.rs`, some CI / sandbox
//! environments reject `CREATE_BREAKAWAY_FROM_JOB` with `ACCESS_DENIED`.
//! These tests reset the creation flags to `CREATE_NO_WINDOW |
//! CREATE_NEW_PROCESS_GROUP` (dropping breakaway) via `as_std_mut` before
//! spawning, so the signal contract — not the breakaway flag — is what
//! is under test.

use loom_platform::process::Command;
use loom_platform::signal::{force_kill_pid, signal_child, Signal};
use std::time::{Duration, Instant};

/// Spawn a child that idles for ~30 seconds so the test has time to
/// signal it.
fn spawn_long_lived() -> std::process::Child {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd.exe");
        // `timeout` and `ping` both block long enough; `ping` is more
        // universally present and works under headless sessions.
        c.args(["/c", "ping", "-n", "30", "127.0.0.1"]);
        use std::os::windows::process::CommandExt;
        c.as_std_mut().creation_flags(
            loom_platform::process::CREATE_NO_WINDOW
                | loom_platform::process::CREATE_NEW_PROCESS_GROUP,
        );
        c
    };

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 30"]);
        c
    };

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    cmd.spawn().expect("spawn long-lived child")
}

/// Poll `child.try_wait()` until it reports an exit or the deadline
/// passes. Returns true iff the child exited within the deadline.
fn wait_with_deadline(child: &mut std::process::Child, deadline: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return false,
        }
    }
    false
}

#[test]
fn force_kill_pid_terminates_a_spawned_child() {
    let mut child = spawn_long_lived();
    let pid = child.id();

    force_kill_pid(pid).expect("force_kill_pid should succeed for live child");

    let exited = wait_with_deadline(&mut child, Duration::from_secs(5));
    assert!(
        exited,
        "child {pid} did not exit within 5s after force_kill_pid"
    );
}

#[test]
fn signal_child_term_terminates_a_spawned_child() {
    let mut child = spawn_long_lived();
    let pid = child.id();

    signal_child(pid, Signal::Term).expect("signal_child(Term) should succeed for live child");

    let exited = wait_with_deadline(&mut child, Duration::from_secs(5));
    assert!(
        exited,
        "child {pid} did not exit within 5s after Signal::Term"
    );
}

#[test]
fn signal_child_kill_terminates_a_spawned_child() {
    let mut child = spawn_long_lived();
    let pid = child.id();

    signal_child(pid, Signal::Kill).expect("signal_child(Kill) should succeed for live child");

    let exited = wait_with_deadline(&mut child, Duration::from_secs(5));
    assert!(
        exited,
        "child {pid} did not exit within 5s after Signal::Kill"
    );
}

#[test]
fn signal_child_interrupt_terminates_a_spawned_child() {
    // On Unix this delivers SIGINT (which default-terminates `sleep`).
    // On Windows the platform first tries GenerateConsoleCtrlEvent and,
    // when the target is in our own console group, falls back to
    // TerminateProcess if that fails. Either way the child must exit.
    let mut child = spawn_long_lived();
    let pid = child.id();

    signal_child(pid, Signal::Interrupt)
        .expect("signal_child(Interrupt) should succeed for live child");

    let exited = wait_with_deadline(&mut child, Duration::from_secs(5));
    assert!(
        exited,
        "child {pid} did not exit within 5s after Signal::Interrupt"
    );
}

#[test]
fn signal_child_for_unknown_pid_returns_error() {
    // PID 0 is reserved on every platform and not a valid target. The
    // function must return an Err, not panic, and not succeed.
    let res = signal_child(0, Signal::Kill);
    assert!(res.is_err(), "expected Err for pid=0, got {res:?}");
}
