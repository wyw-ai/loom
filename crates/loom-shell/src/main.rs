//! loom-shell — Windows admin GUI
//!
//! Manage loom-server and loom-daemon start/stop, service install/uninstall,
//! and live log viewing through a tray icon + tabbed interface.
//!
//! This crate targets Windows only; non-Windows platforms get an empty stub.
//!
//! ## Auto-elevation
//!
//! Loom is built for unattended automation. On startup, loom-shell checks
//! whether it runs with administrator privileges: if not, it automatically
//! re-launches itself via `ShellExecuteW("runas")`. The elevated process
//! and all its spawned children (server, daemon, agent provider commands)
//! inherit the admin token, so the user never needs to re-confirm UAC.

#[cfg(windows)]
mod config;
#[cfg(windows)]
mod install;
#[cfg(windows)]
mod log_viewer;
#[cfg(windows)]
mod preflight;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    use native_windows_gui as nwg;

    // Ensure we run as admin — Loom Native automation principle.
    ensure_admin_or_restart();

    // Initialize NWG
    nwg::init().expect("Failed to init Native Windows GUI");

    // Build UI and run message loop
    let _app = ui::LoomShell::build().expect("Failed to build LoomShell UI");
    nwg::dispatch_thread_events();
    // Message loop ends (window closed)
}

#[cfg(windows)]
/// Check whether the current process runs as admin. If not, re-launch
/// via `ShellExecuteW("runas")` with elevation, then exit the current process.
///
/// Windows security model requires a single UAC confirmation; after that the
/// entire loom-shell → server → daemon → agent provider process tree runs
/// under the admin token.
fn ensure_admin_or_restart() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

    // Always save user-level env before any elevation logic — even if we're
    // already admin (launched from admin terminal), the admin PATH is the
    // system PATH and won't include fnm/npm-global/user-specific directories.
    save_user_env_before_elevation();

    if is_admin() {
        return;
    }

    // Not admin → re-launch with elevation via ShellExecuteW("runas")
    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_wide: Vec<u16> = exe_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Collect command-line arguments (excluding the exe itself)
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args_str = args.join(" ");
    let args_wide: Vec<u16> = if args_str.is_empty() {
        vec![0]
    } else {
        args_str.encode_utf16().chain(std::iter::once(0)).collect()
    };

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),           // hwnd
            windows_sys::core::w!("runas"), // lpOperation
            exe_wide.as_ptr(),              // lpFile
            args_wide.as_ptr(),             // lpParameters
            windows_sys::core::w!(""),      // lpDirectory
            SW_SHOW,                        // nShowCmd
        )
    };

    // ShellExecuteW return value > 32 means the new process was launched
    if result as isize > 32 {
        // New elevated instance started → exit current non-admin instance
        std::process::exit(0);
    }
    // Otherwise (user denied UAC or error) → continue degraded, don't interrupt
}

#[cfg(not(windows))]
fn main() {
    eprintln!("loom-shell is only available on Windows.");
}

/// Save the current user's PATH and HOME environment variables to a file
/// before UAC elevation. The elevated process inherits the system PATH and
/// won't have user-specific directories (fnm, npm global, cargo, etc.).
///
/// Written to both `%LOCALAPPDATA%\loom\user-env.json` (works in non-elevated
/// context) and `%USERPROFILE%\AppData\Local\loom\user-env.json` (works in
/// admin context where LOCALAPPDATA points to the system profile).
///
/// When already running as admin, PATH is the system PATH (no fnm). In that
/// case we try to recover the user's real PATH from the registry.
fn save_user_env_before_elevation() {
    let mut path = std::env::var_os("PATH").unwrap_or_default();

    // If we're already admin, the current PATH is the system PATH — it won't
    // include user-specific dirs like fnm multishells. Try the registry.
    if is_admin() {
        if let Some(user_path) = read_user_path_from_registry() {
            // Sanity check: a valid PATH does NOT start with "Path" or "REG_".
            if !user_path.starts_with("Path") && !user_path.starts_with("REG_") {
                path = std::ffi::OsString::from(user_path);
            } else {
                tracing::warn!(
                    "registry PATH parse produced garbage prefix, ignoring: {}",
                    &user_path[..user_path.len().min(80)]
                );
            }
        }
    }

    let home = std::env::var_os("HOME").unwrap_or_default();
    let loom_config = std::env::var_os("LOOM_CONFIG_DIR").unwrap_or_default();
    let local_appdata = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
    let appdata_roaming = std::env::var_os("APPDATA").unwrap_or_default();
    let userprofile = std::env::var_os("USERPROFILE").unwrap_or_default();

    let json = serde_json::json!({
        "PATH": path.to_string_lossy(),
        "HOME": home.to_string_lossy(),
        "LOOM_CONFIG_DIR": loom_config.to_string_lossy(),
        "LOCALAPPDATA": local_appdata.to_string_lossy(),
        "APPDATA": appdata_roaming.to_string_lossy(),
        "USERPROFILE": userprofile.to_string_lossy(),
    });
    let Ok(data) = serde_json::to_string_pretty(&json) else {
        return;
    };

    // Write to both locations so the reader finds it regardless of context.
    for appdata in [&*local_appdata.to_string_lossy(), &*format!("{}\\AppData\\Local", userprofile.to_string_lossy())] {
        let dir = std::path::PathBuf::from(appdata).join("loom");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("user-env.json");
        let _ = std::fs::write(&file_path, &data);
        tracing::info!(path = %file_path.display(), "saved user env snapshot");
    }
}

/// Check whether the current process runs with admin privileges.
fn is_admin() -> bool {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned: u32 = 0;
    unsafe {
        if GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        ) == 0
        {
            return false;
        }
    }
    elevation.TokenIsElevated != 0
}

/// Read the user PATH from `HKCU\Environment\Path` registry value.
/// Returns None if the registry key doesn't exist or can't be read.
///
/// Uses the Windows Registry API directly instead of shelling out to
/// `reg.exe`, which avoids fragile string parsing of console output
/// (encoding, leading whitespace, multi-column layout).
fn read_user_path_from_registry() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ,
        REG_EXPAND_SZ, REG_SZ,
    };

    let subkey = wide_null("Environment");
    let value_name = wide_null("Path");

    // Open HKCU\Environment with read access.
    let mut hkey: windows_sys::Win32::System::Registry::HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_READ,
            &mut hkey,
        )
    };
    if status != 0 {
        return None;
    }

    // Probe the value: first call with null buffer to get size + type.
    let mut data_type: u32 = 0;
    let mut data_size: u32 = 0;
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null_mut(),
            &mut data_type,
            std::ptr::null_mut(),
            &mut data_size,
        )
    };
    if status != 0 {
        unsafe { RegCloseKey(hkey) };
        return None;
    }

    // Only accept REG_SZ and REG_EXPAND_SZ (string types).
    // Expand environment variables for REG_EXPAND_SZ using
    // ExpandEnvironmentStringsW.
    let is_expand_sz = data_type == REG_EXPAND_SZ;
    if data_type != REG_SZ && data_type != REG_EXPAND_SZ {
        unsafe { RegCloseKey(hkey) };
        return None;
    }

    // Allocate buffer (data_size is in bytes, including null terminator).
    let byte_len = (data_size / 2) as usize;
    let mut buf: Vec<u16> = vec![0u16; byte_len];
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
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

    // Convert wide string to Rust String (strip null terminator).
    let raw = OsString::from_wide(
        &buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())],
    );
    let raw_str = raw.to_string_lossy().into_owned();

    // Expand environment variables for REG_EXPAND_SZ values.
    if is_expand_sz {
        expand_env_string(&raw_str)
    } else {
        Some(raw_str)
    }
}

/// Expand environment-variable references (e.g. `%SystemRoot%`) in a string.
fn expand_env_string(raw: &str) -> Option<String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;

    let wide_input: Vec<u16> = std::ffi::OsStr::new(raw)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // First call to get required buffer size (in characters, including null).
    let needed =
        unsafe { ExpandEnvironmentStringsW(wide_input.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }

    let mut wide_output: Vec<u16> = vec![0u16; needed as usize];
    let written = unsafe {
        ExpandEnvironmentStringsW(
            wide_input.as_ptr(),
            wide_output.as_mut_ptr(),
            needed,
        )
    };
    if written == 0 || written > needed {
        return None;
    }

    let result = std::ffi::OsString::from_wide(
        &wide_output[..wide_output.iter().position(|&c| c == 0).unwrap_or(wide_output.len())],
    );
    Some(result.to_string_lossy().into_owned())
}

/// Create a null-terminated UTF-16 string from a Rust &str.
fn wide_null(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
