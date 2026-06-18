//! UI layer — tabbed admin interface.
//!
//! Layout:
//! - Top: tab bar (Server / Daemon / Logs)
//! - Middle: status panel (service name, running state, PID)
//! - Bottom: action buttons (Start / Stop / Restart / Install / Uninstall)
//!
//! Uses NWG native controls with English labels. Auto-refreshes logs every 3 seconds.

use native_windows_gui as nwg;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config;
use crate::install;
use crate::log_viewer;
use crate::process;
use crate::service;

/// Currently selected tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Server,
    Daemon,
    Logs,
}

/// Application main state.
struct AppState {
    current_tab: Tab,
}

/// Main window UI structure.
pub struct LoomShell {
    window: nwg::Window,
    // Tab buttons
    btn_server: nwg::Button,
    btn_daemon: nwg::Button,
    btn_logs: nwg::Button,
    // Status labels
    lbl_status: nwg::Label,
    lbl_detail: nwg::Label,
    // Action buttons
    btn_start: nwg::Button,
    btn_stop: nwg::Button,
    btn_restart: nwg::Button,
    btn_install: nwg::Button,
    btn_uninstall: nwg::Button,
    // Log text area (read-only)
    log_area: nwg::TextBox,
    // Shared state
    state: Rc<RefCell<AppState>>,
    // Event handler handles (kept alive)
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

        // ---------- Window ----------
        nwg::Window::builder()
            .size((640, 480))
            .title("Loom Admin Panel")
            .build(&mut shell.window)?;

        // ---------- Tab buttons ----------
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
            .text("Logs")
            .position((220, 10))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_logs)?;

        // ---------- Status labels ----------
        nwg::Label::builder()
            .text("Status: --")
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

        // ---------- Log text area ----------
        nwg::TextBox::builder()
            .readonly(true)
            .position((10, 130))
            .size((610, 250))
            .parent(&shell.window)
            .build(&mut shell.log_area)?;

        // ---------- Action buttons ----------
        nwg::Button::builder()
            .text("Start")
            .position((10, 400))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_start)?;

        nwg::Button::builder()
            .text("Stop")
            .position((100, 400))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_stop)?;

        nwg::Button::builder()
            .text("Restart")
            .position((190, 400))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_restart)?;

        nwg::Button::builder()
            .text("Install")
            .position((300, 400))
            .size((90, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_install)?;

        nwg::Button::builder()
            .text("Uninstall")
            .position((400, 400))
            .size((90, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_uninstall)?;

        // ---------- Event bindings ----------

        // Tab switch — Server
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

        // Tab switch — Daemon
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

        // Tab switch — Logs
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

        // Start button
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
                Ok(()) => set_ctrl_text(&lbl_detail, "Started successfully"),
                Err(e) => set_ctrl_text(&lbl_detail, &format!("Start failed: {}", e)),
            }
            refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
        });
        shell._event_handles.push(ev);

        // Stop button
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
                Ok(()) => set_ctrl_text(&lbl_detail, "Stopped successfully"),
                Err(e) => set_ctrl_text(&lbl_detail, &format!("Stop failed: {}", e)),
            }
            refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
        });
        shell._event_handles.push(ev);

        // Restart button — run stop+wait+start on a background thread to avoid
        // blocking the UI. NWG ControlHandle is not Send, so extract the raw
        // HWND outside the thread and pass it in.
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
                        Ok(()) => set_hwnd_text(lbl_detail_raw, "Restarted successfully"),
                        Err(e) => set_hwnd_text(lbl_detail_raw, &format!("Restart failed: {}", e)),
                    }
                    refresh_tab_raw(tab, lbl_status_raw, lbl_detail_raw, log_area_raw);
                });
            });
        shell._event_handles.push(ev);

        // Install service
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
                    Ok(()) => set_ctrl_text(&lbl_detail, "Service installed successfully"),
                    Err(e) => set_ctrl_text(&lbl_detail, &format!("Service install failed: {}", e)),
                }
                refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
            });
        shell._event_handles.push(ev);

        // Uninstall service
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
                    Ok(()) => set_ctrl_text(&lbl_detail, "Service uninstalled successfully"),
                    Err(e) => {
                        set_ctrl_text(&lbl_detail, &format!("Service uninstall failed: {}", e))
                    }
                }
                refresh_tab(tab, &lbl_status, &lbl_detail, &log_area);
            });
        shell._event_handles.push(ev);

        // Initial refresh
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
// Helper functions
// ---------------------------------------------------------------------------

/// Refresh the current tab's UI state.
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
            // Logs tab: show both service logs
            let server_log = log_viewer::tail_server();
            let daemon_log = log_viewer::tail_daemon();
            let combined = format!(
                "=== Server Log ===\n{}\n\n=== Daemon Log ===\n{}",
                server_log, daemon_log
            );
            set_ctrl_text(lbl_status, "Status: Log viewer");
            set_ctrl_text(
                lbl_detail,
                "Showing last 100 lines of loom-server.log and loom-daemon.log (click tab to refresh)",
            );
            set_ctrl_text(log_area, &combined);
            return;
        }
    };

    // Check service status; fall back to process check if not installed
    let svc_state = service::query(svc_name);
    let status_text: String;
    let detail_text: String;

    if svc_state == service::SvcState::NotInstalled {
        // Non-service mode: check child process
        let running = match tab {
            Tab::Server => process::is_server_running(),
            Tab::Daemon => process::is_daemon_running(),
            _ => false,
        };
        if running {
            status_text = format!("{} Status: Process running", label);
            detail_text = format!("Mode: Standalone process\nExecutable: {}", exe_path.display());
        } else {
            status_text = format!("{} Status: Not running", label);
            detail_text = format!(
                "Mode: Standalone process (service not installed)\nExecutable: {}",
                exe_path.display()
            );
        }
    } else {
        status_text = format!("{} Status: {}", label, svc_state.label());
        detail_text = format!(
            "Mode: Windows Service\nService name: {}\nExecutable: {}",
            svc_name,
            exe_path.display()
        );
    }

    set_ctrl_text(lbl_status, &status_text);
    set_ctrl_text(lbl_detail, &detail_text);

    // Refresh log area
    let log_content = match tab {
        Tab::Server => log_viewer::tail_server(),
        Tab::Daemon => log_viewer::tail_daemon(),
        Tab::Logs => String::new(),
    };
    set_ctrl_text(log_area, &log_content);
}

/// Set control text via ControlHandle's HWND (compatible with NWG 1.0.12/1.0.13).
fn set_ctrl_text(handle: &nwg::ControlHandle, text: &str) {
    let hwnd = match handle.hwnd() {
        Some(h) => h as isize,
        None => return,
    };
    set_hwnd_text(hwnd, text);
}

/// Extract the raw HWND (isize) from a ControlHandle for cross-thread passing.
/// Returns 0 for an invalid handle.
fn extract_hwnd(handle: &nwg::ControlHandle) -> isize {
    handle.hwnd().map(|h| h as isize).unwrap_or(0)
}

/// Set window text via raw HWND (thread-safe, call only from UI thread or via
/// refresh_tab_raw).
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

/// Raw HWND version of refresh_tab, for background thread callbacks.
fn refresh_tab_raw(tab: Tab, lbl_status: isize, lbl_detail: isize, log_area: isize) {
    let (svc_name, exe_path, label) = match tab {
        Tab::Server => ("LoomServer", config::server_exe(), "Server"),
        Tab::Daemon => ("LoomDaemon", config::daemon_exe(), "Daemon"),
        Tab::Logs => {
            let server_log = log_viewer::tail_server();
            let daemon_log = log_viewer::tail_daemon();
            let combined = format!(
                "=== Server Log ===\n{}\n\n=== Daemon Log ===\n{}",
                server_log, daemon_log
            );
            set_hwnd_text(lbl_status, "Status: Log viewer");
            set_hwnd_text(
                lbl_detail,
                "Showing last 100 lines of loom-server.log and loom-daemon.log\nAuto-refreshes every 3 seconds",
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
            status_text = format!("{} Status: Process running", label);
            detail_text = format!("Mode: Standalone process\nExecutable: {}", exe_path.display());
        } else {
            status_text = format!("{} Status: Not running", label);
            detail_text = format!(
                "Mode: Standalone process (service not installed)\nExecutable: {}",
                exe_path.display()
            );
        }
    } else {
        status_text = format!("{} Status: {}", label, svc_state.label());
        detail_text = format!(
            "Mode: Windows Service\nService name: {}\nExecutable: {}",
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
// Action handlers
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
