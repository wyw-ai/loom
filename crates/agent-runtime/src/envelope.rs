//! Prompt envelope: turns a loose set of per-actor / per-turn sections into
//! a single `session/prompt` payload.
//!
//! The envelope is structured as a sequence of labeled markdown sections so
//! the model can distinguish "who I am" from "what I should remember" from
//! "what the user just said". Section order is intentional — identity and
//! soul go **first** (stable persona), then memory (less stable context),
//! then the scope bootstrap if applicable. Stable sections stay before
//! dynamic per-turn context so provider-side prefix caches can reuse the
//! longest possible prompt prefix. The latest user message stays at the
//! bottom so it's closest to the model's attention.
//!
//! Sections with empty content are skipped.
//!
//! Two entry points:
//!   * [`compose_prompt`] — pure composer. Takes pre-loaded markdown /
//!     pre-rendered memory strings. Used by tests and by callers who
//!     already resolved their inputs.
//!   * [`build_envelope`] — orchestrator. Reads identity / soul from the
//!     profile dir, opens the memory store, runs the selector, renders
//!     memory sections, then hands off to [`compose_prompt`]. This is what
//!     both the server runtime and daemon-managed agent workers call.

/// A named, titled, rendered section. Exposed so callers (and tests) can
/// inspect individual pieces, not just the final glued prompt.
#[derive(Debug, Clone)]
pub struct PromptSection {
    pub name: &'static str,
    pub content: String,
}

/// Input to [`compose_prompt`]. All string fields may be empty — the
/// composer drops empty sections. Pre-rendered memory strings (already
/// formatted by `MemoryRenderer`) are passed through verbatim.
#[derive(Debug, Clone, Default)]
pub struct EnvelopeInput<'a> {
    /// Runtime actor identity resolved by Joi itself. Unlike the profile
    /// identity markdown, this is protocol metadata (actor id/display name)
    /// and should be injected every prompt.
    pub actor_context: &'a str,
    pub identity_markdown: &'a str,
    pub soul_markdown: &'a str,
    pub bootstrap_memory: &'a str,
    pub turn_memory: &'a str,
    /// Dynamic runtime facts for this turn, such as the local wall clock used
    /// by the UI. This is injected every prompt, unlike scope bootstrap.
    pub runtime_context: &'a str,
    /// One-shot scope manifest (who you are, what scope, what CLI is
    /// available). Expected to fire per (actor, scope) on first prompt.
    pub scope_bootstrap: &'a str,
    pub user_message: &'a str,
}

/// Compose the final prompt text + its section breakdown. The returned
/// string is what gets passed to `adapter.send_prompt`; the sections vec
/// is the same data in structured form (useful for telemetry / tests).
pub fn compose_prompt(input: &EnvelopeInput<'_>) -> (String, Vec<PromptSection>) {
    let mut sections: Vec<PromptSection> = Vec::new();

    push_nonempty(
        &mut sections,
        "actor_context",
        input.actor_context.trim().to_string(),
    );
    push_nonempty(
        &mut sections,
        "identity",
        format_markdown_section("Agent identity", input.identity_markdown),
    );
    push_nonempty(
        &mut sections,
        "soul",
        format_markdown_section("Agent soul", input.soul_markdown),
    );
    push_nonempty(
        &mut sections,
        "bootstrap_memory",
        input.bootstrap_memory.trim().to_string(),
    );
    push_nonempty(
        &mut sections,
        "scope_bootstrap",
        input.scope_bootstrap.trim().to_string(),
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
    // a legitimate wake-up signal (e.g. a bare handoff) and the model still
    // needs to see the delimiter.
    sections.push(PromptSection {
        name: "user_message",
        content: format!("=== User message ===\n{}", input.user_message),
    });

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
    sections.push(PromptSection { name, content });
}

fn format_markdown_section(title: &str, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        String::new()
    } else {
        format!("{title}:\n{body}")
    }
}

// ---- high-level orchestrator ----

use std::path::Path;

use proto::methods::{IdentitySpec, MemorySpec};

use crate::memory::{
    load_bootstrap_and_turn, JsonlMemoryStore, MemoryRenderer, MemorySelector, MemoryStore,
};
use crate::profile::read_markdown_file;

/// Inputs for [`build_envelope`]. Callers resolve actor-scoped data
/// (profile dir, specs, current channel) once and hand it in.
#[derive(Debug)]
pub struct BuildContext<'a> {
    /// Joi-resolved actor identity section. Stable for this actor, so callers
    /// should keep volatile facts out of it for better prompt-cache reuse.
    pub actor_context: &'a str,
    /// Absolute path to this actor's profile dir. Identity / soul files
    /// and memory root are all resolved under this.
    pub profile_dir: &'a Path,
    /// `None` means "don't inject identity section". Some(spec) means
    /// "resolve files under profile_dir and inject whatever is there".
    pub identity_spec: Option<&'a IdentitySpec>,
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
    /// The user's message (or equivalent handoff payload). Goes verbatim
    /// into the final `=== User message ===` block.
    pub user_message: &'a str,
    /// Scope bootstrap text (joi CLI manifest). Empty string when this is
    /// not the first prompt in the (actor, scope).
    pub scope_bootstrap: &'a str,
}

/// Load persona + memory, compose. Never fails loud — a profile read
/// error or a memory selector error gets logged and the offending section
/// is skipped, so a broken sidecar never wedges a turn.
pub fn build_envelope(cx: &BuildContext<'_>) -> (String, Vec<PromptSection>) {
    let identity_md = cx
        .identity_spec
        .map(|spec| read_markdown_file(cx.profile_dir, &spec.files.identity))
        .unwrap_or_default();
    let soul_md = cx
        .identity_spec
        .map(|spec| read_markdown_file(cx.profile_dir, &spec.files.soul))
        .unwrap_or_default();

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
        actor_context: cx.actor_context,
        identity_markdown: &identity_md,
        soul_markdown: &soul_md,
        bootstrap_memory: &bootstrap_rendered,
        turn_memory: &turn_rendered,
        runtime_context: cx.runtime_context,
        scope_bootstrap: cx.scope_bootstrap,
        user_message: cx.user_message,
    })
}

fn build_memory_store(profile_dir: &Path, spec: &MemorySpec) -> JsonlMemoryStore {
    let root_path = if std::path::Path::new(&spec.store.root).is_absolute() {
        std::path::PathBuf::from(&spec.store.root)
    } else {
        profile_dir.join(&spec.store.root)
    };
    JsonlMemoryStore::with_shard_by(root_path, &spec.store.shard_by)
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
    std::sync::Arc::new(build_memory_store(profile_dir, spec))
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
            actor_context:
                "=== System: Joi actor identity ===\nYou are Coder (@actor_agent_coder).",
            identity_markdown: "# role",
            soul_markdown: "# style",
            bootstrap_memory: "Bootstrap memory:\n- [fact / high] a",
            turn_memory: "Relevant memory:\n- [note / medium] b",
            runtime_context: "",
            scope_bootstrap: "=== joi bootstrap ===\nscope: thread:x",
            user_message: "hi",
        });
        let names: Vec<_> = sections.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "actor_context",
                "identity",
                "soul",
                "bootstrap_memory",
                "scope_bootstrap",
                "turn_memory",
                "user_message",
            ]
        );
        // Identity must appear before user message.
        assert!(body.find("Agent identity:").unwrap() < body.find("=== User message ===").unwrap());
    }

    #[test]
    fn actor_context_is_injected_before_profile_identity() {
        let (body, sections) = compose_prompt(&EnvelopeInput {
            actor_context:
                "=== System: Joi actor identity ===\nYou are Coder (@actor_agent_coder).",
            identity_markdown: "# role",
            user_message: "hi",
            ..Default::default()
        });
        let names: Vec<_> = sections.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["actor_context", "identity", "user_message"]);
        assert!(body.find("Joi actor identity").unwrap() < body.find("Agent identity:").unwrap());
    }

    #[test]
    fn empty_memory_sections_skipped() {
        let (_, sections) = compose_prompt(&EnvelopeInput {
            identity_markdown: "# role",
            bootstrap_memory: "  ",
            turn_memory: "",
            user_message: "hi",
            ..Default::default()
        });
        let names: Vec<_> = sections.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["identity", "user_message"]);
    }

    #[test]
    fn stable_scope_bootstrap_precedes_dynamic_runtime_context_for_cache_reuse() {
        let (body, sections) = compose_prompt(&EnvelopeInput {
            runtime_context: "=== System: Local time context ===\nCurrent local time: x",
            scope_bootstrap: "=== joi bootstrap ===\nscope: thread:x",
            user_message: "hi",
            ..Default::default()
        });
        let names: Vec<_> = sections.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec!["scope_bootstrap", "runtime_context", "user_message"]
        );
        assert!(body.find("joi bootstrap").unwrap() < body.find("Local time context").unwrap());
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
