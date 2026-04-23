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
pub mod agents_md;
pub mod command;
pub mod envelope;
pub mod mcp_servers;
pub mod memory;
pub mod profile;

pub use adapter::{ActionChoice, Adapter, AdapterEvent, AdapterStartInfo};
pub use agents_md::ensure_agents_md;
pub use envelope::{compose_prompt, EnvelopeInput, PromptSection};
pub use mcp_servers::build_mcp_servers;
pub use profile::{ensure_profile_scaffold, ProfileScaffold};
