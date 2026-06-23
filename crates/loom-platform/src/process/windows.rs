//! Windows process spawn defaults.
//!
//! All three flags must be ORed together at every spawn call so the child:
//!
//! 1. does not pop a stray console window (`CREATE_NO_WINDOW`),
//! 2. can escape the Tauri GUI's restrictive job object
//!    (`CREATE_BREAKAWAY_FROM_JOB` — fixes `ERROR_PRIVILEGE_NOT_HELD`), and
//! 3. lives in its own console process group so
//!    `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT)` can target it
//!    (`CREATE_NEW_PROCESS_GROUP` — required by ARCH D5 signal design).

use std::os::windows::process::CommandExt;

/// Windows `CREATE_NO_WINDOW` (0x0800_0000) — prevents a console window
/// from appearing for the spawned child.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows `CREATE_BREAKAWAY_FROM_JOB` (0x0100_0000) — lets the child
/// escape a parent's job object. Required when spawning from the Tauri
/// GUI process which is itself job-wrapped.
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// Windows `CREATE_NEW_PROCESS_GROUP` (0x0000_0200) — puts the child in
/// its own console process group, so `GenerateConsoleCtrlEvent(CTRL_BREAK,
/// pid)` can deliver a controlled-shutdown signal (ARCH D5).
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// All three platform-default spawn flags, ORed together. This is the bit
/// pattern actually pushed onto every newtype command at construction.
pub(super) const PLATFORM_DEFAULT_FLAGS: u32 =
    CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP;

pub(super) fn apply_std_defaults(cmd: &mut std::process::Command) {
    cmd.creation_flags(PLATFORM_DEFAULT_FLAGS);
}

pub(super) fn apply_tokio_defaults(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(PLATFORM_DEFAULT_FLAGS);
}
