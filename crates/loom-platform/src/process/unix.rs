//! Unix process spawn defaults.
//!
//! We put each spawned child in its own process group (`setpgid(0, 0)` via
//! the modern `Command::process_group(0)` API). This is the moral
//! equivalent of `setsid` before exec for our use-case: a later
//! `kill(-pgid, SIGTERM)` reaches the whole subtree, which matches the
//! Windows `CTRL_BREAK_EVENT` path in `loom_platform::signal` (ARCH D5).

pub(super) fn apply_std_defaults(cmd: &mut std::process::Command) {
    cmd.process_group(0);
}

pub(super) fn apply_tokio_defaults(cmd: &mut tokio::process::Command) {
    cmd.process_group(0);
}
