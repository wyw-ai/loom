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
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

    // Check if the current token is already elevated
    let mut token: HANDLE = std::ptr::null_mut();
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if ok == 0 {
        // Failed to open token → continue (degraded), don't panic here
        return;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned: u32 = 0;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    if ok != 0 && elevation.TokenIsElevated != 0 {
        // Already admin → proceed normally
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
