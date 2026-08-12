//! Context Layer — pluggable context resource system.
//!
//! Defines the AOP interface (the "agentcontext" concept) that lets agents
//! declare how per-turn prompt context is assembled from multiple sources.
//!
//! Design basis: ARCH v2 §A1, MCP Resource primitive, LlamaIndex
//! BaseMemoryBlock.priority. Loom core defines the traits; concrete
//! providers are optional (feature flag or runtime registration).
//!
//! # Constraints
//!
//! - C-3/C-6: Resources MUST NOT perform semantic compression, keyword
//!   extraction, or content truncation. They may only read persisted data,
//!   format it into PromptSections, or skip themselves if budget is
//!   insufficient.
//! - C-4: agentcontext.yml has no `skills` field in D2.

pub mod filesystem;
pub mod memory;
pub mod message_list;

use std::path::Path;

use anyhow::Result;
use proto::types::{ScopeKind, ScopeRef};

use crate::envelope::PromptSection;

// ---------------------------------------------------------------------------
// ContextResource trait
// ---------------------------------------------------------------------------

/// A pluggable context resource that contributes PromptSections to the
/// per-turn prompt envelope. Resources are assembled in priority order
/// within the remaining token budget.
///
/// This trait is the AOP interface Founder described as "agentcontext" —
/// loom core defines the trait; concrete providers are optional.
pub trait ContextResource: Send + Sync {
    /// URI scheme this resource handles (e.g. "message-list", "file",
    /// "memory"). Used by the registry to route agentcontext.yml
    /// declarations to the correct provider.
    fn scheme(&self) -> &str;

    /// Assembly priority. Lower = assembled first (higher importance).
    /// Resources exceeding budget are skipped in reverse priority order.
    /// 0 = never skipped (reserved for critical resources).
    fn priority(&self) -> i32;

    /// Which scope kinds this resource is effective in.
    /// A channel-only resource is skipped in thread scopes, and vice versa.
    fn effective_scope(&self) -> &[ScopeKind];

    /// Assemble this resource's contribution to the prompt.
    ///
    /// `ctx` provides scope metadata, remaining token budget, and
    /// thread context. Returns zero or more PromptSections.
    ///
    /// This method MUST NOT perform semantic compression, keyword
    /// extraction, or content truncation (C-3, C-6). It may only:
    ///   - Read already-persisted data (files, summaries, memory)
    ///   - Format it into PromptSections
    ///   - Skip itself if budget is insufficient
    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>>;
}

// ---------------------------------------------------------------------------
// AssemblyContext
// ---------------------------------------------------------------------------

/// Read-only context passed to [`ContextResource::assemble`].
/// Provides everything a resource needs without exposing mutable state.
pub struct AssemblyContext<'a> {
    /// The scope this turn runs in (thread or channel).
    pub scope: &'a ScopeRef,
    /// Channel id if available (None for channel-level scopes without a
    /// parent channel context).
    pub channel_id: Option<&'a str>,
    /// The actor id of the agent whose turn is being composed.
    pub actor_id: &'a str,
    /// Absolute path to this actor's profile directory.
    pub profile_dir: &'a Path,
    /// Remaining token budget after higher-priority resources consumed
    /// their share. Resources should check this before assembling large
    /// content.
    pub budget_remaining: u64,
    /// Total token budget for this turn (for fraction calculations).
    pub budget_total: u64,
    /// The delivery cursor context string (thread messages, inbox items).
    /// Available so resources like MessageListProvider can reference it
    /// without re-querying.
    pub delivery_context: &'a str,
    /// Whether this is the first turn in this scope.
    pub first_turn: bool,
}

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

            for section in resource_sections {
                let section_tokens = crate::usage::estimate_tokens(&section.content);

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
// Re-exports
// ---------------------------------------------------------------------------

pub use filesystem::FileSystemProvider;
pub use memory::MemoryProvider;
pub use message_list::MessageListProvider;
