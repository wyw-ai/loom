//! Windows backend for `loom_platform::signal`.
//!
//! There is no portable per-signal API on Windows. We split the four
//! [`Signal`] variants into two implementation paths:
//!
//! - **Term / Kill / Quit** → `OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE,
//!   FALSE, pid)` followed by `TerminateProcess(handle, 1)` and
//!   `CloseHandle`. `SYNCHRONIZE` is added (vs. PRD-original
//!   `PROCESS_TERMINATE` alone) so a follow-up `WaitForSingleObject` is
//!   permitted without a re-open (ARCH D5 risk hint).
//! - **Interrupt** → `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid)`. The
//!   target must own its own console process group (children spawned via
//!   `loom_platform::process::Command` satisfy this thanks to
//!   `CREATE_NEW_PROCESS_GROUP` in `apply_*_defaults`). We use
//!   `CTRL_BREAK_EVENT` rather than `CTRL_C_EVENT` because `CTRL_C_EVENT` is
//!   broadcast to every process sharing the sender's console and *ignores*
//!   the process-group id argument, whereas `CTRL_BREAK_EVENT` is the event
//!   that actually respects per-group delivery enabled by
//!   `CREATE_NEW_PROCESS_GROUP`. If the call fails, we fall back to the
//!   `TerminateProcess` path so the caller's "interrupt or kill" intent is
//!   never silently dropped.

use super::Signal;
use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE};
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

/// Standard `SYNCHRONIZE` access right (0x0010_0000) from `WinNT.h`.
///
/// windows-sys 0.59 only re-exports this constant under the
/// `Win32_Storage_FileSystem` feature, which we deliberately do not pull
/// in (it cascades a much larger surface). Inlining the bit pattern keeps
/// our feature footprint minimal while satisfying the ARCH D5 hint that
/// the handle be wait-capable for a future `WaitForSingleObject`.
const SYNCHRONIZE: u32 = 0x0010_0000;

/// Standardised exit code used when forcing a child to exit on Windows.
///
/// Matches the value historically embedded in
/// `agent-runtime::acp::kill_process` and chosen so callers can
/// distinguish PAL-induced termination (`1`) from a normal `0` exit.
const FORCED_EXIT_CODE: u32 = 1;

pub(super) fn signal_child(pid: u32, sig: Signal) -> std::io::Result<()> {
    match sig {
        Signal::Term | Signal::Kill | Signal::Quit => terminate_via_handle(pid),
        Signal::Interrupt => match send_ctrl_break(pid) {
            Ok(()) => Ok(()),
            // Fall back to forced termination — preserves the caller's
            // "make this stop" intent even when CTRL_BREAK_EVENT cannot be
            // delivered (target not in its own process group, etc.).
            Err(_) => terminate_via_handle(pid),
        },
    }
}

fn terminate_via_handle(pid: u32) -> std::io::Result<()> {
    // SAFETY: `OpenProcess` returns a kernel handle or null; we check
    // for null before any further use. `TerminateProcess` only requires
    // the handle came from `OpenProcess` with `PROCESS_TERMINATE`.
    // `CloseHandle` is always paired with a successful open.
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, FALSE, pid);
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let ok = TerminateProcess(handle, FORCED_EXIT_CODE);
        // Capture the error from TerminateProcess *before* CloseHandle,
        // since CloseHandle may overwrite the thread-local last-error.
        let term_err = if ok == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        let _ = CloseHandle(handle);
        match term_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

fn send_ctrl_break(pid: u32) -> std::io::Result<()> {
    // SAFETY: `GenerateConsoleCtrlEvent` is a scalar-arg syscall wrapper;
    // the only invariant we owe it is `pid` is meaningful as a console
    // process group ID. Children spawned via
    // `loom_platform::process::Command` satisfy this by virtue of the
    // `CREATE_NEW_PROCESS_GROUP` flag baked into `apply_*_defaults`.
    //
    // We send `CTRL_BREAK_EVENT` (not `CTRL_C_EVENT`): per the Win32
    // contract `CTRL_C_EVENT` is broadcast to every process sharing the
    // caller's console and ignores the process-group id argument, so it
    // would not target just our child group. `CTRL_BREAK_EVENT` is the
    // event that honours per-group delivery for a `CREATE_NEW_PROCESS_GROUP`
    // child.
    let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
