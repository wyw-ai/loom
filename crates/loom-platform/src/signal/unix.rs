//! Unix backend for `loom_platform::signal`.
//!
//! Delivers signals via `libc::kill(pid, signum)`. All four [`Signal`]
//! variants map to their conventional POSIX numbers; see the module-level
//! table in `mod.rs` for the full mapping.
//!
//! ## Process-group semantics
//!
//! Children spawned through [`crate::process::Command`] /
//! [`crate::process::TokioCommand`] are placed in their own process group
//! (`process_group(0)` → `setpgid(0, 0)`, see `process/unix.rs`); the
//! child's PID is therefore also its process-group ID (pgid). To match the
//! Windows `CTRL_BREAK_EVENT` path and to reach the whole subtree
//! (grandchildren, orphaned `sleep` holding the stdout pipe, etc.), we
//! deliver the signal to the *process group* via `kill(-pgid, sig)` first,
//! and fall back to a direct `kill(pid, sig)` only if the group-targeted
//! call fails. The fallback covers targets that were not spawned through
//! our `process` module and therefore do not own a process group.

use super::Signal;

fn signum(sig: Signal) -> libc::c_int {
    match sig {
        Signal::Term => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
        Signal::Interrupt => libc::SIGINT,
        Signal::Quit => libc::SIGQUIT,
    }
}

pub(super) fn signal_child(pid: u32, sig: Signal) -> std::io::Result<()> {
    // SAFETY: `libc::kill` is a thin syscall wrapper that takes scalar
    // arguments only; no Rust aliasing or lifetime invariant is involved.
    //
    // We target the whole process group first (`-pid`): children spawned
    // via `loom_platform::process` are their own pgid leader, so `-pid`
    // reaches the leader plus every descendant that inherited the group
    // (e.g. a `sh -c 'sleep 30'` child whose `sleep` keeps the stdout
    // pipe open). If the group-targeted call fails — most commonly ESRCH
    // when the group has already exited, or ESRCH/EPERM on a target that
    // was not spawned through our `process` module and thus has no group
    // of its own — we retry against the bare PID so a non-grouped target
    // is still signalled. If both fail we surface the group-kill's errno
    // (it is the more meaningful of the two for our callers).
    let pid = pid as libc::pid_t;
    let sig_num = signum(sig);
    let group_rc = unsafe { libc::kill(-pid, sig_num) };
    if group_rc == 0 {
        return Ok(());
    }
    let group_err = std::io::Error::last_os_error();

    let direct_rc = unsafe { libc::kill(pid, sig_num) };
    if direct_rc == 0 {
        Ok(())
    } else {
        // Discard direct_rc's errno; group_err is the more informative
        // failure for callers reasoning about process groups.
        let _ = std::io::Error::last_os_error();
        Err(group_err)
    }
}
