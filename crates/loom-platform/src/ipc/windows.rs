//! Windows backend for `loom_platform::ipc`.
//!
//! Two responsibilities live here:
//!
//! 1. **Pipe name construction** — [`default_pipe_name`] builds the
//!    namespaced stem (e.g. `loom-daemon-S-1-5-21-…`). The `\\.\pipe\`
//!    prefix is added by `interprocess`'s `GenericNamespaced` resolver.
//!    The current user's SID is appended so multi-user RDP / Fast
//!    User Switching does not collide.
//!
//! 2. **Per-user DACL** — [`apply_security_options`] takes the
//!    cross-platform `ListenerOptions` and installs a
//!    [`SecurityDescriptor`] that grants `GENERIC_ALL` only to the
//!    *Owner* (the SID that creates the pipe, i.e. the daemon
//!    process). The SDDL string used is `D:P(A;;GA;;;OW)`:
//!    - `D:P`  — protected DACL, will not inherit ACEs from parent.
//!    - `A`    — allow ACE.
//!    - `GA`   — `GENERIC_ALL` access mask.
//!    - `OW`   — `OWNER` well-known SID alias (the creator).
//!
//!    **Why explicit DACL** rather than a NULL DACL: a NULL DACL
//!    grants `Everyone` full access, which means any local-logged-in
//!    user (or a low-IL sandbox process) could open the daemon
//!    pipe. The owner-only DACL keeps the pipe accessible to the
//!    daemon owner and rejects everyone else.
//!
//!    **Why SDDL** rather than building an absolute SD by hand: SDDL
//!    deserialisation goes through one well-tested Win32 entry
//!    point ([`ConvertStringSecurityDescriptorToSecurityDescriptorW`])
//!    which `interprocess`'s [`SecurityDescriptor::deserialize`]
//!    wraps. Hand-rolled `InitializeSecurityDescriptor` +
//!    `SetSecurityDescriptorDacl` chains carry many more failure
//!    modes.
//!
//! Feature-tree note: `windows-sys` requires `Win32_Security` and
//! `Win32_Security_Authorization` for `ConvertSidToStringSidW` and
//! `LocalFree` (used in the SID lookup below). These are added in
//! `Cargo.toml` rather than inlined as constants (unlike the
//! `SYNCHRONIZE` constant in `signal/windows.rs`) because the SID
//! converter needs the function symbols, not just integer values.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::ptr;

use interprocess::local_socket::ListenerOptions;
use interprocess::os::windows::local_socket::ListenerOptionsExt;
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::U16CString;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// SDDL fragment installed on every loom-platform IPC listener.
///
/// Reads as: protected (non-inheriting) DACL with one allow ACE
/// granting `GENERIC_ALL` to the *Owner* well-known alias. The
/// owner is the SID that creates the pipe, i.e. the daemon process.
const OWNER_ONLY_SDDL: &str = "D:P(A;;GA;;;OW)";

/// Build the namespaced pipe stem `<stem>-<sid>`.
pub(super) fn default_pipe_name(stem: &str) -> io::Result<String> {
    if stem.is_empty() || stem.contains('\\') || stem.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pipe stem must be non-empty and contain no '\\' or NUL",
        ));
    }
    let sid = current_user_sid_string()?;
    Ok(format!("{stem}-{sid}"))
}

/// Apply the owner-only DACL to a cross-platform [`ListenerOptions`].
pub(super) fn apply_security_options<'a>(
    opts: ListenerOptions<'a>,
) -> io::Result<ListenerOptions<'a>> {
    let sddl_wide = U16CString::from_str(OWNER_ONLY_SDDL)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let sd = SecurityDescriptor::deserialize(sddl_wide.as_ucstr())?;
    Ok(opts.security_descriptor(sd))
}

/// Look up the current process's user SID and return it in string
/// form (S-1-5-21-…).
fn current_user_sid_string() -> io::Result<String> {
    let token = open_current_process_token()?;
    let user_buf = query_token_user(&token)?;
    // SAFETY: `query_token_user` returns a buffer sized to hold a
    // `TOKEN_USER` followed by a SID; we read the `User.Sid`
    // pointer and pass it to `ConvertSidToStringSidW`.
    let sid_ptr = unsafe { (*(user_buf.as_ptr() as *const TOKEN_USER)).User.Sid };
    sid_to_string(sid_ptr)
}

struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: handle came from `OpenProcessToken`; matching
            // `CloseHandle` is documented.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn open_current_process_token() -> io::Result<TokenHandle> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: scalar arg syscall; `GetCurrentProcess` returns a
    // pseudo-handle that does not need closing.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if ok == 0 || token.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(TokenHandle(token))
}

fn query_token_user(token: &TokenHandle) -> io::Result<Vec<u8>> {
    let mut needed: u32 = 0;
    // First call: probe required size. The call is expected to
    // fail with `ERROR_INSUFFICIENT_BUFFER` and write the required
    // size into `needed`.
    // SAFETY: passing a NULL output buffer with size 0 is the
    // documented way to size-probe `GetTokenInformation`.
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut buf = vec![0u8; needed as usize];
    // SAFETY: `buf` is sized to `needed`; we pass the same length.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buf)
}

fn sid_to_string(sid: *mut core::ffi::c_void) -> io::Result<String> {
    let mut wide_ptr: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` came from a `TOKEN_USER` payload owned by the
    // caller-side buffer; `ConvertSidToStringSidW` allocates a
    // UTF-16 string on the local heap which we release below.
    let ok = unsafe { ConvertSidToStringSidW(sid as _, &mut wide_ptr) };
    if ok == 0 || wide_ptr.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pointer is non-null and points to a NUL-terminated
    // UTF-16 buffer owned by us until `LocalFree` runs.
    let s = unsafe { wide_string_from_ptr(wide_ptr) };
    // SAFETY: `ConvertSidToStringSidW` documents `LocalFree` as the
    // matching deallocator.
    unsafe {
        let _ = LocalFree(wide_ptr as _);
    }
    Ok(s.to_string_lossy().into_owned())
}

/// Read a NUL-terminated UTF-16 string from `ptr` into an `OsString`.
///
/// # Safety
/// `ptr` must point to a NUL-terminated `u16` sequence.
unsafe fn wide_string_from_ptr(ptr: *const u16) -> OsString {
    let mut len = 0;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    OsString::from_wide(slice)
}
