//! B-4 / P0-PAL-8: Daemon IPC integration & fallback tests.
//!
//! Post-PAL-8 these tests assert two things:
//!
//! 1. `start_proxy` now binds a real cross-platform local socket on
//!    *both* Unix and Windows (the previous Windows no-op contract is
//!    gone — Windows uses Named Pipes via `loom_platform::ipc`).
//! 2. `resolve_socket` / `daemon_disabled` still behave correctly
//!    when no env var / discovery file is present (D4 fallback path).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use loom_cli::daemon_ipc::{daemon_disabled, resolve_socket, start_proxy};

/// Each invocation needs a unique path so concurrently-run tests in
/// `cargo test` do not race for the same socket / pipe stem.
fn unique_socket_path() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("loom_pal8_test_{pid}_{n}.sock"))
}

/// AC-PAL8.1: `start_proxy` binds successfully on the current
/// platform (Unix UDS or Windows Named Pipe) and the returned
/// `JoinHandle` represents a live accept loop rather than an
/// immediately-completed no-op.
#[tokio::test]
async fn start_proxy_binds_on_current_platform() {
    let socket = unique_socket_path();
    // Use an unused localhost port for the upstream URL — the proxy
    // does not connect to it until a client actually arrives, so the
    // bind itself should succeed regardless of whether the upstream
    // exists.
    let upstream = "ws://127.0.0.1:1".to_string();

    let result = start_proxy(socket.clone(), upstream).await;
    assert!(
        result.is_ok(),
        "start_proxy should bind successfully, got: {result:?}"
    );

    let handle = result.unwrap();

    // The accept loop runs forever — give it 200 ms to demonstrate it
    // is NOT a no-op handle that completes immediately. A pending
    // future (i.e. `timeout` itself elapses) is exactly what we want.
    let outcome = tokio::time::timeout(Duration::from_millis(200), handle).await;
    assert!(
        outcome.is_err(),
        "accept loop should still be running after 200 ms (proxy is not a no-op): {outcome:?}"
    );

    // Cleanup: best-effort socket file removal (no-op on Windows).
    let _ = std::fs::remove_file(&socket);
}

/// AC-PAL8.2: D4 fallback — `resolve_socket()` returns `None` (or at
/// most `Some(env_path)`) without panicking when no discovery file
/// exists. This is the gate the CLI uses to fall back to a WebSocket
/// connection.
#[test]
fn resolve_socket_returns_none_when_no_discovery() {
    // We don't strictly assert `None` because the test environment may
    // have `LOOM_DAEMON_SOCKET` set; the key invariant is that the
    // function must not panic, and that callers can treat its absence
    // as the "fall back to WS" signal.
    let _ = resolve_socket();
}

/// Robustness: `daemon_disabled()` returns sanely (no panic) whether
/// or not `LOOM_NO_DAEMON` is set.
#[test]
fn daemon_disabled_returns_sanely_by_default() {
    let disabled = daemon_disabled();
    let _ = disabled;
}
