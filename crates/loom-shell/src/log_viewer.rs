//! Log viewer — read the last N lines of standalone process log files and
//! display them in a read-only text area.
//!
//! Logs are written by the daemon/server tracing-appender (not winsw):
//! - `loom-server.log` (rotating, one file per day)
//! - `loom-daemon.log` (rotating, one file per day)

use crate::config;
use std::fs;

use proto::ansi::strip_ansi;

/// Read the last `n` lines of the specified log file.
fn tail_file(path: &std::path::Path, n: usize) -> String {
    match fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > n { lines.len() - n } else { 0 };
            strip_ansi(&lines[start..].join("\r\n"))
        }
        Err(_) => format!("(Failed to read log: {})", path.display()),
    }
}

/// Read last 100 lines of server log (loom-server.log).
pub fn tail_server() -> String {
    tail_file(&config::server_log_file(), 100)
}

/// Read last 100 lines of daemon log (loom-daemon.log).
pub fn tail_daemon() -> String {
    tail_file(&config::daemon_log_file(), 100)
}
