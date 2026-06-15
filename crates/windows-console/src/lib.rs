//! Windows console compatibility helpers.
//!
//! On Windows, the default console host (conhost.exe) enables "Quick Edit
//! Mode" which suspends all output threads when the user clicks inside the
//! console window. This makes long-running server/daemon processes appear to
//! hang. Calling [`init()`] at process start disables this behaviour.
//!
//! On non-Windows platforms [`init()`] is a no-op so callers can invoke it
//! unconditionally without `#[cfg]` gating at the call site.

/// Initialise the Windows console for long-running background processes.
///
/// Disables `ENABLE_QUICK_EDIT_MODE` and `ENABLE_INSERT_MODE` on the
/// process's standard output handle so that accidental mouse clicks do not
/// block stdout writes.
///
/// # Errors
///
/// Errors are silently ignored — this is a best-effort compatibility fix.
/// If the process is not attached to a console (e.g. running as a Windows
/// Service, or piped/redirected) the call has no effect.
pub fn init() {
    #[cfg(windows)]
    {
        use std::os::windows::io::RawHandle;
        // STD_OUTPUT_HANDLE = -11
        const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5u32;
        let handle = unsafe {
            windows_sys::Win32::System::Console::GetStdHandle(STD_OUTPUT_HANDLE)
        };
        if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE || handle.is_null() {
            return;
        }
        let mut mode: u32 = 0;
        if unsafe {
            windows_sys::Win32::System::Console::GetConsoleMode(
                handle as RawHandle,
                &mut mode,
            )
        } == 0
        {
            return;
        }
        // ENABLE_QUICK_EDIT_MODE = 0x0040, ENABLE_INSERT_MODE = 0x0020
        const ENABLE_QUICK_EDIT_MODE: u32 = 0x0040;
        const ENABLE_INSERT_MODE: u32 = 0x0020;
        let new_mode = mode & !(ENABLE_QUICK_EDIT_MODE | ENABLE_INSERT_MODE);
        if new_mode != mode {
            unsafe {
                windows_sys::Win32::System::Console::SetConsoleMode(
                    handle as RawHandle,
                    new_mode,
                );
            }
        }
    }
    // On non-Windows this function is intentionally empty — the
    // `#[cfg(windows)]` block above compiles away entirely.
    let _ = ();
}
