//! Process abstraction — `Command` / `TokioCommand` newtypes with platform
//! defaults applied automatically at construction time.
//!
//! ## What this module gives you
//!
//! - [`Command`] wraps [`std::process::Command`] and on construction:
//!   - **Windows**: ORs `CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB |
//!     CREATE_NEW_PROCESS_GROUP` into the spawn `creation_flags`. The first
//!     two prevent a stray console window and let the child escape the Tauri
//!     GUI's restrictive job object; the third puts the child in its own
//!     process group so `GenerateConsoleCtrlEvent` (ARCH D5) can target it.
//!   - **Unix**: calls `process_group(0)` so the child runs in its own
//!     process group (equivalent of `setsid` before exec); this makes
//!     `kill(-pgid, SIGTERM)` deliver to the whole tree.
//! - [`TokioCommand`] is the same thing for [`tokio::process::Command`].
//!
//! ## Migration map
//!
//! - The constants [`CREATE_NO_WINDOW`] / [`CREATE_BREAKAWAY_FROM_JOB`] used
//!   to live in `agent_runtime::path_util`. They now live here. The old
//!   path still re-exports them for backward compatibility through the
//!   P0-PAL-4 wave.
//! - Direct call sites that build `std::process::Command` and manually call
//!   `.creation_flags(...)` or `.process_group(0)` migrate to
//!   `loom_platform::process::Command::new(...)` in P0-PAL-4. The newtype
//!   applies the platform defaults so the caller can stop writing
//!   `#[cfg(windows)]` for them.

#![allow(missing_docs)]

use std::ffi::OsStr;
use std::path::Path;
use std::process::{ExitStatus, Output, Stdio};

#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
mod unix;

#[cfg(windows)]
pub use self::windows::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};

/// Cross-platform wrapper around [`std::process::Command`] that pre-applies
/// the platform-correct spawn defaults at construction time.
///
/// Use this in place of `std::process::Command::new(...)` everywhere that
/// loom spawns a child process; it removes the need for per-call-site
/// `#[cfg(windows)] { ... creation_flags ... }` blocks.
#[derive(Debug)]
pub struct Command(std::process::Command);

impl Command {
    /// Construct a new command for `program` with platform defaults already
    /// applied (Windows spawn flags / Unix process group).
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        let mut inner = std::process::Command::new(program);
        #[cfg(windows)]
        self::windows::apply_std_defaults(&mut inner);
        #[cfg(not(windows))]
        self::unix::apply_std_defaults(&mut inner);
        Self(inner)
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.0.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.0.args(args);
        self
    }

    pub fn env<K, V>(&mut self, key: K, val: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.0.env(key, val);
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.0.envs(vars);
        self
    }

    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Self {
        self.0.env_remove(key);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.0.env_clear();
        self
    }

    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Self {
        self.0.current_dir(dir);
        self
    }

    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.0.stdin(cfg);
        self
    }

    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.0.stdout(cfg);
        self
    }

    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.0.stderr(cfg);
        self
    }

    pub fn spawn(&mut self) -> std::io::Result<std::process::Child> {
        self.0.spawn()
    }

    pub fn output(&mut self) -> std::io::Result<Output> {
        self.0.output()
    }

    pub fn status(&mut self) -> std::io::Result<ExitStatus> {
        self.0.status()
    }

    /// Borrow the inner `std::process::Command` for read-only inspection.
    pub fn as_std(&self) -> &std::process::Command {
        &self.0
    }

    /// Mutably borrow the inner `std::process::Command` — escape hatch for
    /// rare cases (e.g. `std::os::windows::process::CommandExt` tweaks the
    /// newtype does not expose).
    pub fn as_std_mut(&mut self) -> &mut std::process::Command {
        &mut self.0
    }

    /// Consume the newtype and return the inner `std::process::Command`.
    pub fn into_inner(self) -> std::process::Command {
        self.0
    }
}

/// Cross-platform wrapper around [`tokio::process::Command`] that
/// pre-applies the platform-correct spawn defaults at construction time.
#[derive(Debug)]
pub struct TokioCommand(tokio::process::Command);

impl TokioCommand {
    /// Construct a new async command for `program` with platform defaults
    /// already applied (Windows spawn flags / Unix process group).
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        let mut inner = tokio::process::Command::new(program);
        #[cfg(windows)]
        self::windows::apply_tokio_defaults(&mut inner);
        #[cfg(not(windows))]
        self::unix::apply_tokio_defaults(&mut inner);
        Self(inner)
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.0.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.0.args(args);
        self
    }

    pub fn env<K, V>(&mut self, key: K, val: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.0.env(key, val);
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.0.envs(vars);
        self
    }

    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Self {
        self.0.env_remove(key);
        self
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.0.env_clear();
        self
    }

    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Self {
        self.0.current_dir(dir);
        self
    }

    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.0.stdin(cfg);
        self
    }

    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.0.stdout(cfg);
        self
    }

    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.0.stderr(cfg);
        self
    }

    pub fn kill_on_drop(&mut self, kill_on_drop: bool) -> &mut Self {
        self.0.kill_on_drop(kill_on_drop);
        self
    }

    pub fn spawn(&mut self) -> std::io::Result<tokio::process::Child> {
        self.0.spawn()
    }

    /// Borrow the inner `tokio::process::Command`.
    pub fn as_tokio(&self) -> &tokio::process::Command {
        &self.0
    }

    /// Mutably borrow the inner `tokio::process::Command` — escape hatch
    /// for rare cases the newtype does not expose.
    pub fn as_tokio_mut(&mut self) -> &mut tokio::process::Command {
        &mut self.0
    }

    /// Consume the newtype and return the inner `tokio::process::Command`.
    pub fn into_inner(self) -> tokio::process::Command {
        self.0
    }
}
