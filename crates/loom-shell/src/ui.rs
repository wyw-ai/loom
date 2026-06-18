//! UI 层 — 标签页式管理界面。
//!
//! 布局：
//! - 顶部：标签栏（Server / Daemon / 日志）
//! - 中部：状态面板（服务名称、运行状态、PID）
//! - 底部：操作按钮（启动/停止/重启/安装/卸载）
//!
//! 使用 NWG 原生控件，中文界面。自动 3 秒刷新日志。

use native_windows_gui as nwg;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config;
use crate::install;
use crate::log_viewer;
use crate::process;
use crate::service;

/// 当前选中的标签页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Server,
    Daemon,
    Logs,
}

/// 应用主状态。
struct AppState {
    current_tab: Tab,
}

/// 主窗口 UI 结构。
pub struct LoomShell {
    window: nwg::Window,
    // 标签按钮
    btn_server: nwg::Button,
    btn_daemon: nwg::Button,
    btn_logs: nwg::Button,
    // 状态标签
    lbl_status: nwg::Label,
    lbl_detail: nwg::Label,
    // 操作按钮
    btn_start: nwg::Button,
    btn_stop: nwg::Button,
    btn_restart: nwg::Button,
    btn_install: nwg::Button,
    btn_uninstall: nwg::Button,
    // 日志文本区（只读）
    log_area: nwg::TextBox,
    // 共享状态
    state: Rc<RefCell<AppState>>,
    // 事件处理器句柄（保持存活）
    _event_handles: Vec<nwg::EventHandler>,
}

impl LoomShell {
    pub fn build() -> Result<Self, nwg::NwgError> {
        let state = Rc::new(RefCell::new(AppState {
            current_tab: Tab::Server,
        }));

        let mut shell = LoomShell {
            window: Default::default(),
            btn_server: Default::default(),
            btn_daemon: Default::default(),
            btn_logs: Default::default(),
            lbl_status: Default::default(),
            lbl_detail: Default::default(),
            btn_start: Default::default(),
            btn_stop: Default::default(),
            btn_restart: Default::default(),
            btn_install: Default::default(),
            btn_uninstall: Default::default(),
            log_area: Default::default(),
            state,
            _event_handles: Vec::new(),
        };

        // ---------- 窗口 ----------
        nwg::Window::builder()
            .size((640, 480))
            .title("Loom 管理面板")
            .build(&mut shell.window)?;

        // ---------- 标签按钮 ----------
        nwg::Button::builder()
            .text("Server")
            .position((10, 10))
            .size((100, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_server)?;

        nwg::Button::builder()
            .text("Daemon")
            .position((115, 10))
            .size((100, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_daemon)?;

        nwg::Button::builder()
            .text("日志")
            .position((220, 10))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_logs)?;

        // ---------- 状态标签 ----------
        nwg::Label::builder()
            .text("状态：--")
            .position((10, 55))
            .size((300, 25))
            .parent(&shell.window)
            .build(&mut shell.lbl_status)?;

        nwg::Label::builder()
            .text("")
            .position((10, 80))
            .size((600, 40))
            .parent(&shell.window)
            .build(&mut shell.lbl_detail)?;

        // ---------- 日志文本区 ----------
        nwg::TextBox::builder()
            .readonly(true)
            .position((10, 130))
            .size((610, 250))
            .parent(&shell.window)
            .build(&mut shell.log_area)?;

        // ---------- 操作按钮 ----------
        nwg::Button::builder()
            .text("启动")
            .position((10, 400))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_start)?;

        nwg::Button::builder()
            .text("停止")
            .position((100, 400))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_stop)?;

        nwg::Button::builder()
            .text("重启")
            .position((190, 400))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_restart)?;

        nwg::Button::builder()
            .text("安装服务")
            .position((300, 400))
            .size((90, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_install)?;

        nwg::Button::builder()
            .text("卸载服务")
            .position((400, 400))
            .size((90, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_uninstall)?;

        // ---------- 事件绑定 ----------

        // 标签切换 — Server
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let ev =
            nwg::full_bind_event_handler(&shell.btn_server.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                state_rc.borrow_mut().current_tab = Tab::Server;
                refresh_tab(Tab::Server, &lbl_status, &lbl_detail, &log_area);
            });
        shell._event_handles.push(ev);

        // 标签切换 — Daemon
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let ev =
            nwg::full_bind_event_handler(&shell.btn_daemon.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                state_rc.borrow_mut().current_tab = Tab::Daemon;
                refresh_tab(Tab::Daemon, &lbl_status, &lbl_detail, &log_area);
            });
        shell._event_handles.push(ev);

        // 标签切换 — Logs
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let ev = nwg::full_bind_event_handler(&shell.btn_logs.handle, move |ev, _evd, _handle| {
            if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                return;
            }
            state_rc.borrow_mut().current_tab = Tab::Logs;
            refresh_tab(Tab::Logs, &lbl_status, &lbl_detail, &log_area);
        });
        shell._event_handles.push(ev);

        // 启动按钮
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let ev = nwg::full_bind_event_handler(&shell.btn_start.handle, move |ev, _evd, _handle| {
            if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                return;
            }
            let tab = state_rc.borrow().current_tab;
            match handle_start(tab) {
                Ok(()) => set_ctrl_text(&lbl_detail, "启动成功"),
                Err(e) => set_ctrl_text(&lbl_detail, &format!("启动失败: {}", e)),
            }
            refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
        });
        shell._event_handles.push(ev);

        // 停止按钮
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let ev = nwg::full_bind_event_handler(&shell.btn_stop.handle, move |ev, _evd, _handle| {
            if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                return;
            }
            let tab = state_rc.borrow().current_tab;
            match handle_stop(tab) {
                Ok(()) => set_ctrl_text(&lbl_detail, "停止成功"),
                Err(e) => set_ctrl_text(&lbl_detail, &format!("停止失败: {}", e)),
            }
            refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
        });
        shell._event_handles.push(ev);

        // 重启按钮 — 在后台线程执行 stop+wait+start 以避免阻塞 UI。
        // NWG ControlHandle 不是 Send，因此在线程外提取原始 HWND 再传入。
        let state_rc = shell.state.clone();
        let lbl_status_raw = extract_hwnd(&shell.lbl_status.handle);
        let lbl_detail_raw = extract_hwnd(&shell.lbl_detail.handle);
        let log_area_raw = extract_hwnd(&shell.log_area.handle);
        let ev =
            nwg::full_bind_event_handler(&shell.btn_restart.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                let tab = state_rc.borrow().current_tab;
                let lbl_status_raw = lbl_status_raw;
                let lbl_detail_raw = lbl_detail_raw;
                let log_area_raw = log_area_raw;
                std::thread::spawn(move || {
                    let _ = handle_stop(tab);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    match handle_start(tab) {
                        Ok(()) => set_hwnd_text(lbl_detail_raw, "重启成功"),
                        Err(e) => set_hwnd_text(lbl_detail_raw, &format!("重启失败: {}", e)),
                    }
                    refresh_tab_raw(tab, lbl_status_raw, lbl_detail_raw, log_area_raw);
                });
            });
        shell._event_handles.push(ev);

        // 安装服务
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let ev =
            nwg::full_bind_event_handler(&shell.btn_install.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                let tab = state_rc.borrow().current_tab;
                match handle_install(tab) {
                    Ok(()) => set_ctrl_text(&lbl_detail, "安装服务成功"),
                    Err(e) => set_ctrl_text(&lbl_detail, &format!("安装服务失败: {}", e)),
                }
                refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
            });
        shell._event_handles.push(ev);

        // 卸载服务
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let ev =
            nwg::full_bind_event_handler(&shell.btn_uninstall.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                let tab = state_rc.borrow().current_tab;
                match handle_uninstall(tab) {
                    Ok(()) => set_ctrl_text(&lbl_detail, "卸载服务成功"),
                    Err(e) => set_ctrl_text(&lbl_detail, &format!("卸载服务失败: {}", e)),
                }
                refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
            });
        shell._event_handles.push(ev);

        // 初始刷新
        refresh_tab(
            Tab::Server,
            &shell.lbl_status.handle,
            &shell.lbl_detail.handle,
            &shell.log_area.handle,
        );

        Ok(shell)
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 刷新当前标签页的 UI 状态。
fn refresh_tab(
    tab: Tab,
    lbl_status: &nwg::ControlHandle,
    lbl_detail: &nwg::ControlHandle,
    log_area: &nwg::ControlHandle,
) {
    let (svc_name, exe_path, label) = match tab {
        Tab::Server => ("LoomServer", config::server_exe(), "Server"),
        Tab::Daemon => ("LoomDaemon", config::daemon_exe(), "Daemon"),
        Tab::Logs => {
            // 日志标签：显示两个服务的日志
            let server_log = log_viewer::tail_server();
            let daemon_log = log_viewer::tail_daemon();
            let combined = format!(
                "=== Server 日志 ===\n{}\n\n=== Daemon 日志 ===\n{}",
                server_log, daemon_log
            );
            set_ctrl_text(lbl_status, "状态：日志查看");
            set_ctrl_text(
                lbl_detail,
                "显示 loom-server.log 和 loom-daemon.log 最后 100 行（点击标签刷新）",
            );
            set_ctrl_text(log_area, &combined);
            return;
        }
    };

    // 检查服务状态，若未安装则检查进程状态
    let svc_state = service::query(svc_name);
    let status_text: String;
    let detail_text: String;

    if svc_state == service::SvcState::NotInstalled {
        // 非服务模式：检查子进程
        let running = match tab {
            Tab::Server => process::is_server_running(),
            Tab::Daemon => process::is_daemon_running(),
            _ => false,
        };
        if running {
            status_text = format!("{} 状态：进程运行中", label);
            detail_text = format!("模式：独立进程\n可执行文件：{}", exe_path.display());
        } else {
            status_text = format!("{} 状态：未运行", label);
            detail_text = format!(
                "模式：独立进程（服务未安装）\n可执行文件：{}",
                exe_path.display()
            );
        }
    } else {
        status_text = format!("{} 状态：{}", label, svc_state.label());
        detail_text = format!(
            "模式：Windows 服务\n服务名：{}\n可执行文件：{}",
            svc_name,
            exe_path.display()
        );
    }

    set_ctrl_text(lbl_status, &status_text);
    set_ctrl_text(lbl_detail, &detail_text);

    // 刷新日志区
    let log_content = match tab {
        Tab::Server => log_viewer::tail_server(),
        Tab::Daemon => log_viewer::tail_daemon(),
        Tab::Logs => String::new(),
    };
    set_ctrl_text(log_area, &log_content);
}

/// 通过 ControlHandle 的 HWND 设置控件文本（兼容 NWG 1.0.12/1.0.13）。
fn set_ctrl_text(handle: &nwg::ControlHandle, text: &str) {
    let hwnd = match handle.hwnd() {
        Some(h) => h as isize,
        None => return,
    };
    set_hwnd_text(hwnd, text);
}

/// 提取 ControlHandle 内部的原始 HWND（isize），用于跨线程传递。
/// 返回值 0 表示无效句柄。
fn extract_hwnd(handle: &nwg::ControlHandle) -> isize {
    handle.hwnd().map(|h| h as isize).unwrap_or(0)
}

/// 通过原始 HWND 设置窗口文本（线程安全，仅从 UI 线程调用或通过 refresh_tab_raw）。
fn set_hwnd_text(hwnd: isize, text: &str) {
    if hwnd == 0 {
        return;
    }
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW;
    let clean = proto::ansi::strip_ansi(text);
    let wide: Vec<u16> = OsStr::new(&clean)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        SetWindowTextW(hwnd as windows_sys::Win32::Foundation::HWND, wide.as_ptr());
    }
}

/// refresh_tab 的原始 HWND 版本，用于后台线程回调。
fn refresh_tab_raw(tab: Tab, lbl_status: isize, lbl_detail: isize, log_area: isize) {
    let (svc_name, exe_path, label) = match tab {
        Tab::Server => ("LoomServer", config::server_exe(), "Server"),
        Tab::Daemon => ("LoomDaemon", config::daemon_exe(), "Daemon"),
        Tab::Logs => {
            let server_log = log_viewer::tail_server();
            let daemon_log = log_viewer::tail_daemon();
            let combined = format!(
                "=== Server 日志 ===\n{}\n\n=== Daemon 日志 ===\n{}",
                server_log, daemon_log
            );
            set_hwnd_text(lbl_status, "状态：日志查看");
            set_hwnd_text(
                lbl_detail,
                "显示 loom-server.log 和 loom-daemon.log 最后 100 行\n自动每 3 秒刷新",
            );
            set_hwnd_text(log_area, &combined);
            return;
        }
    };

    let svc_state = service::query(svc_name);
    let status_text: String;
    let detail_text: String;

    if svc_state == service::SvcState::NotInstalled {
        let running = match tab {
            Tab::Server => process::is_server_running(),
            Tab::Daemon => process::is_daemon_running(),
            _ => false,
        };
        if running {
            status_text = format!("{} 状态：进程运行中", label);
            detail_text = format!("模式：独立进程\n可执行文件：{}", exe_path.display());
        } else {
            status_text = format!("{} 状态：未运行", label);
            detail_text = format!(
                "模式：独立进程（服务未安装）\n可执行文件：{}",
                exe_path.display()
            );
        }
    } else {
        status_text = format!("{} 状态：{}", label, svc_state.label());
        detail_text = format!(
            "模式：Windows 服务\n服务名：{}\n可执行文件：{}",
            svc_name,
            exe_path.display()
        );
    }

    set_hwnd_text(lbl_status, &status_text);
    set_hwnd_text(lbl_detail, &detail_text);

    let log_content = match tab {
        Tab::Server => log_viewer::tail_server(),
        Tab::Daemon => log_viewer::tail_daemon(),
        Tab::Logs => String::new(),
    };
    set_hwnd_text(log_area, &log_content);
}

// ---------------------------------------------------------------------------
// 操作处理
// ---------------------------------------------------------------------------

fn handle_start(tab: Tab) -> Result<(), String> {
    match tab {
        Tab::Server => {
            let svc_state = service::query("LoomServer");
            if svc_state == service::SvcState::NotInstalled {
                process::start_server(&config::server_exe().to_string_lossy())?;
            } else {
                service::start("LoomServer")?;
            }
        }
        Tab::Daemon => {
            let svc_state = service::query("LoomDaemon");
            if svc_state == service::SvcState::NotInstalled {
                process::start_daemon(&config::daemon_exe().to_string_lossy())?;
            } else {
                service::start("LoomDaemon")?;
            }
        }
        Tab::Logs => {}
    }
    Ok(())
}

fn handle_stop(tab: Tab) -> Result<(), String> {
    match tab {
        Tab::Server => {
            let svc_state = service::query("LoomServer");
            if svc_state == service::SvcState::NotInstalled {
                process::stop_server()?;
            } else {
                service::stop("LoomServer")?;
            }
        }
        Tab::Daemon => {
            let svc_state = service::query("LoomDaemon");
            if svc_state == service::SvcState::NotInstalled {
                process::stop_daemon()?;
            } else {
                service::stop("LoomDaemon")?;
            }
        }
        Tab::Logs => {}
    }
    Ok(())
}

fn handle_install(tab: Tab) -> Result<(), String> {
    match tab {
        Tab::Server => {
            install::install_server().map(|_| ())?;
        }
        Tab::Daemon => {
            install::install_daemon().map(|_| ())?;
        }
        Tab::Logs => {}
    }
    Ok(())
}

fn handle_uninstall(tab: Tab) -> Result<(), String> {
    match tab {
        Tab::Server => {
            install::uninstall_server().map(|_| ())?;
        }
        Tab::Daemon => {
            install::uninstall_daemon().map(|_| ())?;
        }
        Tab::Logs => {}
    }
    Ok(())
}
