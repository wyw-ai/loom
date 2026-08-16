//! AC-M2-2: third-party context-resource replacement integration test.
//!
//! A standalone test binary (separate from the unit tests) so the
//! `inventory::submit!` registration below cannot pollute other test
//! binaries. It proves the iter2 pluginization contract end to end:
//!
//! 1. A third-party crate registers a non-memory scheme resource via
//!    `inventory::submit!` — the same zero-privilege path any external
//!    plugin uses (`loom-plugin-context-tier` registers warm-summary and
//!    message-list this way).
//! 2. The replacement resource reads the ambient per-turn data through
//!    the *same* channels the official memory plugin uses:
//!    `ctx.turn_input`, `ctx.delivery_context`, plus its own embedded
//!    config (inventory factories are zero-arg; per ARCH iter2 §2 a
//!    third-party replacement carries its config itself).
//! 3. It participates in the same chain assembly: priority ordering and
//!    the budget waterfall (skip-not-truncate) treat it identically to
//!    the official `plugin-context-memory` resource.

use agent_runtime::context_layer::{discover_plugins, ContextResourceRegistry};
use context_layer_core::{AssemblyContext, ContextResource, PromptSection, SectionSource};
use plugin_context_memory::{JsonlMemoryStore, MemoryRecord, MemoryStore};
use proto::methods::MemorySpec;
use proto::types::{ScopeKind, ScopeRef};

// ---------------------------------------------------------------------------
// Third-party replacement resource ("echo-input" scheme)
// ---------------------------------------------------------------------------

struct EchoInputResource {
    /// The replacement's own embedded config. Since B4 the inventory
    /// factory receives the resource's config envelope, but a
    /// third-party resource may still carry static business config
    /// itself (ARCH iter2 §2: "第三方替身有自己的 agentcontext config").
    label: &'static str,
}

impl EchoInputResource {
    fn new() -> Self {
        Self { label: "echo-config" }
    }
}

impl ContextResource for EchoInputResource {
    fn scheme(&self) -> &'static str {
        "echo-input"
    }

    fn priority(&self) -> i32 {
        15
    }

    fn effective_scope(&self) -> &[ScopeKind] {
        &[ScopeKind::Thread, ScopeKind::Channel]
    }

    fn assemble(&self, ctx: &AssemblyContext<'_>) -> anyhow::Result<Vec<PromptSection>> {
        // Reads all three data channels available to a replacement:
        // own config + ambient turn_input + ambient delivery_context.
        let content = format!(
            "[{}] turn={} delivery={}",
            self.label, ctx.turn_input, ctx.delivery_context
        );
        Ok(vec![PromptSection {
            name: "echo_input",
            content,
            source: SectionSource::Resource { scheme: "echo-input", uri: None },
        }])
    }
}

inventory::submit! {
    context_layer_core::ContextResourcePlugin {
        scheme: "echo-input",
        factory: |_config| Box::new(EchoInputResource::new()),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_memory_spec(root: &std::path::Path) -> MemorySpec {
    let store = JsonlMemoryStore::new(root.to_path_buf());
    store
        .append(&MemoryRecord {
            schema_version: 1,
            id: "m1".into(),
            actor_id: "actor_test".into(),
            ts: "2026-04-05T10:00:00Z".into(),
            record_type: "fact".into(),
            status: "accepted".into(),
            summary: "replacement chain fixture record".into(),
            detail: String::new(),
            confidence: "high".into(),
            source: Default::default(),
            tags: vec![],
        })
        .expect("append fixture record");
    let mut spec = MemorySpec::default();
    spec.store.root = root.display().to_string();
    spec.delivery.prompt = true;
    spec
}

fn test_ctx<'a>(scope: &'a ScopeRef, profile_dir: &'a std::path::Path) -> AssemblyContext<'a> {
    AssemblyContext {
        scope,
        channel_id: None,
        actor_id: "actor_test",
        profile_dir,
        budget_remaining: 100_000,
        budget_total: 100_000,
        delivery_context: "DELIVERY-CURSOR",
        turn_input: "ambient turn input",
        first_turn: false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn third_party_replacement_joins_chain_and_reads_ambient_data() {
    // 1. Discovery: the inventory registration is visible through the
    //    same public API the composer uses.
    let plugins = discover_plugins();
    let factory = *plugins
        .get("echo-input")
        .expect("inventory registration must be discoverable");

    let root = tempfile::tempdir().expect("tempdir");
    let scope = ScopeRef { kind: ScopeKind::Thread, id: "t".into() };
    let ctx = test_ctx(&scope, root.path());

    // 2. Same-chain assembly: official memory plugin (priority 5) and the
    //    third-party replacement (priority 15) in one registry.
    let mut registry = ContextResourceRegistry::new();
    registry.register(Box::new(plugin_context_memory::MemoryResource::new(Some(
        fixture_memory_spec(root.path()),
    ))));
    // B4: the factory now receives the config envelope. Passing a
    // non-empty envelope proves the channel is live end-to-end; the
    // echo resource ignores it, so output is unchanged.
    registry.register(factory(&Some(serde_json::json!({ "label": "ignored-by-echo" }))));

    let (sections, _) = registry.assemble_chain(&ctx, 100_000);

    // 3. The replacement's section is present and reflects all three
    //    data channels (own config + turn_input + delivery_context).
    let echo = sections
        .iter()
        .find(|s| s.name == "echo_input")
        .expect("replacement section assembled");
    assert_eq!(
        echo.content,
        "[echo-config] turn=ambient turn input delivery=DELIVERY-CURSOR"
    );
    assert_eq!(
        echo.source,
        SectionSource::Resource { scheme: "echo-input", uri: None }
    );

    // 4. Priority ordering treats both resources identically: memory (5)
    //    assembles before the replacement (15).
    let memory_pos = sections
        .iter()
        .position(|s| s.name == "bootstrap_memory")
        .expect("official memory section present");
    let echo_pos = sections
        .iter()
        .position(|s| s.name == "echo_input")
        .expect("replacement section present");
    assert!(memory_pos < echo_pos, "priority ordering spans official and third-party resources");
}

#[test]
fn third_party_replacement_honors_budget_waterfall() {
    // The budget waterfall (skip-not-truncate) applies to the replacement
    // exactly as it does to the official memory plugin: with a tiny
    // remaining budget the section is skipped whole, never truncated.
    let plugins = discover_plugins();
    let factory = *plugins.get("echo-input").expect("discoverable");

    let root = tempfile::tempdir().expect("tempdir");
    let scope = ScopeRef { kind: ScopeKind::Thread, id: "t".into() };

    let mut full = ContextResourceRegistry::new();
    full.register(factory(&None));
    let mut tiny = ContextResourceRegistry::new();
    tiny.register(factory(&None));

    let full_ctx = test_ctx(&scope, root.path());
    let mut tiny_ctx = test_ctx(&scope, root.path());
    tiny_ctx.budget_remaining = 8;
    tiny_ctx.budget_total = 8;

    let (full_sections, _) = full.assemble_chain(&full_ctx, 100_000);
    let (tiny_sections, _) = tiny.assemble_chain(&tiny_ctx, 8);

    assert_eq!(full_sections.len(), 1, "ample budget includes the replacement section");
    assert!(
        tiny_sections.iter().all(|s| s.name != "echo_input"),
        "tiny budget skips the replacement section entirely (skip-not-truncate)"
    );
}
