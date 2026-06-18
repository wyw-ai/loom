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
        let handle =
            unsafe { windows_sys::Win32::System::Console::GetStdHandle(STD_OUTPUT_HANDLE) };
        if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE || handle.is_null() {
            return;
        }
        let mut mode: u32 = 0;
        if unsafe {
            windows_sys::Win32::System::Console::GetConsoleMode(handle as RawHandle, &mut mode)
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
                windows_sys::Win32::System::Console::SetConsoleMode(handle as RawHandle, new_mode);
            }
        }
    }
    // On non-Windows this function is intentionally empty — the
    // `#[cfg(windows)]` block above compiles away entirely.
    let _ = ();
}

/// Show a help dialog when the CLI binary is double-clicked with no arguments.
///
/// On Windows this displays a modal `MessageBox` with usage information so the
/// window does not immediately disappear. Returns `true` if the help dialog was
/// shown (i.e. the process was double-clicked) — the caller should then exit.
///
/// On non-Windows platforms this always returns `false`.
pub fn show_double_click_help() -> bool {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().collect();
        // Only intercept when there are literally no arguments (just the
        // program name). Any argument, even `--help`, means the user invoked
        // from a terminal and knows what they're doing.
        if args.len() > 1 {
            return false;
        }

        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let title: Vec<u16> = OsStr::new("Loom CLI")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let body = "\
Loom — 多 actor 协作 CLI 工具

loom.exe 是命令行工具，请在终端中运行：

  loom.exe --help          查看完整帮助
  loom.exe message send    发送消息
  loom.exe task list       查看任务
  loom.exe chat            交互式聊天

常用子命令：channel, thread, message, task, chat, agent, run

项目地址：joi-apps-temp-fork
        ";
        let body: Vec<u16> = OsStr::new(body)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        const MB_OK: u32 = 0;
        const MB_ICONINFORMATION: u32 = 0x40;

        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                body.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = ();
        false
    }
}
