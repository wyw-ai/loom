//! Context Layer Core — the shared interface crate for pluggable context
//! resources.
//!
//! This crate defines the AOP interface (the "agentcontext" concept) that
//! lets plugins declare how per-turn prompt context is assembled from
//! multiple sources. Loom core and plugin crates both depend on this crate
//! — it is the **only** coupling point between loom and plugins.
//!
//! Design basis: ARCH v2 §A1, MCP Resource primitive, LlamaIndex
//! BaseMemoryBlock.priority.
//!
//! # Constraints
//!
//! - C-3/C-6: Resources MUST NOT perform semantic compression, keyword
//!   extraction, or content truncation. They may only read persisted data,
//!   format it into PromptSections, or skip themselves if budget is
//!   insufficient.
//! - C-4: agentcontext.yml has no `skills` field in D2.

use std::path::Path;

use anyhow::Result;
use proto::types::{ScopeKind, ScopeRef};

// ---------------------------------------------------------------------------
// PromptSection
// ---------------------------------------------------------------------------

/// A named, rendered section of the prompt envelope.
///
/// `name` is a static label (e.g. "warm_summary", "delivery_context").
/// `content` is the rendered text inserted into the prompt.
#[derive(Debug, Clone)]
pub struct PromptSection {
    pub name: &'static str,
    pub content: String,
}

// ---------------------------------------------------------------------------
// ContextResource trait
// ---------------------------------------------------------------------------

/// A pluggable context resource that contributes PromptSections to the
/// per-turn prompt envelope. Resources are assembled in priority order
/// within the remaining token budget.
///
/// This trait is the AOP interface Founder described as "agentcontext" —
/// loom core defines the trait; concrete providers are optional.
///
/// Plugins implement this trait and self-register via
/// [`ContextResourcePlugin`] + `inventory::submit!`.
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
// Plugin Registration (inventory)
// ---------------------------------------------------------------------------

/// A plugin's self-registration entry.
///
/// Plugins use `inventory::submit!` to register themselves at compile time.
/// Loom discovers them at runtime via `inventory::iter::<ContextResourcePlugin>`
/// without knowing the plugin's concrete types.
pub struct ContextResourcePlugin {
    /// URI scheme this plugin handles (e.g. "warm-summary", "message-list").
    pub scheme: &'static str,
    /// Factory function that creates a new instance of the resource.
    pub factory: fn() -> Box<dyn ContextResource>,
}

/// Enable inventory collection of ContextResourcePlugin entries.
inventory::collect!(ContextResourcePlugin);

// ---------------------------------------------------------------------------
// Token Estimation
// ---------------------------------------------------------------------------

/// Estimate token count for a text string.
///
/// Uses a mixed heuristic: ASCII runs are counted at ~4 chars/token, while
/// each non-ASCII (e.g. CJK) character counts as one token. This matches
/// the behavior of the original `agent_runtime::usage::estimate_tokens`.
///
/// Moved here so plugins can use it without depending on agent-runtime.
pub fn estimate_tokens(text: &str) -> u64 {
    let mut total = 0u64;
    let mut ascii_run = 0u64;

    for ch in text.chars() {
        if ch.is_ascii() && !ch.is_ascii_whitespace() {
            ascii_run += 1;
            continue;
        }

        if ascii_run > 0 {
            total += ascii_run.div_ceil(4);
            ascii_run = 0;
        }

        if !ch.is_whitespace() {
            total += 1;
        }
    }

    if ascii_run > 0 {
        total += ascii_run.div_ceil(4);
    }

    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_ascii_text() {
        // "hello world" = 11 non-whitespace ASCII chars → ceil(11/4) = 3 tokens
        assert_eq!(estimate_tokens("hello world"), 3);
    }

    #[test]
    fn estimate_tokens_empty_string() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_cjk_text() {
        // Each CJK char = 1 token
        assert_eq!(estimate_tokens("你好世界"), 4);
    }

    #[test]
    fn prompt_section_clone() {
        let s = PromptSection {
            name: "test",
            content: "hello".into(),
        };
        let s2 = s.clone();
        assert_eq!(s2.name, "test");
        assert_eq!(s2.content, "hello");
    }
}
