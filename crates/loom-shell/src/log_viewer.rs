//! 日志查看器 — 读取 winsw 生成的日志文件末尾若干行，在只读文本区展示。
//!
//! winsw 默认日志命名：`<service-id>.out.log` / `<service-id>.err.log`

use crate::config;
use std::fs;

/// 读取指定日志文件的最后 `n` 行。
fn tail_file(path: &std::path::Path, n: usize) -> String {
    match fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > n { lines.len() - n } else { 0 };
            lines[start..].join("\n")
        }
        Err(_) => format!("(无法读取日志: {})", path.display()),
    }
}

/// 读取 Server 日志最后 100 行（LoomServer.out.log）。
pub fn tail_server() -> String {
    tail_file(&config::server_log_dir().join("LoomServer.out.log"), 100)
}

/// 读取 Daemon 日志最后 100 行（LoomDaemon.out.log）。
pub fn tail_daemon() -> String {
    tail_file(&config::daemon_log_dir().join("LoomDaemon.out.log"), 100)
}
