//! Path resolution — locate executables, config directory, and log directory.
//!
//! All paths are resolved relative to the loom-shell.exe directory:
//! - Binaries: `<exe_dir>/loom-server.exe`, `<exe_dir>/loom-daemon.exe`
//! - Config XML: `<exe_dir>/../config/`
//! - Log files: `%LOCALAPPDATA%\loom\logs\server\loom-server.log` /
//!   `daemon\loom-daemon.log` (written by standalone processes, not via winsw)

use std::path::{Path, PathBuf};

/// Directory containing loom-shell.exe.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Full path to loom-server.exe.
pub fn server_exe() -> PathBuf {
    exe_dir().join("loom-server.exe")
}

/// Full path to loom-daemon.exe.
pub fn daemon_exe() -> PathBuf {
    exe_dir().join("loom-daemon.exe")
}

/// WinSW executable path (alongside loom-shell.exe).
pub fn winsw_exe() -> PathBuf {
    exe_dir().join("WinSW-x64.exe")
}

/// Service XML config directory (<exe_dir>/../config).
pub fn config_dir() -> PathBuf {
    exe_dir().join("..").join("config")
}

/// Log root directory (%LOCALAPPDATA%\loom\logs).
fn local_appdata() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| exe_dir().join("..").join("logs"))
}

/// Server log directory (%LOCALAPPDATA%\loom\logs\server).
pub fn server_log_dir() -> PathBuf {
    local_appdata().join("loom").join("logs").join("server")
}

/// Daemon log directory (%LOCALAPPDATA%\loom\logs\daemon).
pub fn daemon_log_dir() -> PathBuf {
    local_appdata().join("loom").join("logs").join("daemon")
}

/// Server log file path (written by standalone process, not via winsw).
pub fn server_log_file() -> PathBuf {
    server_log_dir().join("loom-server.log")
}

/// Daemon log file path (written by standalone process, not via winsw).
pub fn daemon_log_file() -> PathBuf {
    daemon_log_dir().join("loom-daemon.log")
}
