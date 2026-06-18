//! 日志查看器 — 读取独立进程写入的日志文件末尾若干行，在只读文本区展示。
//!
//! 日志由 daemon/server 的 tracing-appender 写入（非 winsw）：
//! - `loom-server.log`（滚动，每天一个新文件）
//! - `loom-daemon.log`（滚动，每天一个新文件）

use crate::config;
use std::fs;

/// Strip ANSI escape sequences (CSI, OSC, and other ESC variants).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                let _ = chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() { break; }
                }
            }
            Some(']') => {
                let _ = chars.next();
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if c == '\u{07}' || (prev == '\u{1b}' && c == '\\') { break; }
                    prev = c;
                }
            }
            Some(c) if "PX_^".contains(c) => {
                let _ = chars.next();
                let mut prev = '\0';
                for c2 in chars.by_ref() {
                    if c2 == '\u{07}' || (prev == '\u{1b}' && c2 == '\\') { break; }
                    prev = c2;
                }
            }
            _ => {}
        }
    }
    out
}

/// 读取指定日志文件的最后 `n` 行。
fn tail_file(path: &std::path::Path, n: usize) -> String {
    match fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > n { lines.len() - n } else { 0 };
            strip_ansi(&lines[start..].join("\r\n"))
        }
        Err(_) => format!("(无法读取日志: {})", path.display()),
    }
}

/// 读取 Server 日志最后 100 行（loom-server.log）。
pub fn tail_server() -> String {
    tail_file(&config::server_log_file(), 100)
}

/// 读取 Daemon 日志最后 100 行（loom-daemon.log）。
pub fn tail_daemon() -> String {
    tail_file(&config::daemon_log_file(), 100)
}
