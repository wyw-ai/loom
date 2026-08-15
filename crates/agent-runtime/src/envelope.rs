//! Prompt envelope: turns Loom-owned per-turn sections into a single prompt
//! payload.
//!
//! The envelope is structured as a sequence of labeled markdown sections so
//! the model can distinguish memory, runtime context, and the user turn. The
//! latest user message stays at the bottom so it's closest to the model's
//! attention.
//!
//! Sections with empty content are skipped.
//!
//! Two entry points:
//!   * [`compose_prompt`] — pure composer. Takes pre-loaded markdown /
//!     pre-rendered memory strings. Used by tests and by callers who
//!     already resolved their inputs.
//!   * [`build_envelope`] — orchestrator. Opens the memory store, runs the
//!     selector, renders memory sections, then hands off to
//!     [`compose_prompt`].

/// A named, titled, rendered section. Exposed so callers (and tests) can
/// inspect individual pieces, not just the final glued prompt.
///
/// The canonical definition lives in `context-layer-core`; this re-export
/// preserves backward compatibility for `agent_runtime::envelope::PromptSection`.
pub use context_layer_core::PromptSection;

/// Input to [`compose_prompt`]. All string fields may be empty — the
/// composer drops empty sections. Pre-rendered memory strings (already
/// formatted by `MemoryRenderer`) are passed through verbatim.
#[derive(Debug, Clone, Default)]
pub struct EnvelopeInput<'a> {
    pub bootstrap_memory: &'a str,
    pub turn_memory: &'a str,
    /// Dynamic runtime facts for this turn, such as the local wall clock used
    /// by the UI.
    pub runtime_context: &'a str,
    pub user_message: &'a str,
}

/// Compose the final prompt text + its section breakdown. The returned
/// string is what gets passed to `adapter.send_prompt`; the sections vec
/// is the same data in structured form (useful for telemetry / tests).
pub fn compose_prompt(input: &EnvelopeInput<'_>) -> (String, Vec<PromptSection>) {
    let mut sections: Vec<PromptSection> = Vec::new();

    push_nonempty(
        &mut sections,
        "bootstrap_memory",
        input.bootstrap_memory.trim().to_string(),
    );
    push_nonempty(
        &mut sections,
        "turn_memory",
        input.turn_memory.trim().to_string(),
    );
    push_nonempty(
        &mut sections,
        "runtime_context",
        input.runtime_context.trim().to_string(),
    );

    // User message is always last, even if blank — an empty user message is
    // a legitimate wake-up signal (e.g. a bare directed message) and the model still
    // needs to see the delimiter.
    sections.push(PromptSection::exempted(
        "user_message",
        "legacy envelope path",
        format!("=== User message ===\n{}", input.user_message),
    ));

    let body = sections
        .iter()
        .map(|s| s.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    (body, sections)
}

fn push_nonempty(sections: &mut Vec<PromptSection>, name: &'static str, content: String) {
    if content.trim().is_empty() {
        return;
    }
    sections.push(PromptSection::exempted(
        name,
        "legacy envelope path",
        content,
    ));
}

// ---- high-level orchestrator ----

use std::path::Path;

use proto::methods::MemorySpec;

use crate::memory::{
    load_bootstrap_and_turn, JsonlMemoryStore, MemoryRenderer, MemorySelector, MemoryStore,
};

/// Inputs for [`build_envelope`].
#[derive(Debug)]
pub struct BuildContext<'a> {
    /// Absolute path to this actor's profile dir. Memory roots are resolved
    /// under this when relative.
    pub profile_dir: &'a Path,
    /// `None` means "don't inject memory section". Some(spec) with
    /// `delivery.prompt = false` also suppresses injection (MCP-only mode).
    pub memory_spec: Option<&'a MemorySpec>,
    /// Channel id for `perChannel: true` memory filtering. `None` means
    /// "no channel scope available" — the selector will fall open.
    pub channel_id: Option<&'a str>,
    /// Recent thread / channel context text used to bias the turn-memory
    /// keyword query. May be empty.
    pub thread_context: &'a str,
    /// Dynamic runtime facts that should be refreshed for every turn.
    pub runtime_context: &'a str,
    /// The user's message (or equivalent directed payload). Goes verbatim
    /// into the final `=== User message ===` block.
    pub user_message: &'a str,
}

/// Load memory, compose. Never fails loud — a memory selector error gets
/// logged and the offending section is skipped, so a broken sidecar never
/// wedges a turn.
pub fn build_envelope(cx: &BuildContext<'_>) -> (String, Vec<PromptSection>) {
    let (bootstrap_rendered, turn_rendered) = match cx.memory_spec {
        Some(mem) if mem.delivery.prompt => {
            let store = build_memory_store(cx.profile_dir, mem);
            let selector = MemorySelector::new(mem.clone(), cx.channel_id.map(String::from));
            match load_bootstrap_and_turn(&selector, &store, cx.user_message, cx.thread_context) {
                Ok((boot, turn)) => (
                    MemoryRenderer::render_bootstrap(&boot),
                    MemoryRenderer::render_turn(&turn),
                ),
                Err(err) => {
                    tracing::warn!(%err, "memory selection failed; skipping memory section");
                    (String::new(), String::new())
                }
            }
        }
        _ => (String::new(), String::new()),
    };

    compose_prompt(&EnvelopeInput {
        bootstrap_memory: &bootstrap_rendered,
        turn_memory: &turn_rendered,
        runtime_context: cx.runtime_context,
        user_message: cx.user_message,
    })
}

fn build_memory_store(profile_dir: &Path, spec: &MemorySpec) -> JsonlMemoryStore {
    plugin_memory::open_memory_store(profile_dir, spec)
}

/// Directly expose the concrete store; used by the MCP bridge subcommand
/// and any caller that needs to write records (selectors are read-only).
pub fn open_memory_store(profile_dir: &Path, spec: &MemorySpec) -> JsonlMemoryStore {
    build_memory_store(profile_dir, spec)
}

/// Public read-through for tests / callers that only need the store trait.
pub fn open_memory_store_dyn(
    profile_dir: &Path,
    spec: &MemorySpec,
) -> std::sync::Arc<dyn MemoryStore> {
    plugin_memory::open_memory_store_dyn(profile_dir, spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_inputs_yield_only_user_message() {
        let (body, sections) = compose_prompt(&EnvelopeInput {
            user_message: "hello",
            ..Default::default()
        });
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "user_message");
        assert!(body.contains("=== User message ==="));
        assert!(body.contains("hello"));
    }

    #[test]
    fn full_envelope_orders_sections() {
        let (body, sections) = compose_prompt(&EnvelopeInput {
            bootstrap_memory: "Bootstrap memory:\n- [fact / high] a",
            turn_memory: "Relevant memory:\n- [note / medium] b",
            runtime_context: "=== System: Local time context ===\nCurrent local time: x",
            user_message: "hi",
        });
        let names: Vec<_> = sections.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "bootstrap_memory",
                "turn_memory",
                "runtime_context",
                "user_message",
            ]
        );
        assert!(body.find("Bootstrap memory").unwrap() < body.find("Relevant memory").unwrap());
        assert!(body.find("Relevant memory").unwrap() < body.find("Local time context").unwrap());
        assert!(
            body.find("Local time context").unwrap() < body.find("=== User message ===").unwrap()
        );
    }

    #[test]
    fn empty_memory_sections_skipped() {
        let (_, sections) = compose_prompt(&EnvelopeInput {
            bootstrap_memory: "  ",
            turn_memory: "",
            user_message: "hi",
            ..Default::default()
        });
        let names: Vec<_> = sections.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["user_message"]);
    }

    #[test]
    fn runtime_context_precedes_user_message() {
        let (body, sections) = compose_prompt(&EnvelopeInput {
            runtime_context: "=== System: Local time context ===\nCurrent local time: x",
            user_message: "hi",
            ..Default::default()
        });
        let names: Vec<_> = sections.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["runtime_context", "user_message"]);
        assert!(
            body.find("Local time context").unwrap() < body.find("=== User message ===").unwrap()
        );
    }

    #[test]
    fn whitespace_only_user_message_still_emits_delimiter() {
        let (body, sections) = compose_prompt(&EnvelopeInput {
            user_message: "",
            ..Default::default()
        });
        assert_eq!(sections.len(), 1);
        assert!(body.contains("=== User message ==="));
    }
}
