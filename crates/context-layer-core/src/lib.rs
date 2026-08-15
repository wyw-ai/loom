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
//! # Provenance (iter 1, R1.1)
//!
//! Every [`PromptSection`] carries a mandatory [`SectionSource`] recording
//! where its content came from. The three branches are exhaustive by
//! design — there is no "unspecified" state. Two `uri` exemptions
//! (memory, message-list) are deliberate and recorded in the ARCH design
//! doc §1.3: their content is a multi-source aggregate or an
//! off-chain projection, so no single persistent uri exists.
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
use serde::Serialize;

// ---------------------------------------------------------------------------
// PromptSection
// ---------------------------------------------------------------------------

/// Provenance of a [`PromptSection`]: where its content came from.
///
/// Exhaustive by design — no "undefined" state (AC-R1-2). Every
/// construction site must pick one of the three branches; the convenience
/// constructors on [`PromptSection`] keep that to a single line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SectionSource {
    /// Produced by a ContextResource registered under this scheme.
    /// `uri` is the persistent source locator when a single one exists
    /// (e.g. "summaries/{scope_id}.md", a file uri). `None` means the
    /// content is an aggregate/projection with no single uri — such
    /// exemptions are recorded in the ARCH design doc §1.3.
    Resource {
        scheme: &'static str,
        uri: Option<String>,
    },
    /// Composed by the runtime outside the resource chain (fixed sections
    /// and bypass paths such as the summary-generation prompt).
    Runtime { origin: &'static str },
    /// Explicitly exempted from provenance; `reason` records why
    /// (AC-R1-2). Reserved for inputs that are their own origin (the user
    /// turn input) and for legacy paths kept only for compilation.
    Exempted { reason: &'static str },
}

/// A named, rendered section of the prompt envelope.
///
/// `name` is a static label (e.g. "warm_summary", "delivery_context").
/// `content` is the rendered text inserted into the prompt.
/// `source` is mandatory provenance — see [`SectionSource`].
#[derive(Debug, Clone)]
pub struct PromptSection {
    pub name: &'static str,
    pub content: String,
    pub source: SectionSource,
}

impl PromptSection {
    /// Section produced by a chain resource identified only by scheme
    /// (no single persistent uri — aggregate/projection content).
    pub fn from_resource(name: &'static str, scheme: &'static str, content: String) -> Self {
        Self {
            name,
            content,
            source: SectionSource::Resource {
                scheme,
                uri: None,
            },
        }
    }

    /// Section produced by a chain resource with a fully traceable,
    /// persistent uri.
    pub fn from_resource_uri(
        name: &'static str,
        scheme: &'static str,
        uri: String,
        content: String,
    ) -> Self {
        Self {
            name,
            content,
            source: SectionSource::Resource {
                scheme,
                uri: Some(uri),
            },
        }
    }

    /// Section composed by the runtime outside the resource chain
    /// (fixed sections, bypass paths).
    pub fn runtime(name: &'static str, origin: &'static str, content: String) -> Self {
        Self {
            name,
            content,
            source: SectionSource::Runtime { origin },
        }
    }

    /// Section explicitly exempted from provenance (e.g. the user turn
    /// input, which is its own origin).
    pub fn exempted(name: &'static str, reason: &'static str, content: String) -> Self {
        Self {
            name,
            content,
            source: SectionSource::Exempted { reason },
        }
    }
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
    /// The current turn's user input (templated). Ambient per-turn data
    /// made available to every resource: retrieval-style resources
    /// (including third-party memory replacements) may query against it.
    /// Same value the composer previously passed to the memory
    /// pre-render step.
    pub turn_input: &'a str,
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

// Enable inventory collection of ContextResourcePlugin entries.
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

// ---------------------------------------------------------------------------
// Test helpers (R1.2)
// ---------------------------------------------------------------------------

/// Test-only helpers for verifying the skip-not-truncate contract (C-3/C-6).
///
/// Enabled via the `test-helpers` cargo feature (dev-dependencies only).
#[cfg(feature = "test-helpers")]
pub mod test_support {
    use super::{AssemblyContext, ContextResource};

    /// Assert that a resource honors skip-not-truncate under budget
    /// pressure: every section present in the tiny-budget output must be
    /// byte-identical to some section in the full-budget output, or
    /// entirely absent. A section that appears truncated or altered
    /// relative to its full-budget twin, or a section that only exists
    /// under the tiny budget, is a contract violation and panics.
    ///
    /// Matching is multiset containment over `(name, content)` pairs, not
    /// name lookup: resources may legitimately emit several sections
    /// sharing one name (e.g. one `file_resource` section per file), and
    /// pairing those by name alone would cross-match distinct sections.
    ///
    /// `resource` is assembled twice — once with `full_ctx` (budget
    /// effectively unlimited) and once with `tiny_ctx` (budget smaller
    /// than any realistic section). Callers must build both contexts
    /// over the same underlying data.
    pub fn assert_skip_not_truncate(
        resource: &dyn ContextResource,
        full_ctx: &AssemblyContext<'_>,
        tiny_ctx: &AssemblyContext<'_>,
    ) {
        let full_sections = resource.assemble(full_ctx)
            .expect("full-budget assemble must succeed");
        let tiny_sections = resource.assemble(tiny_ctx)
            .expect("tiny-budget assemble must succeed (skipping is allowed, failing is not)");

        // Unmatched full-budget sections; each tiny section consumes one
        // byte-identical twin. Leftovers at the end are the skipped ones.
        let mut unmatched: Vec<&super::PromptSection> = full_sections.iter().collect();

        for tiny in &tiny_sections {
            let twin = unmatched
                .iter()
                .position(|s| s.name == tiny.name && s.content == tiny.content);
            match twin {
                Some(idx) => {
                    unmatched.remove(idx);
                }
                None => {
                    if let Some(full_twin) = unmatched.iter().find(|s| s.name == tiny.name) {
                        panic!(
                            "skip-not-truncate violation: section {:?} is present but truncated \
                             or altered under the tiny budget (tiny {} bytes vs full {} bytes)",
                            tiny.name,
                            tiny.content.len(),
                            full_twin.content.len()
                        );
                    }
                    panic!(
                        "skip-not-truncate violation: section {:?} exists only under the tiny \
                         budget (invented content)",
                        tiny.name
                    );
                }
            }
        }

        let skipped: Vec<&str> = unmatched.iter().map(|s| s.name).collect();
        if !skipped.is_empty() {
            eprintln!(
                "[assert_skip_not_truncate] sections skipped under tiny budget: {:?}",
                skipped
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_ascii_text() {
        // Space breaks the ascii_run: "hello" (5 chars → ceil(5/4) = 2)
        // + "world" (5 chars → ceil(5/4) = 2) = 4 tokens.
        assert_eq!(estimate_tokens("hello world"), 4);
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
        let s = PromptSection::from_resource("test", "memory", "hello".into());
        let s2 = s.clone();
        assert_eq!(s2.name, "test");
        assert_eq!(s2.content, "hello");
        assert_eq!(
            s2.source,
            SectionSource::Resource {
                scheme: "memory",
                uri: None,
            }
        );
    }

    #[test]
    fn section_source_constructors_are_exhaustive() {
        // AC-R1-2: every constructor maps to exactly one branch, and the
        // three branches are distinguishable.
        let from_scheme = PromptSection::from_resource("a", "memory", "x".into());
        let from_uri = PromptSection::from_resource_uri("b", "file", "f.md".into(), "x".into());
        let runtime = PromptSection::runtime("c", "fn:demo", "x".into());
        let exempted = PromptSection::exempted("d", "user turn input", "x".into());

        assert_eq!(
            from_scheme.source,
            SectionSource::Resource {
                scheme: "memory",
                uri: None,
            }
        );
        assert_eq!(
            from_uri.source,
            SectionSource::Resource {
                scheme: "file",
                uri: Some("f.md".into()),
            }
        );
        assert_eq!(
            runtime.source,
            SectionSource::Runtime {
                origin: "fn:demo"
            }
        );
        assert_eq!(
            exempted.source,
            SectionSource::Exempted {
                reason: "user turn input"
            }
        );

        // All four are pairwise distinct provenance values.
        let sources = [
            from_scheme.source,
            from_uri.source,
            runtime.source,
            exempted.source,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }

    #[test]
    fn section_source_serializes_for_telemetry() {
        // D-D: provenance must stay serializable so telemetry can carry it.
        let json = serde_json::to_value(SectionSource::Resource {
            scheme: "warm-summary",
            uri: Some("summaries/thr_1.md".into()),
        })
        .expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "Resource": { "scheme": "warm-summary", "uri": "summaries/thr_1.md" }
            })
        );
    }
}
