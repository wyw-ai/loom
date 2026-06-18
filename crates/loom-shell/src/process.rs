//! Child process management — start/stop server.exe and daemon.exe as
//! standalone processes.
//!
//! Used in non-service mode; spawns child processes directly and tracks PIDs.

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

/// Managed child process handle.
static SERVER_PROCESS: Mutex<Option<Child>> = Mutex::new(None);
static DAEMON_PROCESS: Mutex<Option<Child>> = Mutex::new(None);

/// Start server.exe as a child process.
pub fn start_server(exe_path: &str) -> Result<u32, String> {
    let mut cmd = Command::new(exe_path);
    // On Windows, prevent console windows and let child escape the Tauri
    // GUI's restrictive job object (fixes ERROR_PRIVILEGE_NOT_HELD).
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start server: {e}"))?;
    let pid = child.id();
    *SERVER_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(child);
    Ok(pid)
}

/// Start loom-daemon.exe as a child process.
pub fn start_daemon(exe_path: &str) -> Result<u32, String> {
    let mut cmd = Command::new(exe_path);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start daemon: {e}"))?;
    let pid = child.id();
    *DAEMON_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(child);
    Ok(pid)
}

/// Stop the server child process (kill then wait to reap).
pub fn stop_server() -> Result<(), String> {
    let mut guard = SERVER_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(mut child) = guard.take() {
        child
            .kill()
            .map_err(|e| format!("Failed to stop server: {e}"))?;
        child.wait().ok();
    }
    Ok(())
}

/// Stop the daemon child process.
pub fn stop_daemon() -> Result<(), String> {
    let mut guard = DAEMON_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(mut child) = guard.take() {
        child
            .kill()
            .map_err(|e| format!("Failed to stop daemon: {e}"))?;
        child.wait().ok();
    }
    Ok(())
}

/// Check whether the server child process is still running.
pub fn is_server_running() -> bool {
    let mut guard = SERVER_PROCESS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(ref mut child) = *guard {
        match child.try_wait() {
            Ok(None) => true, // still running
            Ok(Some(_)) | Err(_) => {
                *guard = None;
                false
            }
        }
    } else {
        false
    }
}

/// Check whether the daemon child process is still running.
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
