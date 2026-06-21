//! Process abstraction — `Command` / `TokioCommand` newtypes with platform
//! defaults applied automatically.
//!
//! Skeleton (P0-PAL-1). Real implementation lands in P0-PAL-3.
//!
//! Public API will be:
//!
//! ```ignore
//! pub struct Command(std::process::Command);
//! pub struct TokioCommand(tokio::process::Command);
//! impl Command {
//!     pub fn new<S: AsRef<OsStr>>(program: S) -> Self;
//!     pub fn into_inner(self) -> std::process::Command;
//!     // + delegating arg/args/env/current_dir/stdin/stdout/stderr/...
//! }
//! ```
//!
//! Internal split: `windows.rs` applies
//! `CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP`,
//! `unix.rs` applies `setsid` via `pre_exec`. Signal delivery (`signal_child`,
//! `force_kill_child`) migrates here in P0-PAL-6.
