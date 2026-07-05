//! Transport-agnostic agent runtime.
//!
//! This crate is the common code that drives ACP children, command-style
//! one-shot CLIs, and (eventually) other agent transports. It is consumed by:
//!   * `loom-server` — for the embedded supervisor (legacy/v0 mode);
//!   * `loom-cli` — for daemon-managed local agent workers.
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
pub mod provider;
pub mod server_url;
pub mod tracing_setup;
pub mod usage;

pub use adapter::{
    ActionChoice, Adapter, AdapterEvent, AdapterModelChoice, AdapterModelOptions, AdapterPrompt,
    AdapterStartInfo, PromptPart, PromptRoleHint, TokenUsage,
};
pub use agents_md::{ensure_agents_md, AgentsMdContext, AgentsMdMember};
pub use bundle::{
    prepare_bundle_install, resolved_bundle_version, validate_bundle_current, PreparedBundleInstall,
};
pub use envelope::{compose_prompt, EnvelopeInput, PromptSection};
pub use interactive::{InteractiveCommandAdapter, InteractiveCommandConfig};
pub use mcp_servers::build_mcp_servers;
pub use profile::{ensure_profile_scaffold, ProfileScaffold};
pub use provider::ProviderRuntimeEvent;
pub use server_url::{agent_child_server_url, local_agent_child_server_url};

#[cfg(test)]
mod tests {
    #[test]
    fn provider_runtime_event_is_part_of_public_runtime_surface() {
        let event = crate::ProviderRuntimeEvent::Session {
            session_id: "sid_public".into(),
        };

        assert_eq!(event.into_session_id(), Some("sid_public".into()));
    }
}
