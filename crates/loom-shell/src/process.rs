//! Child process management — start/stop server.exe and daemon.exe as
//! standalone processes.
//!
//! Used in non-service mode; spawns child processes directly and tracks PIDs.
//! Includes an optional supervisor that auto-restarts processes that exit
//! unexpectedly, ensuring high availability for long-running services.

use std::collections::HashMap;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// Whether the server was intentionally stopped (don't auto-restart).
static SERVER_STOPPED_INTENTIONALLY: AtomicBool = AtomicBool::new(false);
/// Whether the daemon was intentionally stopped (don't auto-restart).
static DAEMON_STOPPED_INTENTIONALLY: AtomicBool = AtomicBool::new(false);

/// Supervisor configuration constants.
const SUPERVISOR_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const SUPERVISOR_RESTART_COOLDOWN: Duration = Duration::from_secs(10);
/// Max restarts allowed in a rolling 1-hour window before the supervisor gives
/// up.  Raised from 10 to 30 — long provider runs or network flaps can
/// legitimately cause several restarts in an hour without indicating a
/// persistent problem.
const SUPERVISOR_MAX_RESTARTS_PER_HOUR: u32 = 30;

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
                let value = rest
                    .trim_start_matches(|c: char| c == ' ' || c == '=' || c == '"')
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
    SERVER_STOPPED_INTENTIONALLY.store(false, Ordering::SeqCst);
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
    DAEMON_STOPPED_INTENTIONALLY.store(false, Ordering::SeqCst);
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
/// Marks the server as intentionally stopped so the supervisor won't restart it.
pub fn stop_server() -> Result<(), String> {
    SERVER_STOPPED_INTENTIONALLY.store(true, Ordering::SeqCst);
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
/// Marks the daemon as intentionally stopped so the supervisor won't restart it.
pub fn stop_daemon() -> Result<(), String> {
    DAEMON_STOPPED_INTENTIONALLY.store(true, Ordering::SeqCst);
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

// ---------------------------------------------------------------------------
// Supervisor — auto-restart crashed child processes
// ---------------------------------------------------------------------------

/// Track restart state for a supervised process.
struct RestartTracker {
    restart_times: Vec<Instant>,
    last_restart: Option<Instant>,
}

impl RestartTracker {
    fn new() -> Self {
        Self {
            restart_times: Vec::new(),
            last_restart: None,
        }
    }

    /// Check if we're allowed to restart now. Returns false if:
    /// - We've restarted too recently (cooldown, with exponential backoff)
    /// - We've restarted too many times in the last hour (rate limit)
    fn can_restart(&mut self) -> bool {
        let now = Instant::now();
        // Prune old entries (>1 hour)
        let one_hour = Duration::from_secs(3600);
        self.restart_times
            .retain(|t| now.duration_since(*t) < one_hour);
        let recent = self.restart_times.len() as u32;
        if recent >= SUPERVISOR_MAX_RESTARTS_PER_HOUR {
            tracing::error!(
                "supervisor: max restarts/hour ({}) exceeded with {} recent restarts — giving up",
                SUPERVISOR_MAX_RESTARTS_PER_HOUR,
                recent,
            );
            return false;
        }
        // Exponential backoff: base cooldown doubles every 5 restarts.
        // 0→10s, 5→20s, 10→40s, 15→80s, 20→160s, 25→320s.
        let backoff_multiplier = (1u32 << (recent / 5).min(6)).max(1); // capped at 64x
        let effective_cooldown = SUPERVISOR_RESTART_COOLDOWN.saturating_mul(backoff_multiplier);
        if let Some(last) = self.last_restart {
            let elapsed = now.duration_since(last);
            if elapsed < effective_cooldown {
                tracing::warn!(
                    "supervisor: restart cooldown active ({}s remaining, backoff {}s, recent {}), giving up on this cycle",
                    effective_cooldown.saturating_sub(elapsed).as_secs(),
                    effective_cooldown.as_secs(),
                    recent,
                );
                return false;
            }
        }
        true
    }

    fn record_restart(&mut self) {
        let now = Instant::now();
        self.restart_times.push(now);
        self.last_restart = Some(now);
    }
}

/// Start the process supervisor on a background thread.
///
/// The supervisor periodically checks whether the server and daemon child
/// processes are still alive. If a process that was previously running exits
/// unexpectedly (not stopped via `stop_server`/`stop_daemon`), the supervisor
/// automatically restarts it with cooldown and rate-limiting.
///
/// Call this once after loom-shell starts.
pub fn start_supervisor() {
    std::thread::spawn(move || {
        let server_exe = crate::config::server_exe().to_string_lossy().to_string();
        let daemon_exe = crate::config::daemon_exe().to_string_lossy().to_string();
        let mut server_tracker = RestartTracker::new();
        let mut daemon_tracker = RestartTracker::new();
        let mut server_was_running = false;
        let mut daemon_was_running = false;

        loop {
            std::thread::sleep(SUPERVISOR_CHECK_INTERVAL);

            // --- Server supervision ---
            let server_running = is_server_running() || is_server_port_open();
            if server_was_running && !server_running {
                if SERVER_STOPPED_INTENTIONALLY.load(Ordering::SeqCst) {
                    tracing::info!("supervisor: server stopped intentionally — not restarting");
                    SERVER_STOPPED_INTENTIONALLY.store(false, Ordering::SeqCst);
                } else if server_tracker.can_restart() {
                    tracing::warn!("supervisor: server process died unexpectedly — restarting");
                    match start_server(&server_exe) {
                        Ok(pid) => {
                            tracing::info!("supervisor: server restarted (PID {pid})");
                            server_tracker.record_restart();
                        }
                        Err(e) => {
                            tracing::error!("supervisor: failed to restart server: {e}");
                        }
                    }
                }
            }
            server_was_running = server_running;

            // --- Daemon supervision ---
            let daemon_running = is_daemon_running() || is_daemon_running_any();
            if daemon_was_running && !daemon_running {
                if DAEMON_STOPPED_INTENTIONALLY.load(Ordering::SeqCst) {
                    tracing::info!("supervisor: daemon stopped intentionally — not restarting");
                    DAEMON_STOPPED_INTENTIONALLY.store(false, Ordering::SeqCst);
                } else if daemon_tracker.can_restart() {
                    tracing::warn!("supervisor: daemon process died unexpectedly — restarting");
                    match start_daemon(&daemon_exe) {
                        Ok(pid) => {
                            tracing::info!("supervisor: daemon restarted (PID {pid})");
                            daemon_tracker.record_restart();
                        }
                        Err(e) => {
                            tracing::error!("supervisor: failed to restart daemon: {e}");
                        }
                    }
                }
            }
            daemon_was_running = daemon_running;
        }
    });
}
