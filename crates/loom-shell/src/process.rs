//! 子进程管理 — 以独立进程方式启动/停止 server.exe 和 daemon.exe。
//!
//! 非服务模式下使用，直接 spawn 子进程并跟踪 PID。

use std::process::{Child, Command};
use std::sync::Mutex;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Windows `CREATE_NO_WINDOW` — prevents a console window from appearing.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Windows `CREATE_BREAKAWAY_FROM_JOB` — lets the child escape a parent's job
/// object (fixes ERROR_PRIVILEGE_NOT_HELD when spawning from Tauri GUI).
#[cfg(windows)]
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;

/// 受管理的子进程句柄。
static SERVER_PROCESS: Mutex<Option<Child>> = Mutex::new(None);
static DAEMON_PROCESS: Mutex<Option<Child>> = Mutex::new(None);

/// 启动 server.exe 作为子进程。
pub fn start_server(exe_path: &str) -> Result<u32, String> {
    let mut cmd = Command::new(exe_path);
    // On Windows, prevent console windows and let child escape the Tauri
    // GUI's restrictive job object (fixes ERROR_PRIVILEGE_NOT_HELD).
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    let child = cmd.spawn().map_err(|e| format!("启动 server 失败: {e}"))?;
    let pid = child.id();
    *SERVER_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(child);
    Ok(pid)
}

/// 启动 loom-daemon.exe 作为子进程。
pub fn start_daemon(exe_path: &str) -> Result<u32, String> {
    let mut cmd = Command::new(exe_path);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    let child = cmd.spawn().map_err(|e| format!("启动 daemon 失败: {e}"))?;
    let pid = child.id();
    *DAEMON_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(child);
    Ok(pid)
}

/// 停止 server 子进程（kill 后 wait 回收）。
pub fn stop_server() -> Result<(), String> {
    let mut guard = SERVER_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(mut child) = guard.take() {
        child.kill().map_err(|e| format!("停止 server 失败: {e}"))?;
        child.wait().ok();
    }
    Ok(())
}

/// 停止 daemon 子进程。
pub fn stop_daemon() -> Result<(), String> {
    let mut guard = DAEMON_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(mut child) = guard.take() {
        child.kill().map_err(|e| format!("停止 daemon 失败: {e}"))?;
        child.wait().ok();
    }
    Ok(())
}

/// 检查 server 子进程是否仍在运行。
pub fn is_server_running() -> bool {
    let mut guard = SERVER_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(ref mut child) = *guard {
        match child.try_wait() {
            Ok(None) => true, // 仍在运行
            Ok(Some(_)) | Err(_) => {
                *guard = None;
                false
            }
        }
    } else {
        false
    }
}

/// 检查 daemon 子进程是否仍在运行。
pub fn is_daemon_running() -> bool {
    let mut guard = DAEMON_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(ref mut child) = *guard {
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) | Err(_) => {
                *guard = None;
                false
            }
        }
    } else {
        false
    }
}
