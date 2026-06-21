//! Out-of-band attention signals for the chat TUI: a terminal bell and a
//! best-effort desktop notification. Both are spawn-and-forget — the chat
//! event loop must never block on them, so failures are swallowed and only
//! traced at debug level.
//!
//! No external crates: we shell out to `osascript` on macOS and `notify-send`
//! on Linux. On any other platform the desktop notifier is a no-op.

use std::io::Write;
use std::process::Stdio;

use loom_platform::process::Command;

/// Write the BEL character (`\x07`) to stderr. Most terminals honor this as a
/// short audible/visible alert; alternate-screen mode (which the TUI uses)
/// does not suppress it. Stderr — not stdout — because ratatui owns stdout.
pub fn ring_terminal_bell() {
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(b"\x07");
    let _ = stderr.flush();
}

/// Best-effort desktop notification. Returns immediately; the spawned process
/// runs detached and is never awaited. Failures (binary missing, malformed
/// args) are swallowed.
pub fn desktop_notify(title: &str, body: &str) {
    let title = sanitize(title);
    let body = sanitize(body);

    if cfg!(target_os = "macos") {
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            escape_dq(&body),
            escape_dq(&title),
        );
        let mut cmd = Command::new("osascript");
        cmd.arg("-e").arg(script);
        spawn_detached(&mut cmd);
    } else if cfg!(target_os = "linux") {
        let mut cmd = Command::new("notify-send");
        cmd.arg(&title).arg(&body);
        spawn_detached(&mut cmd);
    }
    // Other platforms: no-op.
}

fn spawn_detached(cmd: &mut Command) {
    let res = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(e) = res {
        tracing::debug!(error = %e, "desktop_notify spawn failed");
    }
}

/// Strip control characters that would either confuse `osascript` parsing or
/// crash through the terminal alongside the notification. Newlines collapse
/// to spaces — most notifiers render single-line anyway.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .filter(|c| !c.is_control())
        .collect()
}

fn escape_dq(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{escape_dq, sanitize};

    #[test]
    fn sanitize_strips_controls_and_collapses_newlines() {
        assert_eq!(sanitize("hello\nworld\u{0007}"), "hello world");
    }

    #[test]
    fn escape_dq_protects_quotes_and_backslashes() {
        assert_eq!(escape_dq(r#"he said "hi" \ ok"#), r#"he said \"hi\" \\ ok"#);
    }
}
