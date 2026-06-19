//! Child process management — start/stop server.exe and daemon.exe as
//! standalone processes.
//!
//! Used in non-service mode; spawns child processes directly and tracks PIDs.

use std::collections::HashMap;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::Duration;

use crate::preflight::loom_config_dir;

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

/// Path to the saved user environment snapshot (written before UAC elevation).
///
/// In admin context LOCALAPPDATA points to the system profile, so we fall
/// back to USERPROFILE\AppData\Local.
fn user_env_path() -> PathBuf {
    let candidates = [
        std::env::var("LOCALAPPDATA").ok(),
        std::env::var("USERPROFILE")
            .ok()
            .map(|up| format!("{up}\\AppData\\Local")),
    ];
    for c in candidates.iter().flatten() {
        let p = PathBuf::from(c).join("loom").join("user-env.json");
        if p.exists() {
            return p;
        }
    }
    // Fallback to LOCALAPPDATA path even if it doesn't exist yet
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("loom")
        .join("user-env.json")
}

/// Load the user environment snapshot saved before UAC elevation.
/// Returns a map of env var name → value from the user's original session.
fn load_user_env() -> HashMap<String, String> {
    let path = user_env_path();
    let Ok(data) = std::fs::read_to_string(&path) else {
        tracing::warn!(path = %path.display(), "no user-env snapshot found; daemon will use system PATH");
        return HashMap::new();
    };
    let Ok(json): Result<serde_json::Value, _> = serde_json::from_str(&data) else {
        return HashMap::new();
    };
    let mut env = HashMap::new();
    if let Some(obj) = json.as_object() {
        for (key, value) in obj {
            if let Some(val) = value.as_str() {
                if !val.is_empty() {
                    env.insert(key.clone(), val.to_string());
                }
            }
        }
    }
    tracing::info!(path = %path.display(), vars = ?env.keys().collect::<Vec<_>>(), "loaded user env snapshot");
    env
}

/// Try to read the daemon.toml machine-id so we can pass `--machine-id` to the
/// daemon child process. Returns None if daemon.toml is missing or unreadable.
pub(crate) fn machine_id_from_daemon_config() -> Option<String> {
    let config_dir = loom_config_dir()?;
    let path = config_dir.join("daemon.toml");
    let data = std::fs::read_to_string(&path).ok()?;
    // Simple TOML key-value extraction for [machine] id = "..."
    // without pulling in a full TOML parser dependency.
    let mut in_machine_section = false;
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed == "[machine]" {
            in_machine_section = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_machine_section = false;
            continue;
        }
        if in_machine_section {
            if let Some(rest) = trimmed.strip_prefix("id") {
                let value = rest.trim_start_matches(|c: char| c == ' ' || c == '=' || c == '"')
                    .trim_end_matches('"');
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

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
///
/// Injects the user-level PATH (saved before UAC elevation) so that tools
/// managed by version managers (fnm, nvm, etc.) are discoverable. Also passes
/// `--machine-id` if available from daemon.toml.
pub fn start_daemon(exe_path: &str) -> Result<u32, String> {
    let user_env = load_user_env();

    let mut cmd = Command::new(exe_path);

    // Inject user-level environment variables so the daemon can find
    // copilot CLI and other npm-global tools installed via fnm/nvm.
    for (key, value) in &user_env {
        cmd.env(key, value);
    }

    // Pass --machine-id if we can determine it from daemon.toml.
    if let Some(machine_id) = machine_id_from_daemon_config() {
        cmd.arg("--machine-id").arg(&machine_id);
    }

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

/// Check if any loom-server process is listening on the default port.
/// This detects servers started by other shell instances (not just our tracked child).
pub fn is_server_port_open() -> bool {
    TcpStream::connect_timeout(
        &"127.0.0.1:7878".parse().unwrap(),
        Duration::from_millis(500),
    )
    .is_ok()
}

/// Find any running loom-daemon.exe process by scanning the process list.
/// This detects daemons started by other shell instances.
pub fn is_daemon_running_any() -> bool {
    #[cfg(windows)]
    {
        if let Ok(output) = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq loom-daemon.exe", "/FO", "CSV", "/NH"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout.contains("loom-daemon.exe");
        }
    }
    is_daemon_running()
}
