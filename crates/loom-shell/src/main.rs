//! loom-shell — Windows 管理 GUI
//!
//! 通过托盘图标 + 标签页界面管理 loom-server 和 loom-daemon 的
//! 启停、服务安装/卸载，以及实时日志查看。
//!
//! 整个 crate 仅面向 Windows 构建；非 Windows 平台提供一个空 stub。
//!
//! ## 管理员自动提权
//!
//! Loom 致力于自动化无看管运行。loom-shell 启动时检测是否以管理员
//! 身份运行：如果不是，自动通过 `ShellExecuteW("runas")` 提权重启。
//! 提权后的进程以及它 spawn 的所有子进程（server、daemon、agent
//! provider 命令）都继承管理员令牌，无需用户反复确认 UAC。

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

    // 确保以管理员身份运行 — Loom Native 自动化原则。
    ensure_admin_or_restart();

    // 初始化 NWG
    nwg::init().expect("Failed to init Native Windows GUI");

    // 构建 UI 并运行消息循环
    let _app = ui::LoomShell::build().expect("Failed to build LoomShell UI");
    nwg::dispatch_thread_events();
    // 消息循环结束（窗口关闭）
}

#[cfg(windows)]
/// 检测当前进程是否以管理员身份运行。如果不是，通过 ShellExecuteW("runas")
/// 自动提权重启当前 exe，然后退出当前进程。
///
/// Windows 安全模型要求 UAC 弹窗确认；用户只需确认一次，之后整个
/// loom-shell → server → daemon → agent provider 进程树都运行在管理员
/// 令牌下。
fn ensure_admin_or_restart() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

    // 检查当前令牌是否已提权
    let mut token: HANDLE = std::ptr::null_mut();
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if ok == 0 {
        // 无法打开令牌 → 继续（降级运行），不要在这里 panic
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
        // 已经是管理员 → 正常继续
        return;
    }

    // 不是管理员 → 通过 ShellExecuteW("runas") 提权重启
    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_wide: Vec<u16> = exe_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // 收集命令行参数（不含 exe 自身）
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

    // ShellExecuteW 返回值 > 32 表示成功启动新进程
    if result as isize > 32 {
        // 新实例已启动 → 退出当前非管理员实例
        std::process::exit(0);
    }
    // 否则（用户拒绝 UAC 或出错）→ 降级运行，不做打断
}

#[cfg(not(windows))]
fn main() {
    eprintln!("loom-shell is only available on Windows.");
}
