//! Context Tier plugin — Hot/Warm/Cold temperature model for Loom.
//!
//! This crate provides two ContextResources that self-register via
//! `inventory::submit!`:
//!
//! - **warm-summary** (priority 7): Reads persisted Warm summary files and
//!   injects them into the prompt envelope.
//! - **message-list** (priority 10): Wraps the pre-queried delivery cursor
//!   context as a PromptSection.
//!
//! Loom discovers these plugins at runtime via
//! `agent_runtime::context_layer::discover_plugins()` without knowing this
//! crate's concrete types (Founder principle: "plugin 不应该和 loom 内部代码耦合").

mod warm_summary;
mod message_list;

// Re-export the concrete types so loom (agent_serve.rs) can call static
// utility methods like WarmSummaryContextResource::clear/persist.
// These are NOT used for instantiation — instantiation goes through
// inventory self-registration.
pub use message_list::MessageListProvider;
pub use warm_summary::WarmSummaryContextResource;

// ── Self-Registration (inventory) ─────────────────────────────────────────
//
// These submissions happen at compile time. Loom discovers them via
// `inventory::iter::<ContextResourcePlugin>` without knowing these types.

inventory::submit! {
    context_layer_core::ContextResourcePlugin {
        scheme: "warm-summary",
        factory: |_config| Box::new(warm_summary::WarmSummaryContextResource::new()),
    }
}

inventory::submit! {
    context_layer_core::ContextResourcePlugin {
        scheme: "message-list",
        factory: |_config| Box::new(message_list::MessageListProvider::new()),
    }
}
