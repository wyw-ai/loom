//! Pre-flight diagnostics — run before starting the daemon to detect common
//! configuration issues (missing copilot CLI, broken PATH, missing machine-id).
//!
//! The copilot check is two-phase:
//!   1. Resolve the copilot binary full path (across system PATH, saved user
//!      PATH, and common npm global directories).
//!   2. Smoke-test the binary with `copilot --version` (timeout 5 s, purely
//!      local — no token spend, no network).  If it fails, the full stderr
//!      and exit code are written to the daemon log and shown to the user.
//!
//! Only when the smoke test passes will the check return Ok, giving confidence
//! that the daemon's agents can actually launch copilot subprocesses.

use loom_platform::process::Command;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Max wall-clock time for the `copilot --version` smoke test.
const SMOKE_TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of a single pre-flight check.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub label: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Error,
}

impl CheckStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            CheckStatus::Ok => "[OK]",
            CheckStatus::Warn => "[WARN]",
            CheckStatus::Error => "[FAIL]",
        }
    }
}

/// Run all pre-flight checks and return results.
///
/// Diagnostics are also appended to the daemon log so they survive across
/// restarts for post-mortem analysis.
pub fn run_checks() -> Vec<CheckResult> {
    let results = vec![
        check_copilot_cli(),
        check_daemon_config(),
        check_user_env_snapshot(),
        check_disk_space(),
        check_data_dir_writable(),
        check_webview2(),
        check_server_port(),
    ];

    // Persist diagnostics to the daemon log for high-availability visibility.
    write_diagnostics_to_log(&results);

    results
}

// ---------------------------------------------------------------------------
// Copilot CLI — resolve + smoke test
// ---------------------------------------------------------------------------

/// Two-phase copilot check: resolve the binary, then smoke-test it.
fn check_copilot_cli() -> CheckResult {
    let candidates = ["copilot", "copilotcli"];

    // Phase 1 — resolve the full binary path.
    let (binary_path, source) = match resolve_copilot_binary(&candidates) {
        Some(pair) => pair,
        None => {
            let detail = build_not_found_detail();
            return CheckResult {
                label: "copilot CLI",
                status: CheckStatus::Error,
                detail,
            };
        }
    };

    // Phase 2 — smoke test (copilot --version, no token spend).
    match smoke_test_copilot(&binary_path) {
        SmokeResult::Ok { version } => CheckResult {
            label: "copilot CLI",
            status: CheckStatus::Ok,
            detail: format!("{version}  ({source})"),
        },
        SmokeResult::Timeout => CheckResult {
            label: "copilot CLI",
            status: CheckStatus::Error,
            detail: format!(
                "smoke test timed out after {}s\n\
                 binary: {}\n\
                 The copilot process hung — check for antivirus blocking or\n\
                 corrupted installation. Try reinstalling with:\n\
                   npm install -g @github/copilot-cli",
                SMOKE_TEST_TIMEOUT.as_secs(),
                binary_path.display(),
            ),
        },
        SmokeResult::Failed { exit_code, stderr } => CheckResult {
            label: "copilot CLI",
            status: CheckStatus::Error,
            detail: format!(
                "smoke test failed (exit code {exit_code})\n\
                 binary: {}\n\
                 stderr: {stderr}\n\
                 The copilot binary was found but cannot execute — it may be\n\
                 a stale shim from a removed fnm version. Run:\n\
                   fnm use <version>\n\
                   npm install -g @github/copilot-cli",
                binary_path.display(),
            ),
        },
        SmokeResult::SpawnFailed { error } => CheckResult {
            label: "copilot CLI",
            status: CheckStatus::Error,
            detail: format!(
                "cannot spawn copilot process\n\
                 binary: {}\n\
                 error: {error}\n\
                 The file exists but the OS cannot launch it. Check file\n\
                 permissions and antivirus settings.",
                binary_path.display(),
            ),
        },
    }
}

/// Resolve the full path to a copilot binary.
///
/// Search order:
///   1. Current (system/admin) PATH
///   2. Saved user-env PATH (captured before UAC elevation)
///   3. Common npm global directories (including fnm defaults)
///
/// Returns `(full_path, source_description)`.
fn resolve_copilot_binary(candidates: &[&str]) -> Option<(PathBuf, String)> {
    // 1. System PATH
    let sys_path = std::env::var_os("PATH").unwrap_or_default();
    for name in candidates {
        if let Some(p) = resolve_in_path(name, &sys_path) {
            return Some((p, "system PATH".into()));
        }
    }

    // 2. Saved user-env PATH
    if let Some(user_env) = load_user_env_snapshot() {
        if let Some(user_path) = user_env.get("PATH") {
            let ospath = std::ffi::OsString::from(user_path);
            for name in candidates {
                if let Some(p) = resolve_in_path(name, &ospath) {
                    return Some((p, "saved user PATH".into()));
                }
            }
        }
    }

    // 3. Common npm global directories — try all PATHEXT + .ps1 extensions
    for dir in npm_global_dirs() {
        for name in candidates {
            for ext in &[".exe", ".cmd", ".bat", ".ps1"] {
                let p = dir.join(format!("{name}{ext}"));
                if p.is_file() {
                    return Some((p, format!("fallback {}", dir.display())));
                }
            }
        }
    }

    None
}

/// Build the "not found" diagnostic message.
fn build_not_found_detail() -> String {
    format!(
        "copilot/copilotcli not found in any search path.\n\
         Searched: system PATH + user-env PATH + npm global dirs\n\
         Action: npm install -g @github/copilot-cli\n\
         Then restart loom-shell to capture the new PATH."
    )
}

// ---------------------------------------------------------------------------
// Smoke test
// ---------------------------------------------------------------------------

enum SmokeResult {
    Ok { version: String },
    Timeout,
    Failed { exit_code: i32, stderr: String },
    SpawnFailed { error: String },
}

/// Run `copilot --version` with a 5-second timeout.
///
/// `--version` is purely local — it prints the version banner and exits
/// immediately. No token is consumed, no network request is made.
fn smoke_test_copilot(binary: &Path) -> SmokeResult {
    let mut cmd = Command::new(binary);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return SmokeResult::SpawnFailed {
                error: e.to_string(),
            };
        }
    };

    let start = Instant::now();

    // Busy-wait with 100 ms polling up to the timeout.
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                // Best-effort read; pipes are small, this won't block.
                if let Some(ref mut pipe) = child.stdout {
                    let _ = std::io::Read::read_to_string(pipe, &mut stdout);
                }
                if let Some(ref mut pipe) = child.stderr {
                    let _ = std::io::Read::read_to_string(pipe, &mut stderr);
                }

                if status.success() {
                    let version = stdout.trim().to_string();
                    if version.is_empty() {
                        return SmokeResult::Failed {
                            exit_code: status.code().unwrap_or(-1),
                            stderr: "no output on stdout".into(),
                        };
                    }
                    return SmokeResult::Ok { version };
                }

                let combined = if stdout.is_empty() {
                    stderr.trim().to_string()
                } else if stderr.is_empty() {
                    stdout.trim().to_string()
                } else {
                    format!("{}\n{}", stdout.trim(), stderr.trim())
                };
                return SmokeResult::Failed {
                    exit_code: status.code().unwrap_or(-1),
                    stderr: combined,
                };
            }
            Ok(None) => {
                if start.elapsed() > SMOKE_TEST_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return SmokeResult::Timeout;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return SmokeResult::SpawnFailed {
                    error: e.to_string(),
                };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// daemon.toml check
// ---------------------------------------------------------------------------

fn check_daemon_config() -> CheckResult {
    let config_dir = loom_config_dir();

    let Some(config_dir) = config_dir else {
        return CheckResult {
            label: "daemon.toml config",
            status: CheckStatus::Error,
            detail: "Cannot determine LOOM config directory — HOME, LOOM_CONFIG_DIR, and USERPROFILE are not set".to_string(),
        };
    };

    let path = config_dir.join("daemon.toml");
    if !path.exists() {
        return CheckResult {
            label: "daemon.toml config",
            status: CheckStatus::Warn,
            detail: format!(
                "{} not found — will be auto-created on first daemon start",
                path.display()
            ),
        };
    }

    let Ok(data) = std::fs::read_to_string(&path) else {
        return CheckResult {
            label: "daemon.toml config",
            status: CheckStatus::Error,
            detail: format!("{} exists but cannot be read", path.display()),
        };
    };

    let mut in_machine = false;
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed == "[machine]" {
            in_machine = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_machine = false;
            continue;
        }
        if in_machine {
            if let Some(rest) = trimmed.strip_prefix("id") {
                let value = rest
                    .trim_start_matches(|c: char| c == ' ' || c == '=' || c == '"')
                    .trim_end_matches('"');
                if !value.is_empty() {
                    return CheckResult {
                        label: "daemon.toml config",
                        status: CheckStatus::Ok,
                        detail: format!("machine-id = \"{value}\" at {}", path.display()),
                    };
                }
            }
        }
    }

    CheckResult {
        label: "daemon.toml config",
        status: CheckStatus::Warn,
        detail: format!(
            "{} exists but has no [machine] id — daemon will use defaults",
            path.display()
        ),
    }
}

// ---------------------------------------------------------------------------
// User env snapshot check
// ---------------------------------------------------------------------------

fn check_user_env_snapshot() -> CheckResult {
    match load_user_env_snapshot() {
        Some(env) => {
            let has_path = env.contains_key("PATH");
            let has_home = env.contains_key("HOME");
            CheckResult {
                label: "user env snapshot",
                status: CheckStatus::Ok,
                detail: format!(
                    "saved before elevation: PATH={}, HOME={}",
                    if has_path { "yes" } else { "no" },
                    if has_home { "yes" } else { "no" },
                ),
            }
        }
        None => CheckResult {
            label: "user env snapshot",
            status: CheckStatus::Warn,
            detail: "no user-env.json — normal if loom-shell was not elevated by UAC".to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Disk space check
// ---------------------------------------------------------------------------

/// Minimum free disk space (100 MB) before warning.
const MIN_FREE_DISK_BYTES: u64 = 100 * 1024 * 1024;

/// Check that the data directory has adequate free disk space.
///
/// The loom data directory stores SQLite journals, agent workspaces, and
/// logs. Running out of disk space can corrupt the database mid-write.
fn check_disk_space() -> CheckResult {
    let data_dir = loom_data_dir();
    let free = match free_disk_space_bytes(&data_dir) {
        Some(bytes) => bytes,
        None => {
            return CheckResult {
                label: "disk space",
                status: CheckStatus::Warn,
                detail: format!("cannot query free space on {}", data_dir.display(),),
            };
        }
    };

    if free < MIN_FREE_DISK_BYTES {
        let free_mb = free / (1024 * 1024);
        CheckResult {
            label: "disk space",
            status: CheckStatus::Warn,
            detail: format!(
                "low disk space: {free_mb} MB free on {} — minimum recommended is 100 MB.\n\
                 Running out of space can corrupt SQLite journals mid-write.",
                data_dir.display(),
            ),
        }
    } else {
        let free_mb = free / (1024 * 1024);
        CheckResult {
            label: "disk space",
            status: CheckStatus::Ok,
            detail: format!("{free_mb} MB free on {}", data_dir.display()),
        }
    }
}

/// Query free disk space (in bytes) for the volume containing `path`
/// using the Windows `GetDiskFreeSpaceExW` API.
fn free_disk_space_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // Get the root of the volume (e.g. "C:\")
    let path_str = path.to_string_lossy();
    let root = if path_str.len() >= 2 && path_str.as_bytes()[1] == b':' {
        format!("{}:\\", &path_str[..1])
    } else {
        // Fallback: use the current directory's root
        let cwd = std::env::current_dir().ok()?;
        let cwd_str = cwd.to_string_lossy();
        if cwd_str.len() >= 2 && cwd_str.as_bytes()[1] == b':' {
            format!("{}:\\", &cwd_str[..1])
        } else {
            "C:\\".to_string()
        }
    };

    let root_wide: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut free_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free_bytes: u64 = 0;

    let ok = unsafe {
        GetDiskFreeSpaceExW(
            root_wide.as_ptr(),
            &mut free_bytes,
            &mut total_bytes,
            &mut total_free_bytes,
        )
    };

    if ok != 0 {
        // `free_bytes` is the free space available to the calling user
        // (accounts for quotas). `total_free_bytes` is total free on the
        // volume. We report the user-available free space.
        Some(free_bytes)
    } else {
        None
    }
}

/// Return the loom data directory used for journals, workspaces, and logs.
fn loom_data_dir() -> PathBuf {
    std::env::var_os("LOOM_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|appdata| PathBuf::from(appdata).join("loom"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Data directory writability check
// ---------------------------------------------------------------------------

/// Check that the loom data directory is writable.
///
/// The daemon and agents write journal entries, workspace files, and
/// session state to this directory. A read-only directory blocks all
/// agent activity.
fn check_data_dir_writable() -> CheckResult {
    let data_dir = loom_data_dir();

    // Ensure the directory exists before testing writability.
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        return CheckResult {
            label: "data dir writable",
            status: CheckStatus::Error,
            detail: format!("cannot create data directory {}: {e}", data_dir.display(),),
        };
    }

    // Write a probe file and clean it up.
    let probe = data_dir.join(".preflight-write-test");
    match std::fs::write(&probe, b"loom preflight probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            CheckResult {
                label: "data dir writable",
                status: CheckStatus::Ok,
                detail: format!("{} is writable", data_dir.display()),
            }
        }
        Err(e) => CheckResult {
            label: "data dir writable",
            status: CheckStatus::Error,
            detail: format!(
                "{} is not writable: {e}\n\
                 Check filesystem permissions or antivirus blocking.",
                data_dir.display(),
            ),
        },
    }
}

// ---------------------------------------------------------------------------
// WebView2 runtime check
// ---------------------------------------------------------------------------

/// Check whether the Microsoft Edge WebView2 runtime is installed.
///
/// The Tauri GUI shell depends on WebView2 for rendering the desktop
/// interface. Without it the GUI will fail to launch.
fn check_webview2() -> CheckResult {
    // WebView2 Evergreen installer key (machine-wide).
    // Per-user installs use HKCU instead, but the Evergreen bootstrapper
    // typically installs to HKLM.
    let key_path = r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let value_name = "pv"; // product version

    if let Some(version) = read_registry_string(key_path, value_name) {
        return CheckResult {
            label: "WebView2 runtime",
            status: CheckStatus::Ok,
            detail: format!("installed (v{version})"),
        };
    }

    // Try per-user install location as fallback.
    let per_user_key =
        r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    if let Some(version) = read_registry_string_hkcu(per_user_key, value_name) {
        return CheckResult {
            label: "WebView2 runtime",
            status: CheckStatus::Ok,
            detail: format!("installed per-user (v{version})"),
        };
    }

    CheckResult {
        label: "WebView2 runtime",
        status: CheckStatus::Warn,
        detail: "WebView2 runtime not detected — the Tauri GUI will fail to launch.\n\
                 Install from: https://developer.microsoft.com/microsoft-edge/webview2/"
            .to_string(),
    }
}

/// Read a string value from HKLM registry.
fn read_registry_string(key_path: &str, value_name: &str) -> Option<String> {
    read_registry_string_impl(
        windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
        key_path,
        value_name,
    )
}

/// Read a string value from HKCU registry.
fn read_registry_string_hkcu(key_path: &str, value_name: &str) -> Option<String> {
    read_registry_string_impl(
        windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
        key_path,
        value_name,
    )
}

fn read_registry_string_impl(
    hkey_root: windows_sys::Win32::System::Registry::HKEY,
    key_path: &str,
    value_name: &str,
) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, KEY_READ, REG_SZ,
    };

    let subkey: Vec<u16> = std::ffi::OsStr::new(key_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let val: Vec<u16> = std::ffi::OsStr::new(value_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(hkey_root, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if status != 0 {
        return None;
    }

    let mut data_type: u32 = 0;
    let mut data_size: u32 = 0;
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            val.as_ptr(),
            std::ptr::null_mut(),
            &mut data_type,
            std::ptr::null_mut(),
            &mut data_size,
        )
    };
    if status != 0 || data_type != REG_SZ {
        unsafe { RegCloseKey(hkey) };
        return None;
    }

    let byte_len = (data_size / 2) as usize;
    let mut buf: Vec<u16> = vec![0u16; byte_len];
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            val.as_ptr(),
            std::ptr::null_mut(),
            &mut data_type,
            buf.as_mut_ptr() as *mut u8,
            &mut data_size,
        )
    };
    unsafe { RegCloseKey(hkey) };
    if status != 0 {
        return None;
    }

    let raw = OsString::from_wide(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
    Some(raw.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// Server port check
// ---------------------------------------------------------------------------

/// Check whether the default loom-server port (7878) is already in use.
///
/// A port conflict means another loom-server instance is already running,
/// or another application is occupying the port. This is informational —
/// the user may want to stop the existing instance first.
fn check_server_port() -> CheckResult {
    use std::net::TcpStream;

    match TcpStream::connect_timeout(
        &"127.0.0.1:7878".parse().unwrap(),
        Duration::from_millis(500),
    ) {
        Ok(_) => CheckResult {
            label: "server port 7878",
            status: CheckStatus::Ok,
            detail: "port 7878 is reachable — a loom-server instance appears to be running"
                .to_string(),
        },
        Err(_) => CheckResult {
            label: "server port 7878",
            status: CheckStatus::Ok,
            detail: "port 7878 is free — no conflicting server detected".to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the loom config directory (where daemon.toml lives).
///
/// Priority: `LOOM_CONFIG_DIR` → `HOME/.loom` → `USERPROFILE/.loom` (Windows)
pub(crate) fn loom_config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("LOOM_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home).join(".loom"));
    }
    // Windows admin context: HOME may be empty, fall back to USERPROFILE
    if let Some(up) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(up).join(".loom"));
    }
    None
}

/// Load the saved user-env.json snapshot.
///
/// In an elevated (admin) context, `LOCALAPPDATA` points to the system profile
/// instead of the user's. We try multiple fallbacks to find the real file.
fn load_user_env_snapshot() -> Option<HashMap<String, String>> {
    let candidates = user_env_json_candidates();
    for path in &candidates {
        let Ok(data) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) else {
            continue;
        };
        let mut map = HashMap::new();
        if let Some(obj) = json.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    if !s.is_empty() {
                        map.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
        return Some(map);
    }
    None
}

/// Candidate paths for user-env.json, ordered by priority.
fn user_env_json_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    // 1. LOCALAPPDATA (correct for non-elevated, wrong for admin)
    if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
        v.push(PathBuf::from(&appdata).join("loom").join("user-env.json"));
    }
    // 2. USERPROFILE\AppData\Local (correct for admin context)
    if let Ok(up) = std::env::var("USERPROFILE") {
        v.push(
            PathBuf::from(&up)
                .join("AppData")
                .join("Local")
                .join("loom")
                .join("user-env.json"),
        );
    }
    v
}

/// Resolve a command name to its full path within a PATH string.
///
/// Tries PATHEXT extensions first (.exe, .cmd, .bat, .ps1, etc.), then the
/// bare name. fnm on Windows creates `.ps1` shims in per-shell directories
/// and `.cmd` shims in the node-versions installation directory.
fn resolve_in_path(name: &str, path: &std::ffi::OsString) -> Option<PathBuf> {
    // Collect extensions from PATHEXT + explicit fallbacks for fnm (.ps1)
    let mut exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".exe;.cmd;.bat;.ps1".into())
        .split(';')
        .map(|s| s.to_lowercase())
        .collect();
    // Ensure .ps1 is always included (fnm default on Windows)
    if !exts.contains(&".ps1".to_string()) {
        exts.push(".ps1".into());
    }
    // Bare name always tried last
    exts.push(String::new());

    for dir in std::env::split_paths(path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Common npm global bin directories on Windows, including fnm version dirs.
fn npm_global_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(&appdata).join("npm"));
    }
    // fnm default alias and version directories
    if let Ok(fnm_dir) = std::env::var("FNM_DIR") {
        dirs.push(PathBuf::from(&fnm_dir));
        // fnm node-versions/<ver>/installation has npm global bins (copilot.cmd etc.)
        let versions = PathBuf::from(&fnm_dir).join("node-versions");
        if versions.exists() {
            if let Ok(entries) = std::fs::read_dir(&versions) {
                for entry in entries.flatten() {
                    let install = entry.path().join("installation");
                    if install.exists() {
                        dirs.push(install);
                    }
                }
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let fnm_default = PathBuf::from(&appdata).join("fnm");
        if fnm_default.exists() {
            dirs.push(fnm_default);
        }
    }
    // USERPROFILE fallback for admin context (FNM_DIR may not be set)
    if let Ok(up) = std::env::var("USERPROFILE") {
        let fnm_roaming = PathBuf::from(&up)
            .join("AppData")
            .join("Roaming")
            .join("fnm");
        if fnm_roaming.exists() {
            dirs.push(fnm_roaming.clone());
            // Also scan node-versions/<ver>/installation for .cmd shims
            let versions = fnm_roaming.join("node-versions");
            if let Ok(entries) = std::fs::read_dir(&versions) {
                for entry in entries.flatten() {
                    let install = entry.path().join("installation");
                    if install.exists() {
                        dirs.push(install);
                    }
                }
            }
        }
    }
    dirs
}

// ---------------------------------------------------------------------------
// Log persistence — write diagnostics to daemon log for HA visibility
// ---------------------------------------------------------------------------

/// Append pre-flight results to the daemon log file so the diagnostics survive
/// across restarts and are visible in the Logs tab.
fn write_diagnostics_to_log(results: &[CheckResult]) {
    let log_path = daemon_log_path();
    let timestamp = chrono_now();

    let mut buf = format!(
        "══════════════════════════════════════════════════════════════\n\
         loom-shell pre-flight diagnostics  [{timestamp}]\n\
         ══════════════════════════════════════════════════════════════\n"
    );

    for c in results {
        buf.push_str(&format!(
            "  {} {}\n    {}\n",
            c.status.icon(),
            c.label,
            c.detail.replace('\n', "\n    "),
        ));
    }

    let has_errors = results.iter().any(|c| c.status == CheckStatus::Error);
    buf.push_str("──────────────────────────────────────────────────────────────\n");
    if has_errors {
        buf.push_str("  RESULT: FAILED — daemon start was blocked due to errors above.\n");
    } else {
        buf.push_str("  RESULT: OK — all checks passed, proceeding with daemon start.\n");
    }
    buf.push_str("══════════════════════════════════════════════════════════════\n\n");

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = file.write_all(buf.as_bytes());
        let _ = file.flush();
    }
}

fn daemon_log_path() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(local)
        .join("loom")
        .join("logs")
        .join("daemon")
        .join("loom-daemon.log")
}

/// Return the current local time as an ISO-ish string (no chrono dep needed).
fn chrono_now() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple UTC breakdown (no chrono dependency).
    let days = secs / 86400;
    let time = secs % 86400;
    let hours = time / 3600;
    let mins = (time % 3600) / 60;
    let secs = time % 60;

    // Unix day 0 = 1970-01-01.  Rough civil date (good enough for logs).
    let (y, m, d) = civil_from_days(days as i64);

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{secs:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
/// Simplified algorithm — accurate enough for log timestamps.
fn civil_from_days(mut days: i64) -> (i64, u32, u32) {
    days += 719468; // shift epoch to 0000-03-01
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month phase [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
