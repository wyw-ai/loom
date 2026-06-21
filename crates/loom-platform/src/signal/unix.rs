//! Unix backend for `loom_platform::signal`.
//!
//! Delivers signals via `libc::kill(pid, signum)`. All four [`Signal`]
//! variants map to their conventional POSIX numbers; see the module-level
//! table in `mod.rs` for the full mapping.

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
    // We treat any non-zero return as the OS error and surface it.
    let rc = unsafe { libc::kill(pid as libc::pid_t, signum(sig)) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
