//! B-2.3: Dual-Write Logging Integration Test
//!
//! Constructs a tracing subscriber that mirrors the production dual-layer
//! pattern: console (with ANSI) + file (without ANSI). Emits an info event
//! and verifies it appears in the file log, with ANSI escape sequences
//! stripped.
//!
//! Note: `tracing::subscriber::set_global_default` can only succeed once
//! per process, so B2.3a/B2.3b/B2.3d (event-in-file) and B2.3c
//! (no-ANSI-in-file) assertions are combined in one test.
//!
//! Gated to Windows per PM PRD scope, though the dual-write pattern exists
//! on all platforms.

#[cfg(windows)]
use tracing_subscriber::layer::SubscriberExt;

/// AC-B2.3a, B2.3b, B2.3c, B2.3d: Initialize a subscriber with the
/// production dual-layer pattern — stderr (ANSI on) + file (ANSI off).
/// Emit an info event containing CJK characters and verify:
/// - The event text appears in the file log (B2.3a, B2.3b, B2.3d).
/// - The file log contains NO ANSI ESC character (B2.3c).
#[cfg(windows)]
#[test]
fn dual_write_file_contains_event_and_strips_ansi() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let log_file = dir.path().join("dual_write.log");

    let log_file_clone = log_file.clone();
    let file_writer = move || {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file_clone)
            .expect("open log file")
    };

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(true)
                .with_writer(std::io::stderr),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer),
        );

    // Integration tests in tests/ each get their own binary, so
    // set_global_default will succeed exactly once.
    tracing::subscriber::set_global_default(subscriber).expect("set global default subscriber");

    tracing::info!("dual_write_test_marker_with_cjk_中文测试");

    let file_content = std::fs::read_to_string(&log_file).expect("read log file");

    // B2.3a, B2.3b, B2.3d: event text present in file output
    assert!(
        file_content.contains("dual_write_test_marker_with_cjk"),
        "file should contain test marker, got: {file_content}"
    );

    // B2.3c: no ANSI ESC character in file output
    let esc_char = '\u{1b}';
    assert!(
        !file_content.contains(esc_char),
        "file output should not contain ANSI ESC char"
    );
}
