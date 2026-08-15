//! Context Layer — pluggable context resource system (runtime container).
//!
//! This module re-exports the core trait and data types from
//! `context-layer-core` and provides the runtime registry, built-in
//! providers (memory, filesystem), and plugin discovery.
//!
//! Plugin providers (warm-summary, message-list) live in the
//! `loom-plugin-context-tier` crate and self-register via
//! `inventory::submit!`. Loom discovers them at runtime via
//! [`discover_plugins`] without knowing their concrete types.
//!
//! # Constraints
//!
//! - C-3/C-6: Resources MUST NOT perform semantic compression, keyword
//!   extraction, or content truncation. They may only read persisted data,
//!   format it into PromptSections, or skip themselves if budget is
//!   insufficient.
//! - C-4: agentcontext.yml has no `skills` field in D2.

pub mod builder;
pub mod filesystem;

// Force-link the loom-plugin-context-tier crate so its `inventory::submit!`
// registrations are not stripped by the linker. Without this, the
// plugin registrations would be dead-code eliminated because
// agent-runtime never references loom-plugin-context-tier's types directly.
extern crate loom_plugin_context_tier;

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use proto::types::ScopeRef;

// Re-export core types from context-layer-core so existing consumers
// (agent_serve.rs, tests) can keep using `agent_runtime::ContextResource`
// etc. without change.
pub use context_layer_core::{
    estimate_tokens, AssemblyContext, ContextResource, ContextResourcePlugin, PromptSection,
};

// ---------------------------------------------------------------------------
// ResourceProvider trait
// ---------------------------------------------------------------------------

/// A source of external resources that can be mounted into context.
/// ResourceProvider is the data-access layer; ContextResource is the
/// prompt-assembly layer. A single provider may back multiple resources.
pub trait ResourceProvider: Send + Sync {
    /// URI scheme this provider handles (e.g. "file", "obsidian", "sql").
    fn scheme(&self) -> &str;

    /// List available resources for a scope.
    fn list(&self, scope: &ScopeRef, profile_dir: &Path) -> Result<Vec<ResourceHandle>>;

    /// Read a specific resource by URI.
    fn read(&self, uri: &str, scope: &ScopeRef, profile_dir: &Path) -> Result<ResourceContent>;
}

/// Metadata about a resource available from a [`ResourceProvider`].
#[derive(Debug, Clone)]
pub struct ResourceHandle {
    pub uri: String,
    pub name: String,
    pub size_bytes: u64,
}

/// The content of a resource read from a [`ResourceProvider`].
#[derive(Debug, Clone)]
pub struct ResourceContent {
    pub uri: String,
    pub media_type: String,
    pub text: String,
}

// ---------------------------------------------------------------------------
// ContextResourceRegistry
// ---------------------------------------------------------------------------

/// Builds and holds the chain of ContextResources for a turn.
/// Resources are sorted by priority (ascending) before assembly.
pub struct ContextResourceRegistry {
    resources: Vec<Box<dyn ContextResource>>,
}

impl ContextResourceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
        }
    }

    /// Register a ContextResource. The resource is inserted in priority
    /// order (lower priority value = assembled first).
    pub fn register(&mut self, resource: Box<dyn ContextResource>) {
        let priority = resource.priority();
        let pos = self
            .resources
            .iter()
            .position(|r| r.priority() > priority)
            .unwrap_or(self.resources.len());
        self.resources.insert(pos, resource);
    }

    /// Returns true if the registry has no resources.
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Returns true if any registered resource uses the given scheme.
    pub fn has_scheme(&self, scheme: &str) -> bool {
        self.resources.iter().any(|r| r.scheme() == scheme)
    }

    /// Assemble all resources in priority order, applying the token budget
    /// waterfall. Resources that would exceed `budget_remaining` are skipped
    /// (not truncated, per C-3/C-6), unless their priority is 0 (always
    /// assembled).
    ///
    /// Returns the assembled PromptSections and the remaining budget.
    pub fn assemble_chain(
        &self,
        ctx: &AssemblyContext<'_>,
        mut budget_remaining: u64,
    ) -> (Vec<PromptSection>, u64) {
        let mut sections = Vec::new();

        for resource in &self.resources {
            // Check scope effectiveness.
            if !resource
                .effective_scope()
                .contains(&ctx.scope.kind)
            {
                continue;
            }

            let resource_sections = match resource.assemble(ctx) {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(
                        scheme = resource.scheme(),
                        %err,
                        "context resource assemble failed; skipping"
                    );
                    continue;
                }
            };

            // B5 zero-content guard: a registered plugin returning no
            // sections is almost always a config or registration problem
            // (wrong envelope key, unreadable source, silent skip inside
            // the plugin). Empty output is legal but invisible in the
            // assembled prompt, so surface it at warn level.
            if resource_sections.is_empty() {
                tracing::warn!(
                    scheme = resource.scheme(),
                    "context layer plugin supplied no sections (check config/registration)"
                );
            }

            for section in resource_sections {
                let section_tokens = context_layer_core::estimate_tokens(&section.content);

                // Priority 0 resources are always included regardless of budget.
                if resource.priority() != 0 && section_tokens > budget_remaining {
                    tracing::debug!(
                        name = section.name,
                        section_tokens,
                        budget_remaining,
                        "skipping context section: exceeds remaining budget (not truncating per C-3)"
                    );
                    continue;
                }

                budget_remaining = budget_remaining.saturating_sub(section_tokens);
                sections.push(section);
            }
        }

        (sections, budget_remaining)
    }
}

impl Default for ContextResourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Plugin Discovery (inventory)
// ---------------------------------------------------------------------------

/// Discover all plugins registered via `inventory::submit!`.
///
/// Returns a map of scheme → factory function. Loom uses this to populate
/// the resource factory map without knowing any plugin's concrete types.
///
/// Plugin crates (e.g. `loom-plugin-context-tier`) self-register at compile time;
/// this function traverses those registrations at runtime.
pub fn discover_plugins() -> HashMap<String, fn(&Option<serde_json::Value>) -> Box<dyn ContextResource>> {
    let mut map = HashMap::new();
    for plugin in inventory::iter::<ContextResourcePlugin> {
        map.insert(plugin.scheme.to_string(), plugin.factory);
    }
    map
}

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use builder::ContextResourceBuilder;
pub use filesystem::FileSystemProvider;

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use proto::types::ScopeKind;

    /// Helper: create a simple test resource with given priority and token size.
    fn make_resource(
        scheme: &'static str,
        priority: i32,
        token_estimate: usize,
    ) -> Box<dyn ContextResource> {
        let content = "a".repeat(token_estimate * 4); // ~4 chars per token
        ContextResourceBuilder::new(scheme)
            .priority(priority)
            .assemble(move |_| {
                Ok(vec![PromptSection::from_resource(
                    "test",
                    scheme,
                    content.clone(),
                )])
            })
            .build()
    }

    fn make_ctx<'a>(scope: &'a ScopeRef, budget_remaining: u64) -> AssemblyContext<'a> {
        AssemblyContext {
            scope,
            channel_id: None,
            actor_id: "test",
            profile_dir: Path::new("/tmp"),
            budget_remaining,
            budget_total: budget_remaining,
            delivery_context: "",
            turn_input: "",
            first_turn: false,
        }
    }

    // (d) assemble_chain budget waterfall verification
    #[test]
    fn assemble_chain_budget_waterfall_includes_within_budget() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let mut registry = ContextResourceRegistry::new();
        // Two resources: priority 1 (10 tokens) and priority 2 (10 tokens).
        registry.register(make_resource("low-priority", 1, 10));
        registry.register(make_resource("high-priority", 2, 10));

        let ctx = make_ctx(&scope, 100);
        let (sections, remaining) = registry.assemble_chain(&ctx, 100);
        // Both fit within budget.
        assert_eq!(sections.len(), 2);
        assert!(remaining < 100);
    }

    #[test]
    fn assemble_chain_budget_waterfall_skips_over_budget() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let mut registry = ContextResourceRegistry::new();
        // priority 1: 10 tokens (fits), priority 2: 1000 tokens (exceeds remaining).
        registry.register(make_resource("fits", 1, 10));
        registry.register(make_resource("too-big", 2, 1000));

        let ctx = make_ctx(&scope, 100);
        let (sections, _remaining) = registry.assemble_chain(&ctx, 100);
        // Only the first resource fits; the second is skipped (not truncated).
        assert_eq!(sections.len(), 1);
    }

    #[test]
    fn assemble_chain_priority_zero_always_included() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let mut registry = ContextResourceRegistry::new();
        // priority 0: always included even if over budget.
        registry.register(make_resource("critical", 0, 1000));

        let ctx = make_ctx(&scope, 10);
        let (sections, _remaining) = registry.assemble_chain(&ctx, 10);
        assert_eq!(sections.len(), 1, "priority 0 resource must always be included");
    }

    #[test]
    fn assemble_chain_respects_scope_filtering() {
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: "test".into(),
        };
        let mut registry = ContextResourceRegistry::new();
        // Thread-only resource should be skipped in Channel scope.
        registry.register(
            ContextResourceBuilder::new("thread-only")
                .priority(1)
                .scopes(vec![ScopeKind::Thread])
                .assemble(|_| {
                    Ok(vec![PromptSection::from_resource(
                        "should-not-appear",
                        "thread-only",
                        "data".into(),
                    )])
                })
                .build(),
        );

        let ctx = make_ctx(&scope, 100);
        let (sections, _) = registry.assemble_chain(&ctx, 100);
        assert!(sections.is_empty(), "thread-only resource should be skipped in channel scope");
    }

    #[test]
    fn discover_plugins_finds_registered_plugins() {
        let plugins = discover_plugins();
        // loom-plugin-context-tier crate registers warm-summary and message-list.
        assert!(
            plugins.contains_key("warm-summary"),
            "warm-summary plugin should be discovered via inventory"
        );
        assert!(
            plugins.contains_key("message-list"),
            "message-list plugin should be discovered via inventory"
        );
    }

    // (e) B5 zero-content guard: a plugin returning Ok(vec![]) is legal
    // but suspicious — the chain must keep flowing (no error, no phantom
    // section) while the guard makes the situation visible in logs.
    #[test]
    fn assemble_chain_empty_plugin_output_does_not_break_the_chain() {
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "test".into(),
        };
        let mut registry = ContextResourceRegistry::new();
        registry.register(
            ContextResourceBuilder::new("empty-plugin")
                .priority(1)
                .assemble(|_| Ok(vec![]))
                .build(),
        );
        registry.register(make_resource("healthy", 2, 10));

        let ctx = make_ctx(&scope, 100);
        let (sections, _) = registry.assemble_chain(&ctx, 100);
        assert_eq!(
            sections.len(),
            1,
            "empty plugin output yields no section; later resources still assemble"
        );
        // make_resource names its section "test" and carries the scheme
        // in the source; only the healthy resource's section survives.
        assert_eq!(sections[0].name, "test");
        assert_eq!(
            sections[0].source,
            context_layer_core::SectionSource::Resource { scheme: "healthy", uri: None }
        );
    }
}
