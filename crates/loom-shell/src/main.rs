//! loom-shell — Windows 管理 GUI
//!
//! 通过托盘图标 + 标签页界面管理 loom-server 和 loom-daemon 的
//! 启停、服务安装/卸载，以及实时日志查看。
//!
//! 整个 crate 仅面向 Windows 构建；非 Windows 平台提供一个空 stub。

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

    // 初始化 NWG
    nwg::init().expect("Failed to init Native Windows GUI");

    // 构建 UI 并运行消息循环
    let app = ui::LoomShell::build().expect("Failed to build LoomShell UI");
    nwg::dispatch_thread_events();
    // 消息循环结束（窗口关闭）
}

#[cfg(not(windows))]
fn main() {
    eprintln!("loom-shell is only available on Windows.");
}
