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
use crate::preflight;
use crate::process;
use crate::service;

/// List available machine config directories under daemon-configs/.
/// Returns (machine_id, display_name) pairs.
fn list_machine_configs() -> Vec<(String, String)> {
    let Some(configs_dir) = preflight::loom_config_dir() else {
        return Vec::new();
    };
    let configs_dir = configs_dir.join("daemon-configs");
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&configs_dir) {
        for entry in read_dir.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                let machine_id = entry.file_name().to_string_lossy().to_string();
                // Try to read the name from daemon.toml in this directory
                let name = entry.path().join("daemon.toml");
                let display = if let Ok(data) = std::fs::read_to_string(&name) {
                    extract_toml_value(&data, "name").unwrap_or_else(|| machine_id.clone())
                } else {
                    machine_id.clone()
                };
                entries.push((machine_id, display));
            }
        }
    }
    entries
}

/// Extract a simple string value from TOML under [machine] section.
fn extract_toml_value(data: &str, key: &str) -> Option<String> {
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
            if let Some(rest) = trimmed.strip_prefix(key) {
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

/// Extract a top-level `key = "value"` from TOML-like content (not section-scoped).
fn extract_direct_value(data: &str, key: &str) -> Option<String> {
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            continue; // skip section headers
        }
        if let Some(rest) = trimmed.strip_prefix(key) {
            let value = rest
                .trim_start_matches(|c: char| c == ' ' || c == '=' || c == '"')
                .trim_end_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Build a detailed status line for the Daemon tab.
/// Shows the active machine-id from daemon.toml and lists available machine configs
/// under daemon-configs/.
fn format_daemon_detail(extra_info: &str) -> String {
    let mut detail = String::from(extra_info);
    detail.push('\n');

    // Show active machine-id from daemon.toml
    match process::machine_id_from_daemon_config() {
        Some(mid) => {
            detail.push_str(&format!("\r\nActive machine-id: {mid}"));
        }
        None => {
            detail.push_str("\r\nNo machine-id configured in daemon.toml");
        }
    }

    // List available machine configs under daemon-configs/
    let Some(configs_dir) = preflight::loom_config_dir() else {
        return detail;
    };
    let configs_dir = configs_dir.join("daemon-configs");
    if configs_dir.is_dir() {
        let entries: Vec<_> = std::fs::read_dir(&configs_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        if entries.is_empty() {
            detail.push_str("\r\nAvailable configs: (none)");
        } else {
            detail.push_str("\r\nAvailable machine configs in daemon-configs/:");
            for entry in &entries {
                let marker = if Some(entry.as_str())
                    == process::machine_id_from_daemon_config().as_deref()
                {
                    " <-- active"
                } else {
                    ""
                };
                detail.push_str(&format!("\r\n  - {entry}{marker}"));
            }
        }
    }

    detail
}

/// Currently selected tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Server,
    Daemon,
    Logs,
    Config,
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
    btn_config: nwg::Button,
    // Status labels
    lbl_status: nwg::Label,
    lbl_detail: nwg::Label,
    // Machine selector (Daemon tab)
    cmb_machine: nwg::ComboBox<String>,
    // Action buttons
    btn_start: nwg::Button,
    btn_stop: nwg::Button,
    btn_restart: nwg::Button,
    btn_install: nwg::Button,
    btn_uninstall: nwg::Button,
    // Config editor (shared between Daemon and Config tabs)
    lbl_config: nwg::Label,
    txt_config: nwg::TextBox,
    btn_save_config: nwg::Button,
    btn_discard: nwg::Button,
    btn_clean_zombies: nwg::Button,
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
            btn_config: Default::default(),
            lbl_status: Default::default(),
            lbl_detail: Default::default(),
            cmb_machine: Default::default(),
            btn_start: Default::default(),
            btn_stop: Default::default(),
            btn_restart: Default::default(),
            btn_install: Default::default(),
            btn_uninstall: Default::default(),
            lbl_config: Default::default(),
            txt_config: Default::default(),
            btn_save_config: Default::default(),
            btn_discard: Default::default(),
            btn_clean_zombies: Default::default(),
            log_area: Default::default(),
            state,
            _event_handles: Vec::new(),
        };

        // ---------- Window ----------
        nwg::Window::builder()
            .size((640, 680))
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

        nwg::Button::builder()
            .text("Config")
            .position((305, 10))
            .size((80, 30))
            .parent(&shell.window)
            .build(&mut shell.btn_config)?;

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

        // ---------- Machine selector (Daemon tab) ----------
        nwg::ComboBox::builder()
            .position((10, 122))
            .size((250, 200))
            .parent(&shell.window)
            .build(&mut shell.cmb_machine)?;

        // Discard selected machine config
        nwg::Button::builder()
            .text("Discard")
            .position((270, 122))
            .size((65, 24))
            .parent(&shell.window)
            .build(&mut shell.btn_discard)?;

        // Clean zombie actors from server
        nwg::Button::builder()
            .text("Clean Zombies")
            .position((340, 122))
            .size((100, 24))
            .parent(&shell.window)
            .build(&mut shell.btn_clean_zombies)?;

        // ---------- Log text area ----------
        nwg::TextBox::builder()
            .readonly(true)
            .position((10, 155))
            .size((610, 225))
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

        // ---------- Config file editor (Daemon tab → daemon.toml, Config tab → cli.toml) ----------
        nwg::Label::builder()
            .text("")
            .position((10, 440))
            .size((300, 20))
            .parent(&shell.window)
            .build(&mut shell.lbl_config)?;

        nwg::TextBox::builder()
            .position((10, 462))
            .size((610, 150))
            .parent(&shell.window)
            .build(&mut shell.txt_config)?;

        nwg::Button::builder()
            .text("Save Config")
            .position((520, 618))
            .size((100, 28))
            .parent(&shell.window)
            .build(&mut shell.btn_save_config)?;

        // ---------- Event bindings ----------

        // Tab switch — Server
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let ev =
            nwg::full_bind_event_handler(&shell.btn_server.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                state_rc.borrow_mut().current_tab = Tab::Server;
                refresh_tab(
                    Tab::Server,
                    &lbl_status,
                    &lbl_detail,
                    &log_area,
                    &cmb_machine,
                );
                populate_config_editor(Tab::Server, &lbl_config, &txt_config, Some(&btn_save));
            });
        shell._event_handles.push(ev);

        // Tab switch — Daemon
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let ev =
            nwg::full_bind_event_handler(&shell.btn_daemon.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                state_rc.borrow_mut().current_tab = Tab::Daemon;
                refresh_tab(
                    Tab::Daemon,
                    &lbl_status,
                    &lbl_detail,
                    &log_area,
                    &cmb_machine,
                );
                populate_config_editor(Tab::Daemon, &lbl_config, &txt_config, Some(&btn_save));
            });
        shell._event_handles.push(ev);

        // Tab switch — Logs
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let ev = nwg::full_bind_event_handler(&shell.btn_logs.handle, move |ev, _evd, _handle| {
            if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                return;
            }
            state_rc.borrow_mut().current_tab = Tab::Logs;
            refresh_tab(Tab::Logs, &lbl_status, &lbl_detail, &log_area, &cmb_machine);
            populate_config_editor(Tab::Logs, &lbl_config, &txt_config, Some(&btn_save));
        });
        shell._event_handles.push(ev);

        // Tab switch — Config
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let ev =
            nwg::full_bind_event_handler(&shell.btn_config.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                state_rc.borrow_mut().current_tab = Tab::Config;
                refresh_tab(
                    Tab::Config,
                    &lbl_status,
                    &lbl_detail,
                    &log_area,
                    &cmb_machine,
                );
                populate_config_editor(Tab::Config, &lbl_config, &txt_config, Some(&btn_save));
            });
        shell._event_handles.push(ev);

        // Start button
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let ev = nwg::full_bind_event_handler(&shell.btn_start.handle, move |ev, _evd, _handle| {
            if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                return;
            }
            let tab = state_rc.borrow().current_tab;
            let machine_id = if matches!(tab, Tab::Daemon) {
                read_combo_machine_id(extract_hwnd(&cmb_machine))
            } else {
                None
            };
            match handle_start(tab, machine_id.as_deref()) {
                Ok(()) => set_ctrl_text(&lbl_detail, "Started successfully"),
                Err(e) => set_ctrl_text(&lbl_detail, &format!("Start failed: {}", e)),
            }
            refresh_tab(tab, &lbl_status, &lbl_detail, &log_area, &cmb_machine);
            populate_config_editor(tab, &lbl_config, &txt_config, Some(&btn_save));
        });
        shell._event_handles.push(ev);

        // Stop button
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let ev = nwg::full_bind_event_handler(&shell.btn_stop.handle, move |ev, _evd, _handle| {
            if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                return;
            }
            let tab = state_rc.borrow().current_tab;
            match handle_stop(tab) {
                Ok(()) => set_ctrl_text(&lbl_detail, "Stopped successfully"),
                Err(e) => set_ctrl_text(&lbl_detail, &format!("Stop failed: {}", e)),
            }
            refresh_tab(tab, &lbl_status, &lbl_detail, &log_area, &cmb_machine);
            populate_config_editor(tab, &lbl_config, &txt_config, Some(&btn_save));
        });
        shell._event_handles.push(ev);

        // Restart button — run stop+wait+start on a background thread to avoid
        // blocking the UI. NWG ControlHandle is not Send, so extract the raw
        // HWND outside the thread and pass it in.
        let state_rc = shell.state.clone();
        let lbl_status_raw = extract_hwnd(&shell.lbl_status.handle);
        let lbl_detail_raw = extract_hwnd(&shell.lbl_detail.handle);
        let log_area_raw = extract_hwnd(&shell.log_area.handle);
        let cmb_machine_raw = extract_hwnd(&shell.cmb_machine.handle);
        let lbl_config_raw = extract_hwnd(&shell.lbl_config.handle);
        let txt_config_raw = extract_hwnd(&shell.txt_config.handle);
        let ev =
            nwg::full_bind_event_handler(&shell.btn_restart.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                let tab = state_rc.borrow().current_tab;
                let lbl_status_raw = lbl_status_raw;
                let lbl_detail_raw = lbl_detail_raw;
                let log_area_raw = log_area_raw;
                let cmb_machine_raw = cmb_machine_raw;
                let lbl_config_raw = lbl_config_raw;
                let txt_config_raw = txt_config_raw;
                std::thread::spawn(move || {
                    let _ = handle_stop(tab);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let machine_id = if matches!(tab, Tab::Daemon) {
                        read_combo_machine_id(cmb_machine_raw)
                    } else {
                        None
                    };
                    match handle_start(tab, machine_id.as_deref()) {
                        Ok(()) => set_hwnd_text(lbl_detail_raw, "Restarted successfully"),
                        Err(e) => set_hwnd_text(lbl_detail_raw, &format!("Restart failed: {}", e)),
                    }
                    refresh_tab_raw(
                        tab,
                        lbl_status_raw,
                        lbl_detail_raw,
                        log_area_raw,
                        cmb_machine_raw,
                    );
                    populate_config_editor_raw(tab, lbl_config_raw, txt_config_raw);
                });
            });
        shell._event_handles.push(ev);

        // Install service
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
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
                refresh_tab(tab, &lbl_status, &lbl_detail, &log_area, &cmb_machine);
                populate_config_editor(tab, &lbl_config, &txt_config, Some(&btn_save));
            });
        shell._event_handles.push(ev);

        // Uninstall service
        let state_rc = shell.state.clone();
        let lbl_status = shell.lbl_status.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let log_area = shell.log_area.handle;
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
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
                refresh_tab(tab, &lbl_status, &lbl_detail, &log_area, &cmb_machine);
                populate_config_editor(tab, &lbl_config, &txt_config, Some(&btn_save));
            });
        shell._event_handles.push(ev);

        // Save Config button
        let state_rc = shell.state.clone();
        let txt_config = shell.txt_config.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let lbl_config = shell.lbl_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let ev = nwg::full_bind_event_handler(
            &shell.btn_save_config.handle,
            move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                let tab = state_rc.borrow().current_tab;
                let content = get_ctrl_text(&txt_config);
                let result = match tab {
                    Tab::Daemon => save_daemon_toml(&content),
                    Tab::Config => save_cli_toml(&content),
                    _ => Err("No config to save on this tab".to_string()),
                };
                match result {
                    Ok(()) => set_ctrl_text(&lbl_detail, "Config saved successfully"),
                    Err(e) => set_ctrl_text(&lbl_detail, &format!("Save failed: {}", e)),
                }
                // Re-read config file to show the saved content
                populate_config_editor(tab, &lbl_config, &txt_config, Some(&btn_save));
            },
        );
        shell._event_handles.push(ev);

        // Discard button — delete selected machine config from daemon-configs/
        let cmb_machine = shell.cmb_machine.handle;
        let lbl_detail = shell.lbl_detail.handle;
        let lbl_status = shell.lbl_status.handle;
        let log_area = shell.log_area.handle;
        let lbl_config = shell.lbl_config.handle;
        let txt_config = shell.txt_config.handle;
        let btn_save = shell.btn_save_config.handle;
        let state_rc = shell.state.clone();
        let ev =
            nwg::full_bind_event_handler(&shell.btn_discard.handle, move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                let machine_id = read_combo_machine_id(extract_hwnd(&cmb_machine));
                match machine_id {
                    Some(ref id) if !id.is_empty() => match handle_discard_machine(id) {
                        Ok(msg) => set_ctrl_text(&lbl_detail, &msg),
                        Err(e) => set_ctrl_text(&lbl_detail, &format!("Discard failed: {}", e)),
                    },
                    _ => set_ctrl_text(&lbl_detail, "No machine selected to discard"),
                }
                let tab = state_rc.borrow().current_tab;
                refresh_tab(tab, &lbl_status, &lbl_detail, &log_area, &cmb_machine);
                populate_config_editor(tab, &lbl_config, &txt_config, Some(&btn_save));
            });
        shell._event_handles.push(ev);

        // Clean Zombies button — scan server for orphan machine actors
        // and delete them. Runs on a background thread to avoid UI freeze.
        let lbl_detail_raw = extract_hwnd(&shell.lbl_detail.handle);
        let lbl_status_raw = extract_hwnd(&shell.lbl_status.handle);
        let log_area_raw = extract_hwnd(&shell.log_area.handle);
        let cmb_machine_raw = extract_hwnd(&shell.cmb_machine.handle);
        let lbl_config_raw = extract_hwnd(&shell.lbl_config.handle);
        let txt_config_raw = extract_hwnd(&shell.txt_config.handle);
        let ev = nwg::full_bind_event_handler(
            &shell.btn_clean_zombies.handle,
            move |ev, _evd, _handle| {
                if ev != nwg::Event::OnMousePress(nwg::MousePressEvent::MousePressLeftUp) {
                    return;
                }
                // Show "working…" immediately
                set_hwnd_text(lbl_detail_raw, "Scanning server for zombie actors…");
                let lbl_detail_raw = lbl_detail_raw;
                let lbl_status_raw = lbl_status_raw;
                let log_area_raw = log_area_raw;
                let cmb_machine_raw = cmb_machine_raw;
                let lbl_config_raw = lbl_config_raw;
                let txt_config_raw = txt_config_raw;
                std::thread::spawn(move || {
                    let result = handle_clean_zombie_actors();
                    match &result {
                        Ok(msg) => set_hwnd_text(lbl_detail_raw, msg),
                        Err(e) => {
                            set_hwnd_text(lbl_detail_raw, &format!("Clean zombies failed: {}", e))
                        }
                    }
                    // Refresh the Server tab
                    refresh_tab_raw(
                        Tab::Server,
                        lbl_status_raw,
                        lbl_detail_raw,
                        log_area_raw,
                        cmb_machine_raw,
                    );
                    populate_config_editor_raw(Tab::Server, lbl_config_raw, txt_config_raw);
                });
            },
        );
        shell._event_handles.push(ev);

        // Hide config editor controls initially (shown only on Daemon/Config tabs).
        show_window(extract_hwnd(&shell.lbl_config.handle), false);
        show_window(extract_hwnd(&shell.txt_config.handle), false);
        show_window(extract_hwnd(&shell.btn_save_config.handle), false);

        // Initial refresh
        refresh_tab(
            Tab::Server,
            &shell.lbl_status.handle,
            &shell.lbl_detail.handle,
            &shell.log_area.handle,
            &shell.cmb_machine.handle,
        );
        populate_config_editor(
            Tab::Server,
            &shell.lbl_config.handle,
            &shell.txt_config.handle,
            Some(&shell.btn_save_config.handle),
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
    cmb_machine: &nwg::ControlHandle,
) {
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
            set_ctrl_text(lbl_status, "Status: Log viewer");
            set_ctrl_text(
                lbl_detail,
                "Showing last 100 lines of loom-server.log and loom-daemon.log (click tab to refresh)",
            );
            set_ctrl_text(log_area, &combined);
            return;
        }
        Tab::Config => {
            show_env_config(lbl_status, lbl_detail, log_area);
            return;
        }
    };

    // Check service status; fall back to process check if not installed
    let svc_state = service::query(svc_name);
    let status_text: String;
    let detail_text: String;

    if svc_state == service::SvcState::NotInstalled {
        // Non-service mode: check child process AND port/any process
        let running = match tab {
            Tab::Server => process::is_server_running() || process::is_server_port_open(),
            Tab::Daemon => process::is_daemon_running() || process::is_daemon_running_any(),
            _ => false,
        };
        if running {
            status_text = format!("{} Status: Process running", label);
            detail_text = format!(
                "Mode: Standalone process\nExecutable: {}",
                exe_path.display()
            );
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

    // For the Daemon tab, enrich detail with machine-id and available configs
    if matches!(tab, Tab::Daemon) {
        set_ctrl_text(lbl_detail, &format_daemon_detail(&detail_text));
        // Populate machine selector combobox
        populate_machine_combo(cmb_machine);
    }

    // Refresh log area
    let log_content = match tab {
        Tab::Server => log_viewer::tail_server(),
        Tab::Daemon => log_viewer::tail_daemon(),
        Tab::Logs | Tab::Config => String::new(),
    };
    set_ctrl_text(log_area, &log_content);
}
fn set_ctrl_text(handle: &nwg::ControlHandle, text: &str) {
    let hwnd = match handle.hwnd() {
        Some(h) => h as isize,
        None => return,
    };
    set_hwnd_text(hwnd, text);
}

/// Read text from a TextBox control via raw HWND.
fn get_ctrl_text(handle: &nwg::ControlHandle) -> String {
    let Some(hwnd) = handle.hwnd() else {
        return String::new();
    };
    let hwnd = hwnd as isize;
    if hwnd == 0 {
        return String::new();
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;
        let len = GetWindowTextLengthW(hwnd as _) as usize;
        if len == 0 {
            return String::new();
        }
        let mut buf: Vec<u16> = vec![0u16; len + 1];
        GetWindowTextW(hwnd as _, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..len])
    }
}

/// Show or hide a window by raw HWND.
fn show_window(hwnd: isize, visible: bool) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow;
        ShowWindow(
            hwnd as _,
            if visible {
                5 /* SW_SHOW */
            } else {
                0 /* SW_HIDE */
            },
        );
    }
}

/// Save raw TOML content to daemon.toml.
///
/// Validation rules (from knowledge base P0 bugs):
/// - Must contain a `[machine]` section.
/// - `serverUrl` must NOT use `http://` (causes all actors to go offline —
///   see [[2026-06-18 Daemon serverUrl 配置错误导致 Actor 离线]]).
/// - `serverUrl` should contain `/rpc` (daemon can auto-normalize, but
///   missing path is a warning).
fn save_daemon_toml(content: &str) -> Result<(), String> {
    if !content.contains("[machine]") {
        return Err("daemon.toml must contain a [machine] section".to_string());
    }
    // Reject http:// serverUrl — the P0 bug that caused all actors offline.
    if let Some(line) = content.lines().find(|l| {
        let trimmed = l.trim();
        trimmed.starts_with("serverUrl") || trimmed.starts_with("server_url")
    }) {
        let lower = line.to_lowercase();
        if lower.contains("http://") && !lower.contains("ws://") {
            return Err("serverUrl must use ws:// (or wss://), not http://.\n\
                 Using http:// causes ALL actors to go offline.\n\
                 Correct format: serverUrl = \"ws://127.0.0.1:7878/rpc\""
                .to_string());
        }
    }
    let Some(config_dir) = preflight::loom_config_dir() else {
        return Err("Cannot determine loom config directory".to_string());
    };
    let path = config_dir.join("daemon.toml");

    // Detect machine.id change (warns about zombie actors — see
    // [[2026-06-14 resolve_actor_alias 僵尸 Actor Bug]]).
    let id_change_warning = if path.exists() {
        if let Ok(old_data) = std::fs::read_to_string(&path) {
            let old_id = extract_toml_value(&old_data, "id");
            let new_id = extract_toml_value(content, "id");
            if old_id.is_some() && new_id.is_some() && old_id != new_id {
                Some(format!(
                    "WARNING: machine.id changed from \"{}\" to \"{}\".\n\
                     Old service actor (actor_service_{0}) will become a ZOMBIE on server.\n\
                     Use the admin panel on server to clean up old actors if needed.",
                    old_id.as_deref().unwrap_or("?"),
                    new_id.as_deref().unwrap_or("?")
                ))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let _ = std::fs::create_dir_all(&config_dir);
    std::fs::write(&path, content).map_err(|e| format!("Failed to write daemon.toml: {}", e))?;

    if let Some(warning) = id_change_warning {
        // Return success with machine-id change warning baked in.
        Err(format!("Saved successfully.\n\n{}", warning))
    } else {
        Ok(())
    }
}

/// Save raw TOML content to cli.toml (minimal validation: must have server_url).
fn save_cli_toml(content: &str) -> Result<(), String> {
    if !content.contains("server_url") {
        return Err("cli.toml must contain a server_url field".to_string());
    }
    let Some(config_dir) = preflight::loom_config_dir() else {
        return Err("Cannot determine loom config directory".to_string());
    };
    let _ = std::fs::create_dir_all(&config_dir);
    let path = config_dir.join("cli.toml");
    std::fs::write(&path, content).map_err(|e| format!("Failed to write cli.toml: {}", e))
}

/// Populate the config editor TextBox with daemon.toml or cli.toml content,
/// and show/hide the config controls based on the active tab.
fn populate_config_editor(
    tab: Tab,
    lbl_config: &nwg::ControlHandle,
    txt_config: &nwg::ControlHandle,
    btn_save_config: Option<&nwg::ControlHandle>,
) {
    let Some(config_dir) = preflight::loom_config_dir() else {
        return;
    };
    let (label, path) = match tab {
        Tab::Daemon => ("daemon.toml", config_dir.join("daemon.toml")),
        Tab::Config => ("cli.toml", config_dir.join("cli.toml")),
        _ => {
            // Hide config editor + save button for Server / Logs tabs
            let hwnd_lbl = extract_hwnd(lbl_config);
            let hwnd_txt = extract_hwnd(txt_config);
            show_window(hwnd_lbl, false);
            show_window(hwnd_txt, false);
            if let Some(btn) = btn_save_config {
                show_window(extract_hwnd(btn), false);
            }
            return;
        }
    };
    // Show controls
    show_window(extract_hwnd(lbl_config), true);
    show_window(extract_hwnd(txt_config), true);
    if let Some(btn) = btn_save_config {
        show_window(extract_hwnd(btn), true);
    }
    // Set label
    set_ctrl_text(
        lbl_config,
        &format!("  {label}  (editable — Save to apply)"),
    );
    // Load file content
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    set_ctrl_text(txt_config, &content);
}

/// Populate the machine selector ComboBox with available configs from daemon-configs/.
fn populate_machine_combo(handle: &nwg::ControlHandle) {
    let Some(hwnd) = handle.hwnd() else { return };
    populate_machine_combo_raw(hwnd as isize);
}

fn populate_machine_combo_raw(hwnd: isize) {
    // Clear existing items
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
        SendMessageW(hwnd as _, 0x014B /* CB_RESETCONTENT */, 0, 0);
    }

    let configs = list_machine_configs();
    if configs.is_empty() {
        // Add a placeholder item
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
            let text: Vec<u16> = "No machine configs \u{2014} register a host in GUI first"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            SendMessageW(
                hwnd as _,
                0x0143, /* CB_ADDSTRING */
                0,
                text.as_ptr() as _,
            );
        }
    } else {
        let active_id = process::machine_id_from_daemon_config();
        for (idx, (machine_id, display)) in configs.iter().enumerate() {
            let label = format!("{display} ({machine_id})");
            let label_wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
                SendMessageW(
                    hwnd as _,
                    0x0143, /* CB_ADDSTRING */
                    0,
                    label_wide.as_ptr() as _,
                );
            }
            // Select the active one
            if active_id.as_deref() == Some(machine_id.as_str()) {
                unsafe {
                    use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
                    SendMessageW(hwnd as _, 0x014E /* CB_SETCURSEL */, idx, 0);
                }
            }
        }
    }
}

/// Extract the raw HWND (isize) from a ControlHandle for cross-thread passing.
/// Returns 0 for an invalid handle.
fn extract_hwnd(handle: &nwg::ControlHandle) -> isize {
    handle.hwnd().map(|h| h as isize).unwrap_or(0)
}

/// Read the selected machine-id from the Daemon tab ComboBox (thread-safe, call
/// from any thread via raw HWND). Returns None if nothing is selected or the
/// placeholder item is active.
fn read_combo_machine_id(hwnd: isize) -> Option<String> {
    if hwnd == 0 {
        return None;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
        // CB_GETCURSEL — returns isize, CB_ERR = -1
        let idx = SendMessageW(hwnd as _, 0x0147, 0, 0);
        if idx == -1 {
            return None;
        }
        // CB_GETLBTEXTLEN — returns isize
        let len = SendMessageW(hwnd as _, 0x0149, idx as usize, 0);
        if len <= 0 {
            return None;
        }
        let len_usize = len as usize;
        let mut buf: Vec<u16> = vec![0u16; len_usize + 1];
        SendMessageW(hwnd as _, 0x0148, idx as usize, buf.as_mut_ptr() as _);
        let raw = String::from_utf16_lossy(&buf[..len_usize]);
        // Label format: "display_name (machine_id)"
        let machine_id = raw
            .rsplit_once(" (")
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .unwrap_or(&raw);
        if machine_id.is_empty() || machine_id.starts_with("No machine configs") {
            return None;
        }
        Some(machine_id.to_string())
    }
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
fn refresh_tab_raw(
    tab: Tab,
    lbl_status: isize,
    lbl_detail: isize,
    log_area: isize,
    cmb_machine: isize,
) {
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
        Tab::Config => {
            show_env_config_raw(lbl_status, lbl_detail, log_area);
            return;
        }
    };

    let svc_state = service::query(svc_name);
    let status_text: String;
    let detail_text: String;

    if svc_state == service::SvcState::NotInstalled {
        let running = match tab {
            Tab::Server => process::is_server_running() || process::is_server_port_open(),
            Tab::Daemon => process::is_daemon_running() || process::is_daemon_running_any(),
            _ => false,
        };
        if running {
            status_text = format!("{} Status: Process running", label);
            detail_text = format!(
                "Mode: Standalone process\nExecutable: {}",
                exe_path.display()
            );
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

    // For the Daemon tab, enrich detail with machine-id and available configs
    if matches!(tab, Tab::Daemon) {
        set_hwnd_text(lbl_detail, &format_daemon_detail(&detail_text));
        populate_machine_combo_raw(cmb_machine);
    }

    let log_content = match tab {
        Tab::Server => log_viewer::tail_server(),
        Tab::Daemon => log_viewer::tail_daemon(),
        Tab::Logs | Tab::Config => String::new(),
    };
    set_hwnd_text(log_area, &log_content);
}

/// Raw-HWND variant of populate_config_editor — callable from background threads.
fn populate_config_editor_raw(tab: Tab, lbl_config: isize, txt_config: isize) {
    let Some(config_dir) = preflight::loom_config_dir() else {
        return;
    };
    let (label, path) = match tab {
        Tab::Daemon => ("daemon.toml", config_dir.join("daemon.toml")),
        Tab::Config => ("cli.toml", config_dir.join("cli.toml")),
        _ => {
            show_window(lbl_config, false);
            show_window(txt_config, false);
            return;
        }
    };
    show_window(lbl_config, true);
    show_window(txt_config, true);
    set_hwnd_text(
        lbl_config,
        &format!("  {label}  (editable — Save to apply)"),
    );
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    set_hwnd_text(txt_config, &content);
}

/// Format the user-env.json snapshot for display in the Config tab.
fn format_env_config() -> String {
    let env = load_env_snapshot();

    if env.is_empty() {
        return String::from(
            "No user-env.json snapshot found.\r\n\
             This file is created automatically before UAC elevation to preserve\r\n\
             the user's original PATH and environment.\r\n\
             \r\n\
             Start loom-shell from a non-admin context to generate it.",
        );
    }

    let mut out = String::from("=== User Environment Snapshot (user-env.json) ===\r\n\r\n");

    // Show PATH first (most important), split each entry onto its own line.
    if let Some(path) = env.get("PATH") {
        out.push_str("── PATH ──────────────────────────────────────────\r\n");
        let entries: Vec<&str> = path.split(';').collect();
        for (i, entry) in entries.iter().enumerate() {
            let icon = if entry.is_empty() { " " } else { ">" };
            out.push_str(&format!("  {}  [{}] {}\r\n", icon, i, entry));
        }
        out.push_str("\r\n");
    }

    // Show other vars in a compact table.
    let mut sorted: Vec<&str> = env
        .keys()
        .filter(|k| *k != "PATH")
        .map(|k| k.as_str())
        .collect();
    if !sorted.is_empty() {
        out.push_str("── Other Variables ───────────────────────────────\r\n");
        sorted.sort();
        for key in &sorted {
            if let Some(val) = env.get(*key) {
                out.push_str(&format!("  {} = {}\r\n", key, val));
            }
        }
    }

    out
}

/// Load the user-env.json snapshot from the standard locations.
fn load_env_snapshot() -> std::collections::HashMap<String, String> {
    let candidates = env_json_candidates();
    for path in &candidates {
        let Ok(data) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) else {
            continue;
        };
        let mut map = std::collections::HashMap::new();
        if let Some(obj) = json.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    if !s.is_empty() {
                        map.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
        if !map.is_empty() {
            return map;
        }
    }
    std::collections::HashMap::new()
}

/// Candidate paths for user-env.json.
fn env_json_candidates() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
        v.push(
            std::path::PathBuf::from(&appdata)
                .join("loom")
                .join("user-env.json"),
        );
    }
    if let Ok(up) = std::env::var("USERPROFILE") {
        v.push(
            std::path::PathBuf::from(&up)
                .join("AppData")
                .join("Local")
                .join("loom")
                .join("user-env.json"),
        );
    }
    v
}

/// Show environment configuration in the UI (ControlHandle version).
fn show_env_config(
    lbl_status: &nwg::ControlHandle,
    lbl_detail: &nwg::ControlHandle,
    log_area: &nwg::ControlHandle,
) {
    set_ctrl_text(lbl_status, "Config: Environment Variables");
    set_ctrl_text(
        lbl_detail,
        "User environment snapshot saved before UAC elevation. Click Config tab to refresh.",
    );
    set_ctrl_text(log_area, &format_env_config());
}

/// Show environment configuration in the UI (raw HWND version).
fn show_env_config_raw(lbl_status: isize, lbl_detail: isize, log_area: isize) {
    set_hwnd_text(lbl_status, "Config: Environment Variables");
    set_hwnd_text(
        lbl_detail,
        "User environment snapshot saved before UAC elevation. Click Config tab to refresh.",
    );
    set_hwnd_text(log_area, &format_env_config());
}

// ---------------------------------------------------------------------------
// Action handlers
// ---------------------------------------------------------------------------

fn handle_start(tab: Tab, machine_id: Option<&str>) -> Result<(), String> {
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
            // If a machine-id is given (from the ComboBox selector), write it
            // to daemon.toml so the daemon starts with the right machine config.
            if let Some(mid) = machine_id {
                let Some(config_dir) = preflight::loom_config_dir() else {
                    return Err("Cannot determine loom config directory".to_string());
                };
                let config_path = config_dir.join("daemon.toml");
                if let Some(parent) = config_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let content = format!("[machine]\nid = \"{}\"\n", mid.replace('"', "\\\""));
                std::fs::write(&config_path, &content)
                    .map_err(|e| format!("Failed to write daemon.toml: {}", e))?;
            }

            // Run pre-flight checks before starting the daemon.
            // If there are errors (e.g. copilot not found), block the start
            // and return a diagnostic message so the user knows what's wrong.
            let checks = preflight::run_checks();
            let errors: Vec<_> = checks
                .iter()
                .filter(|c| c.status == preflight::CheckStatus::Error)
                .collect();
            if !errors.is_empty() {
                let mut msg = String::from("Pre-flight checks failed:\r\n");
                for c in &checks {
                    msg.push_str(&format!(
                        "  {} {}: {}\r\n",
                        c.status.icon(),
                        c.label,
                        c.detail.replace('\n', "\r\n    ")
                    ));
                }
                return Err(msg);
            }

            let svc_state = service::query("LoomDaemon");
            if svc_state == service::SvcState::NotInstalled {
                process::start_daemon(&config::daemon_exe().to_string_lossy())?;
            } else {
                service::start("LoomDaemon")?;
            }
        }
        Tab::Logs | Tab::Config => {}
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
        Tab::Logs | Tab::Config => {}
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
        Tab::Logs | Tab::Config => {}
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
        Tab::Logs | Tab::Config => {}
    }
    Ok(())
}

/// Delete a machine config directory from daemon-configs/.
fn handle_discard_machine(machine_id: &str) -> Result<String, String> {
    let Some(config_dir) = preflight::loom_config_dir() else {
        return Err("Cannot determine loom config directory".to_string());
    };
    let dir = config_dir.join("daemon-configs").join(machine_id);
    if !dir.exists() {
        return Err(format!(
            "Config directory does not exist: {}",
            dir.display()
        ));
    }
    std::fs::remove_dir_all(&dir)
        .map_err(|e| format!("Failed to remove {}: {}", dir.display(), e))?;
    Ok(format!(
        "Discarded machine config: {}  (directory removed)",
        machine_id
    ))
}

/// Scan the server for zombie machine actors (registered but no local
/// daemon-configs/ directory) and delete them via the loom CLI.
fn handle_clean_zombie_actors() -> Result<String, String> {
    // Read server URL from cli.toml
    let Some(config_dir) = preflight::loom_config_dir() else {
        return Err("Cannot determine loom config directory".to_string());
    };
    let cli_toml_path = config_dir.join("cli.toml");
    let server_url = if cli_toml_path.exists() {
        let content = std::fs::read_to_string(&cli_toml_path)
            .map_err(|e| format!("Failed to read cli.toml: {}", e))?;
        extract_direct_value(&content, "server_url")
            .unwrap_or_else(|| "ws://127.0.0.1:7878/rpc".to_string())
    } else {
        "ws://127.0.0.1:7878/rpc".to_string()
    };

    // Determine loom.exe path (same dir as loom-shell.exe)
    let exe_path = std::env::current_exe().map_err(|e| format!("Cannot get exe path: {}", e))?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| "Cannot determine exe directory".to_string())?;
    let loom_path = exe_dir.join("loom.exe");
    if !loom_path.exists() {
        return Err(format!(
            "loom.exe not found at {}. Build loom-cli first.",
            loom_path.display()
        ));
    }

    // 1. List all actors on server
    let mut list_cmd = loom_platform::process::Command::new(&loom_path);
    list_cmd
        .arg("--json")
        .arg("--server")
        .arg(&server_url)
        .arg("actor")
        .arg("list");
    let output = list_cmd
        .output()
        .map_err(|e| format!("Failed to run loom actor list: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("loom actor list failed: {}", stderr));
    }

    // 2. Parse JSON to find machine service actors
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse actor list: {}", e))?;
    let actors = parsed
        .get("actors")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Unexpected actor list format".to_string())?;

    // Collect machine service actors
    let mut zombie_ids: Vec<String> = Vec::new();
    let daemon_configs_dir = config_dir.join("daemon-configs");

    for actor in actors {
        let Some(kind) = actor.get("kind").and_then(|v| v.as_str()) else {
            continue;
        };
        if kind != "service" {
            continue;
        }
        let Some(actor_id) = actor.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        // Extract machine_id from actor id: "actor_service_<machine_id>"
        let Some(machine_id) = actor_id.strip_prefix("actor_service_") else {
            continue;
        };
        if machine_id.is_empty() {
            continue;
        }

        // Zombie check: machine service actor whose machine_id has no
        // local daemon-configs/ directory
        if !daemon_configs_dir.join(machine_id).exists() {
            zombie_ids.push(actor_id.to_string());
        }
    }

    if zombie_ids.is_empty() {
        return Ok("No zombie actors found on server.".to_string());
    }

    // 3. Delete each zombie actor
    let mut deleted: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for actor_id in &zombie_ids {
        let mut del_cmd = loom_platform::process::Command::new(&loom_path);
        del_cmd
            .arg("--server")
            .arg(&server_url)
            .arg("actor")
            .arg("delete")
            .arg(actor_id);
        let output = del_cmd
            .output()
            .map_err(|e| format!("Failed to run actor delete: {}", e))?;
        if output.status.success() {
            deleted.push(actor_id.clone());
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            errors.push(format!("  {}: {}", actor_id, stderr.trim()));
        }
    }

    let mut msg = format!("Cleaned {} zombie actor(s) from server:\n", deleted.len());
    for id in &deleted {
        msg.push_str(&format!("  - deleted {}\n", id));
    }
    if !errors.is_empty() {
        msg.push_str("\nErrors:\n");
        for err in &errors {
            msg.push_str(&format!("{}\n", err));
        }
    }
    Ok(msg)
}
