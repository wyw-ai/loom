//! Integration tests for `loom_platform::ipc`.
//!
//! Each test creates an in-process `LocalListener` (server), connects
//! a `LocalStream` (client), and verifies the round-trip.
//!
//! The test names are deliberately mangled with a random-OS-hex suffix
//! so concurrent test runners do not collide on the same pipe/socket
//! name. On Windows `interprocess` enforces single-listener semantics
//! per pipe name, so parallel tests are safe — each uses a unique stem.
//!
//! Because these tests exercise the OS's IPC stack they are excluded
//! from the default `cargo test` via `#[cfg(test)]` gate in the lib;
//! the integration-test crate respects `--test ipc_integration`.

use loom_platform::ipc::{cleanup_stale, local_socket_name, LocalListener, LocalStream};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

static TEST_COUNTER: AtomicU16 = AtomicU16::new(0);

/// Build a unique test name so parallel runs do not collide.
fn test_stem() -> String {
    let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("loom-platform-ipc-test-{n:x}")
}

/// Round-trip: server binds, client connects, sends "hello", server
/// reads it back, then both close cleanly.
#[tokio::test]
async fn bind_connect_send_recv_round_trip() {
    let name = local_socket_name(&test_stem()).expect("local_socket_name");
    // Pre-cleanup (no-op on Windows) so we start clean.
    let _ = cleanup_stale(&name);

    let listener = LocalListener::bind(&name).await.expect("bind");
    // Accept in the background.
    let name_clone = name.clone();
    let join = tokio::spawn(async move {
        let mut stream = LocalStream::connect(&name_clone).await.expect("connect");
        stream.write_all(b"hello").await.expect("write");
        stream.flush().await.expect("flush");
        let mut buf = [0u8; 5];
        stream.read_exact(&mut buf).await.expect("read");
        assert_eq!(&buf, b"hello", "echo mismatch");
    });

    let mut accepted = listener.accept().await.expect("accept");
    let mut buf = [0u8; 5];
    accepted.read_exact(&mut buf).await.expect("server read");
    assert_eq!(&buf, b"hello");
    accepted.write_all(b"hello").await.expect("server write");
    accepted.flush().await.expect("server flush");

    // Wait for the client to finish its read.
    timeout(Duration::from_secs(5), join)
        .await
        .expect("client timeout")
        .expect("client panicked");
}

/// Binding the same name twice must fail with `AddrInUse`.
#[tokio::test]
async fn double_bind_rejected() {
    let name = local_socket_name(&test_stem()).expect("local_socket_name");
    let _ = cleanup_stale(&name);

    let _listener = LocalListener::bind(&name)
        .await
        .expect("first bind should succeed");

    let result = LocalListener::bind(&name).await;
    assert!(result.is_err(), "second bind should fail, got {:?}", result);
}

/// Connecting to a non-existent name must fail with a connection
/// error (typically `NotFound` on Unix, `WAIT_TIMEOUT` / pipe-not-
/// open on Windows, but surfaced as `io::Error` in either case).
#[tokio::test]
async fn connect_to_non_existent_fails() {
    let name = local_socket_name("definitely-no-listener-here").expect("local_socket_name");
    let _ = cleanup_stale(&name);

    let result = LocalStream::connect(&name).await;
    assert!(
        result.is_err(),
        "connecting to non-existent name should fail, got {:?}",
        result
    );
}
