//! Signal handling — child-process signalling.
//!
//! Provides a small, portable surface for delivering signals to a child
//! process identified by its PID. Public signatures are identical on
//! Windows and Unix (PRD §3.1, ARCH D5); platform-specific behaviour is
//! kept inside the `unix`/`windows` submodules.
//!
//! ## Mapping
//!
//! | [`Signal`] variant | Unix                          | Windows                                                              |
//! |--------------------|-------------------------------|----------------------------------------------------------------------|
//! | [`Signal::Term`]   | `kill(-pgid, SIGTERM)` (15)   | `OpenProcess(PROCESS_TERMINATE \| SYNCHRONIZE)` + `TerminateProcess(1)` |
//! | [`Signal::Kill`]   | `kill(-pgid, SIGKILL)` (9)    | `OpenProcess(PROCESS_TERMINATE \| SYNCHRONIZE)` + `TerminateProcess(1)` |
//! | [`Signal::Quit`]   | `kill(-pgid, SIGQUIT)` (3)    | `OpenProcess(PROCESS_TERMINATE \| SYNCHRONIZE)` + `TerminateProcess(1)` |
//! | [`Signal::Interrupt`] | `kill(-pgid, SIGINT)` (2)  | `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid)` with a short delivery-verification wait, then `TerminateProcess(1)` fallback if the child doesn't exit in time |
//!
//! Windows has no per-signal granularity for `TerminateProcess`; the three
//! "forced" variants therefore collapse to the same handle-based call.
//! The benefit of keeping them distinct in the API is that callers can
//! express *intent* (graceful vs. forced vs. quit-with-core) once, and the
//! Unix side honours it natively.
//!
//! ## Caller contract
//!
//! - Children spawned through [`crate::process::Command`] /
//!   [`crate::process::TokioCommand`] satisfy the [Windows process-group
//!   requirement] for `GenerateConsoleCtrlEvent` automatically
//!   (`CREATE_NEW_PROCESS_GROUP` is applied by `apply_*_defaults`). Sending
//!   [`Signal::Interrupt`] on Windows always *attempts* to behave like a
//!   `SIGINT`-with-kill-fallback: the Win32 backend first queues
//!   `CTRL_BREAK_EVENT`, waits a short grace period to verify the child
//!   actually exited (the `GenerateConsoleCtrlEvent` API returns success
//!   even when the event has nowhere to land — e.g. `CREATE_NO_WINDOW`
//!   children), and otherwise escalates to `TerminateProcess`. Callers
//!   therefore do not need a manual [`force_kill_pid`] fallback for the
//!   common case; they may still call it explicitly when they need to
//!   skip the grace window.
//! - On Unix the same process-group setup (`process_group(0)` →
//!   `setpgid(0, 0)`, see `process/unix.rs`) makes the child its own pgid
//!   leader, so the backend targets `kill(-pgid, sig)` to reach the whole
//!   subtree (leader + grandchildren holding the stdout pipe) and falls
//!   back to `kill(pid, sig)` only when the group-targeted call fails
//!   (e.g. the target was not spawned through our `process` module and
//!   owns no group). This mirrors the Windows `CTRL_BREAK_EVENT` reach.
//! - All functions are *fire-and-forget*: they do not wait for the child
//!   to exit. Wait on the owned `Child` separately when ordering matters.
//!
//! [Windows process-group requirement]: https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent

#![allow(clippy::needless_return)]

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// Cross-platform signal kind delivered by [`signal_child`].
///
/// Variants describe *intent*; see the module-level mapping table for what
/// each variant resolves to on a given OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    /// Graceful termination request. Unix: `SIGTERM`. Windows:
    /// `TerminateProcess` (Windows has no native equivalent of `SIGTERM`).
    Term,
    /// Unconditional kill. Unix: `SIGKILL`. Windows: `TerminateProcess`.
    Kill,
    /// Console interrupt (Ctrl-C). Unix: `SIGINT`. Windows:
    /// `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid)` (`CTRL_BREAK_EVENT`
    /// is the event that honours per-process-group delivery for a
    /// `CREATE_NEW_PROCESS_GROUP` child; `CTRL_C_EVENT` would broadcast to
    /// the whole console and ignore the group id), followed by a short
    /// delivery-verification wait and a `TerminateProcess` fallback if
    /// the child has not exited by then.
    Interrupt,
    /// Quit with optional core dump. Unix: `SIGQUIT`. Windows:
    /// `TerminateProcess` (no native equivalent).
    Quit,
}

/// Deliver `sig` to the process identified by `pid`.
///
/// Returns the underlying OS error on failure. Does not wait for the
/// target process to exit.
pub fn signal_child(pid: u32, sig: Signal) -> std::io::Result<()> {
    if pid == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "child pid must be greater than zero",
        ));
    }
    #[cfg(unix)]
    {
        return self::unix::signal_child(pid, sig);
    }
    #[cfg(windows)]
    {
        return self::windows::signal_child(pid, sig);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, sig);
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "signal_child: unsupported platform",
        ));
    }
}

/// Force-kill the process identified by `pid`.
///
/// Equivalent to `signal_child(pid, Signal::Kill)`; this is the dedicated
/// entry point used by `agent-runtime::acp::kill_process` (PAL-6) so the
/// last `cfg(windows)` branch in that file collapses to a single call.
pub fn force_kill_pid(pid: u32) -> std::io::Result<()> {
    signal_child(pid, Signal::Kill)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_variants_are_distinct() {
        assert_ne!(Signal::Term, Signal::Kill);
        assert_ne!(Signal::Kill, Signal::Interrupt);
        assert_ne!(Signal::Interrupt, Signal::Quit);
    }

    #[test]
    fn signal_is_copy_and_hashable() {
        use std::collections::HashSet;
        let mut s = HashSet::new();
        let a = Signal::Term;
        let b = a; // Copy
        s.insert(a);
        s.insert(b);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn signal_to_nonexistent_pid_returns_error() {
        // PID 0 is reserved on every supported platform (the "current
        // process group" / "swapper") and not a valid target for our
        // child-signal helpers. We should get an Err, not a panic or
        // success.
        let res = signal_child(0, Signal::Term);
        assert!(res.is_err(), "expected Err for pid=0, got {res:?}");
    }
}
