//! 日志查看器 — 读取日志文件末尾若干行，在只读文本区展示。

use crate::config;
use std::fs;

/// 读取指定日志文件的最后 `n` 行。
pub fn tail(log_name: &str, n: usize) -> String {
    let path = config::log_dir().join(log_name);
    match fs::read_to_string(&path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = if lines.len() > n { lines.len() - n } else { 0 };
            lines[start..].join("\n")
        }
        Err(_) => format!("(无法读取日志: {})", path.display()),
    }
}

/// 读取 server 日志最后 100 行。
pub fn tail_server() -> String {
    tail("server.log", 100)
}

/// 读取 daemon 日志最后 100 行。
pub fn tail_daemon() -> String {
    tail("daemon.log", 100)
}
