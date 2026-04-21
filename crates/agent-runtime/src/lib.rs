//! Transport-agnostic agent runtime.
//!
//! This crate is the common code that drives ACP children, command-style
//! one-shot CLIs, and (eventually) other agent transports. It is consumed by:
//!   * `joi-server` — for the embedded supervisor (legacy/v0 mode);
//!   * `joi-cli` — for the v1 `joi agent serve` external agent client.
//!
//! Both consumers see the same `Adapter` trait and `AdapterEvent` stream; the
//! decision of how the events become store mutations (direct vs. RPC) lives
//! one level up.

pub mod acp;
pub mod adapter;
pub mod command;

pub use adapter::{ActionChoice, Adapter, AdapterEvent, AdapterStartInfo};
