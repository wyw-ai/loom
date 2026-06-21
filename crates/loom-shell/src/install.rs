//! Service install/uninstall — invoke WinSW-x64.exe to install or remove Windows services.

use crate::config;
use loom_platform::process::Command;

/// Install the loom-server service (calls WinSW install).
pub fn install_server() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-server.xml");

    if !winsw.exists() {
        return Err(format!("WinSW not found: {}", winsw.display()));
    }
    if !xml.exists() {
        return Err(format!("Service config file not found: {}", xml.display()));
    }

    let mut cmd = Command::new(&winsw);
    cmd.arg("install").arg(&xml);
    let output = cmd
        .output()
        .map_err(|e| format!("Executing install failed: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Install the loom-daemon service.
pub fn install_daemon() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-daemon.xml");

    if !winsw.exists() {
        return Err(format!("WinSW not found: {}", winsw.display()));
    }
    if !xml.exists() {
        return Err(format!("Service config file not found: {}", xml.display()));
    }

    let mut cmd = Command::new(&winsw);
    cmd.arg("install").arg(&xml);
    let output = cmd
        .output()
        .map_err(|e| format!("Executing install failed: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Uninstall the loom-server service.
pub fn uninstall_server() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-server.xml");
    let mut cmd = Command::new(&winsw);
    cmd.arg("uninstall").arg(&xml);
    let output = cmd
        .output()
        .map_err(|e| format!("Executing uninstall failed: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Uninstall the loom-daemon service.
pub fn uninstall_daemon() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-daemon.xml");
    let mut cmd = Command::new(&winsw);
    cmd.arg("uninstall").arg(&xml);
    let output = cmd
        .output()
        .map_err(|e| format!("Executing uninstall failed: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
