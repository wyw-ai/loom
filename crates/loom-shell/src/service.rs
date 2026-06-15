//! Windows 服务管理 — 通过 windows-sys SCM API 查询/控制服务状态。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
    SC_MANAGER_ALL_ACCESS, SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS,
    SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS, SERVICE_STOP, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, StartServiceW,
};

/// 将 Rust 字符串转为 Windows 宽字符串（以 null 结尾的 Vec<u16>）。
fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// 服务状态枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvcState {
    Running,
    Stopped,
    StopPending,
    Unknown,
    NotInstalled,
}

impl SvcState {
    pub fn label(self) -> &'static str {
        match self {
            SvcState::Running => "运行中",
            SvcState::Stopped => "已停止",
            SvcState::StopPending => "停止中…",
            SvcState::Unknown => "未知",
            SvcState::NotInstalled => "未安装",
        }
    }
}

/// 查询指定服务的当前状态。
/// `service_name` 是服务的短名称（如 "LoomServer"）。
pub fn query(service_name: &str) -> SvcState {
    let wide_name = to_wide(service_name);
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return SvcState::Unknown;
        }

        let svc = OpenServiceW(scm, wide_name.as_ptr(), SERVICE_QUERY_STATUS);
        if svc.is_null() {
            CloseServiceHandle(scm);
            return SvcState::NotInstalled;
        }

        let mut status: SERVICE_STATUS = std::mem::zeroed();
        let ok = QueryServiceStatus(svc, &mut status);
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);

        if ok == 0 {
            return SvcState::Unknown;
        }

        match status.dwCurrentState {
            SERVICE_RUNNING => SvcState::Running,
            SERVICE_STOPPED => SvcState::Stopped,
            SERVICE_STOP_PENDING => SvcState::StopPending,
            _ => SvcState::Unknown,
        }
    }
}

/// 启动指定服务。
pub fn start(service_name: &str) -> Result<(), String> {
    let wide_name = to_wide(service_name);
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            return Err("无法打开 SCM".into());
        }
        let svc = OpenServiceW(scm, wide_name.as_ptr(), SERVICE_START);
        if svc.is_null() {
            CloseServiceHandle(scm);
            return Err("无法打开服务".into());
        }
        let ok = StartServiceW(svc, 0, std::ptr::null());
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        if ok == 0 {
            Err("启动服务失败".into())
        } else {
            Ok(())
        }
    }
}

/// 停止指定服务。
pub fn stop(service_name: &str) -> Result<(), String> {
    let wide_name = to_wide(service_name);
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            return Err("无法打开 SCM".into());
        }
        let svc = OpenServiceW(scm, wide_name.as_ptr(), SERVICE_QUERY_STATUS | SERVICE_STOP);
        if svc.is_null() {
            CloseServiceHandle(scm);
            return Err("无法打开服务".into());
        }
        let mut status: SERVICE_STATUS = std::mem::zeroed();
        let ok = ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        if ok == 0 {
            Err("停止服务失败".into())
        } else {
            Ok(())
        }
    }
}
