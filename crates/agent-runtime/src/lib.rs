//! Transport-agnostic agent runtime.
//!
//! This crate is the common code that drives ACP children, command-style
//! one-shot CLIs, and (eventually) other agent transports. It is consumed by:
//!   * `joi-server` — for the embedded supervisor (legacy/v0 mode);
//!   * `joi-cli` — for daemon-managed local agent workers.
//!
//! Both consumers see the same `Adapter` trait and `AdapterEvent` stream; the
//! decision of how the events become store mutations (direct vs. RPC) lives
//! one level up.

pub mod acp;
pub mod adapter;
pub mod agents_md;
pub mod bundle;
pub mod command;
pub mod discovery;
pub mod envelope;
pub mod interactive;
pub mod mcp_servers;
pub mod memory;
pub mod profile;
pub mod server_url;

pub use adapter::{
    ActionChoice, Adapter, AdapterEvent, AdapterModelChoice, AdapterModelOptions, AdapterPrompt,
    AdapterStartInfo,
};
pub use agents_md::ensure_agents_md;
pub use bundle::{
    prepare_bundle_install, resolved_bundle_version, validate_bundle_current, PreparedBundleInstall,
};
pub use envelope::{compose_prompt, EnvelopeInput, PromptSection};
pub use interactive::{InteractiveCommandAdapter, InteractiveCommandConfig};
pub use mcp_servers::build_mcp_servers;
pub use profile::{ensure_profile_scaffold, ProfileScaffold};
pub use server_url::{agent_child_server_url, local_agent_child_server_url};
