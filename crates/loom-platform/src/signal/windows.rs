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
//! - **Interrupt** → `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid)` plus a
//!   short delivery-verification window. The target must own its own
//!   console process group (children spawned via
//!   `loom_platform::process::Command` satisfy this thanks to
//!   `CREATE_NEW_PROCESS_GROUP` in `apply_*_defaults`). We use
//!   `CTRL_BREAK_EVENT` rather than `CTRL_C_EVENT` because `CTRL_C_EVENT` is
//!   broadcast to every process sharing the sender's console and *ignores*
//!   the process-group id argument, whereas `CTRL_BREAK_EVENT` is the event
//!   that actually respects per-group delivery enabled by
//!   `CREATE_NEW_PROCESS_GROUP`. The Win32 contract is that
//!   `GenerateConsoleCtrlEvent` returning non-zero only means the event was
//!   *queued*, **not** that the target actually received it: when the child
//!   was started with `CREATE_NO_WINDOW` (no inherited console), the event
//!   has nowhere to land and the API still reports success, so a naive
//!   "Ok ⇒ done" path leaves the child running. We therefore wait up to
//!   `INTERRUPT_GRACE_MS` for the child to exit via
//!   `WaitForSingleObject`; if it doesn't, we force termination via
//!   `TerminateProcess` so the caller's "interrupt or kill" intent is
//!   never silently dropped. The same fallback also covers the case where
//!   `GenerateConsoleCtrlEvent` itself returns an error.

use super::Signal;
use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_TERMINATE,
};

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

/// Grace period (in milliseconds) we wait for a child to exit after
/// queuing `CTRL_BREAK_EVENT` before falling back to `TerminateProcess`.
///
/// `GenerateConsoleCtrlEvent` queues the event but never confirms
/// delivery: a child started with `CREATE_NO_WINDOW` has no console to
/// receive it, yet the API still returns non-zero. 250 ms is short
/// enough to keep tests and shutdown paths responsive (well under the
/// 5 s integration deadline) but long enough that a cooperative
/// well-behaved child started inside a real console group can run its
/// CTRL_BREAK handler and exit before we escalate. ARCH §4.
const INTERRUPT_GRACE_MS: u32 = 250;

pub(super) fn signal_child(pid: u32, sig: Signal) -> std::io::Result<()> {
    match sig {
        Signal::Term | Signal::Kill | Signal::Quit => terminate_via_handle(pid),
        Signal::Interrupt => interrupt_with_fallback(pid),
    }
}

/// `Signal::Interrupt` implementation: try `CTRL_BREAK_EVENT`, then verify
/// the child actually exited within `INTERRUPT_GRACE_MS`; otherwise force
/// termination via `TerminateProcess`.
///
/// Why the verification step is non-negotiable on Windows:
/// `GenerateConsoleCtrlEvent` returning non-zero only means the event was
/// queued, not that the target consumed it. A child spawned with
/// `CREATE_NO_WINDOW` (no inherited console) has no console to deliver the
/// event to, but the API still reports success — so an Ok-only return
/// would silently leave the child running. We therefore open the child
/// with `SYNCHRONIZE` and `WaitForSingleObject` for a short grace period,
/// escalating to `TerminateProcess` on timeout. The same fallback also
/// kicks in if `GenerateConsoleCtrlEvent` itself errors (e.g. target not
/// in our process group), preserving the caller's "make this stop" intent.
fn interrupt_with_fallback(pid: u32) -> std::io::Result<()> {
    // Step 1: queue CTRL_BREAK_EVENT. On error fall straight through to
    // forced termination so we keep the historical "never silently drop"
    // contract.
    if send_ctrl_break(pid).is_err() {
        return terminate_via_handle(pid);
    }

    // Step 2: open the child with SYNCHRONIZE so we can wait on it. If
    // the open fails (e.g. the child already exited cleanly in response
    // to CTRL_BREAK and was reaped), treat that as success — the
    // interrupt achieved its purpose.
    // SAFETY: `OpenProcess` is a scalar-arg syscall; we check the
    // returned handle for null before any further use and pair every
    // successful open with `CloseHandle`.
    let handle: HANDLE = unsafe { OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE, FALSE, pid) };
    if handle.is_null() {
        return Ok(());
    }

    // Step 3: short verification wait. If the child exits within the
    // grace window the interrupt did its job; otherwise we escalate.
    // SAFETY: `handle` is a valid kernel handle from `OpenProcess`
    // above; `WaitForSingleObject` accepts process handles for which
    // SYNCHRONIZE was granted.
    let wait = unsafe { WaitForSingleObject(handle, INTERRUPT_GRACE_MS) };
    // SAFETY: `handle` came from a successful `OpenProcess`; closing it
    // exactly once is required and safe.
    let _ = unsafe { CloseHandle(handle) };

    match wait {
        WAIT_OBJECT_0 => Ok(()),
        // Any non-signalled status — `WAIT_TIMEOUT` (event didn't land
        // or child ignored it) or `WAIT_FAILED` — escalates. The
        // caller's intent is "make this stop", and `terminate_via_handle`
        // is the strongest tool we have.
        _ => terminate_via_handle(pid),
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
