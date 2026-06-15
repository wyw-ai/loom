//! 服务安装/卸载 — 调用 WinSW-x64.exe 安装或卸载 Windows 服务。

use crate::config;
use std::process::Command;

/// 安装 loom-server 服务（调用 WinSW install）。
pub fn install_server() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-server.xml");

    if !winsw.exists() {
        return Err(format!("WinSW 未找到: {}", winsw.display()));
    }
    if !xml.exists() {
        return Err(format!("服务配置文件未找到: {}", xml.display()));
    }

    let output = Command::new(&winsw)
        .arg("install")
        .arg(&xml)
        .output()
        .map_err(|e| format!("执行 install 失败: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// 安装 loom-daemon 服务。
pub fn install_daemon() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-daemon.xml");

    if !winsw.exists() {
        return Err(format!("WinSW 未找到: {}", winsw.display()));
    }
    if !xml.exists() {
        return Err(format!("服务配置文件未找到: {}", xml.display()));
    }

    let output = Command::new(&winsw)
        .arg("install")
        .arg(&xml)
        .output()
        .map_err(|e| format!("执行 install 失败: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// 卸载 loom-server 服务。
pub fn uninstall_server() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-server.xml");
    let output = Command::new(&winsw)
        .arg("uninstall")
        .arg(&xml)
        .output()
        .map_err(|e| format!("执行 uninstall 失败: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// 卸载 loom-daemon 服务。
pub fn uninstall_daemon() -> Result<String, String> {
    let winsw = config::winsw_exe();
    let xml = config::config_dir().join("loom-daemon.xml");
    let output = Command::new(&winsw)
        .arg("uninstall")
        .arg(&xml)
        .output()
        .map_err(|e| format!("执行 uninstall 失败: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
