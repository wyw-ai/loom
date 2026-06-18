//! 路径解析 — 定位可执行文件、配置目录和日志目录。
//!
//! 所有路径都相对于 loom-shell.exe 所在目录解析：
//! - 二进制文件：`<exe_dir>/loom-server.exe`、`<exe_dir>/loom-daemon.exe`
//! - 配置 XML：`<exe_dir>/../config/`
//! - 日志文件：`%LOCALAPPDATA%\loom\logs\server\loom-server.log` /
//!   `daemon\loom-daemon.log`（独立进程写入，不依赖 winsw）

use std::path::{Path, PathBuf};

/// 返回 loom-shell.exe 所在目录。
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// loom-server.exe 的完整路径。
pub fn server_exe() -> PathBuf {
    exe_dir().join("loom-server.exe")
}

/// loom-daemon.exe 的完整路径。
pub fn daemon_exe() -> PathBuf {
    exe_dir().join("loom-daemon.exe")
}

/// WinSW 可执行文件路径（与 loom-shell.exe 同目录）。
pub fn winsw_exe() -> PathBuf {
    exe_dir().join("WinSW-x64.exe")
}

/// 服务 XML 配置目录（<exe_dir>/../config）。
pub fn config_dir() -> PathBuf {
    exe_dir().join("..").join("config")
}

/// 日志根目录（%LOCALAPPDATA%\loom\logs）。
fn local_appdata() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| exe_dir().join("..").join("logs"))
}

/// Server 日志目录（%LOCALAPPDATA%\loom\logs\server）。
pub fn server_log_dir() -> PathBuf {
    local_appdata().join("loom").join("logs").join("server")
}

/// Daemon 日志目录（%LOCALAPPDATA%\loom\logs\daemon）。
pub fn daemon_log_dir() -> PathBuf {
    local_appdata().join("loom").join("logs").join("daemon")
}

/// Server 日志文件路径（独立进程写入，不依赖 winsw）。
pub fn server_log_file() -> PathBuf {
    server_log_dir().join("loom-server.log")
}

/// Daemon 日志文件路径（独立进程写入，不依赖 winsw）。
pub fn daemon_log_file() -> PathBuf {
    daemon_log_dir().join("loom-daemon.log")
}
