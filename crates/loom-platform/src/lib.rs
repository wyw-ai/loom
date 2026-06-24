//! # loom-platform
//!
//! Workspace-level Platform Abstraction Layer (PAL). See the crate README for
//! design rules and the module-by-module status table.
//!
//! ## Invariants
//!
//! - Every cross-platform difference in the Loom workspace either lives here
//!   or is documented as an explicit, justified residual `cfg` in the
//!   originating business crate (see PRD §3.1 and ARCH §7.1).
//! - Public signatures are **identical** on Windows and Unix. The
//!   workspace-level `cargo check` on all three OSes is the contract check.
//! - There is **no runtime polymorphism**: all platform dispatch happens at
//!   compile time via `#[cfg(...)]`. No `Box<dyn Platform>`, no trait
//!   objects in hot paths.

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

pub mod console;
pub mod env;
pub mod fs;
pub mod ipc;
pub mod path;
pub mod process;
pub mod signal;
pub mod time;
