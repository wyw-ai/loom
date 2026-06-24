//! Windows Service management — query/control service status via windows-sys SCM API.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
    StartServiceW, SC_MANAGER_ALL_ACCESS, SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP,
    SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS, SERVICE_STOP,
    SERVICE_STOPPED, SERVICE_STOP_PENDING,
};

/// Convert a Rust string to a Windows wide string (null-terminated Vec<u16>).
fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Service state enum.
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
            SvcState::Running => "Running",
            SvcState::Stopped => "Stopped",
            SvcState::StopPending => "Stopping...",
            SvcState::Unknown => "Unknown",
            SvcState::NotInstalled => "Not installed",
        }
    }
}

/// Query the current state of the specified service.
/// `service_name` is the service short name (e.g. "LoomServer").
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

/// Start the specified service.
pub fn start(service_name: &str) -> Result<(), String> {
    let wide_name = to_wide(service_name);
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            return Err("Failed to open SCM".into());
        }
        let svc = OpenServiceW(scm, wide_name.as_ptr(), SERVICE_START);
        if svc.is_null() {
            CloseServiceHandle(scm);
            return Err("Failed to open service".into());
        }
        let ok = StartServiceW(svc, 0, std::ptr::null());
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        if ok == 0 {
            Err("Failed to start service".into())
        } else {
            Ok(())
        }
    }
}

/// Stop the specified service.
pub fn stop(service_name: &str) -> Result<(), String> {
    let wide_name = to_wide(service_name);
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            return Err("Failed to open SCM".into());
        }
        let svc = OpenServiceW(scm, wide_name.as_ptr(), SERVICE_QUERY_STATUS | SERVICE_STOP);
        if svc.is_null() {
            CloseServiceHandle(scm);
            return Err("Failed to open service".into());
        }
        let mut status: SERVICE_STATUS = std::mem::zeroed();
        let ok = ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        if ok == 0 {
            Err("Failed to stop service".into())
        } else {
            Ok(())
        }
    }
}
