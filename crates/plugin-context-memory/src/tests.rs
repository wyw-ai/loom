//! Tests for [`MemoryResource`] — migrated from the old
//! `agent-runtime` `MemoryProvider` tests, plus the iter2 degradation /
//! provenance / equivalence coverage (ARCH design §6).

use std::path::{Path, PathBuf};

use proto::methods::{
    MemoryDeliverySpec, MemoryQuerySpec, MemorySpec, MemoryStoreSpec,
};
use proto::types::{ScopeKind, ScopeRef};

use context_layer_core::test_support::assert_skip_not_truncate;
use context_layer_core::{AssemblyContext, ContextResource, SectionSource};

use crate::record::{MemoryRecord, MemorySource};
use crate::store::MemoryStore;
use crate::{JsonlMemoryStore, MemoryRenderer, MemoryResource, MemorySelector};
use crate::load_bootstrap_and_turn;

fn tmpdir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("loom-plugin-context-memory-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn mk(id: &str, ts: &str, summary: &str, confidence: &str) -> MemoryRecord {
    MemoryRecord {
        schema_version: 1,
        id: id.into(),
        actor_id: "actor_test".into(),
        ts: ts.into(),
        record_type: "fact".into(),
        status: "accepted".into(),
        summary: summary.into(),
        detail: String::new(),
        confidence: confidence.into(),
        source: MemorySource::default(),
        tags: vec![],
    }
}

fn spec_with_root(root: &Path, prompt: bool) -> MemorySpec {
    MemorySpec {
        store: MemoryStoreSpec {
            root: root.display().to_string(),
            ..MemoryStoreSpec::default()
        },
        query: MemoryQuerySpec {
            mode: "heuristic".into(),
            bootstrap_top_k: 5,
            turn_top_k: 3,
            per_channel: false,
        },
        delivery: MemoryDeliverySpec {
            prompt,
            mcp: false,
        },
        extraction: Default::default(),
        compaction: Default::default(),
    }
}

fn fixture_store() -> (PathBuf, JsonlMemoryStore) {
    let root = tmpdir("fixture");
    let store = JsonlMemoryStore::new(root.clone());
    store
        .append(&mk(
            "r1",
            "2026-04-05T10:00:00Z",
            "deploys go through pipeline X",
            "high",
        ))
        .unwrap();
    store
        .append(&mk("r2", "2026-04-04T10:00:00Z", "prefers tabs", "low"))
        .unwrap();
    (root, store)
}

fn ctx_for<'a>(
    scope: &'a ScopeRef,
    profile_dir: &'a Path,
    turn_input: &'a str,
    budget: u64,
) -> AssemblyContext<'a> {
    AssemblyContext {
        scope,
        channel_id: None,
        actor_id: "actor_test",
        profile_dir,
        budget_remaining: budget,
        budget_total: 10_000,
        delivery_context: "",
        turn_input,
        first_turn: false,
    }
}

#[test]
fn scheme_and_priority() {
    let r = MemoryResource::new(None);
    assert_eq!(r.scheme(), "memory");
    assert_eq!(r.priority(), 5);
    assert_eq!(r.effective_scope(), &[ScopeKind::Thread, ScopeKind::Channel]);
}

#[test]
fn absent_spec_yields_no_sections() {
    // AC-M1-3 degradation path 1: no MemorySpec → no sections.
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: "test".into(),
    };
    let r = MemoryResource::new(None);
    let ctx = ctx_for(&scope, Path::new("/tmp"), "", 10_000);
    assert!(r.assemble(&ctx).unwrap().is_empty());
}

#[test]
fn mcp_only_spec_yields_no_sections() {
    // AC-M1-3 degradation path 2: delivery.prompt = false (MCP-only
    // mode) → no prompt sections even with a populated store.
    let (root, _store) = fixture_store();
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: "test".into(),
    };
    let r = MemoryResource::new(Some(spec_with_root(&root, false)));
    let ctx = ctx_for(&scope, &root, "deploy pipeline", 10_000);
    assert!(r.assemble(&ctx).unwrap().is_empty());
}

#[test]
fn broken_store_degrades_to_empty() {
    // AC-M1-3 degradation path 3: store root that cannot be read
    // (points at a regular file → read_dir fails) → warn + empty
    // sections, never an error.
    let (root, _store) = fixture_store();
    let file_root = root.join("2026-04.jsonl"); // a file, not a dir
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: "test".into(),
    };
    let r = MemoryResource::new(Some(spec_with_root(&file_root, true)));
    let ctx = ctx_for(&scope, &root, "deploy", 10_000);
    let sections = r.assemble(&ctx).unwrap();
    assert!(sections.is_empty());
}

#[test]
fn fixture_store_renders_sections() {
    let (root, _store) = fixture_store();
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: "test".into(),
    };
    let r = MemoryResource::new(Some(spec_with_root(&root, true)));
    let ctx = ctx_for(&scope, &root, "how does the deploy pipeline work", 10_000);
    let sections = r.assemble(&ctx).unwrap();
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].name, "bootstrap_memory");
    assert_eq!(sections[0].content,
        "Bootstrap memory:\n- [fact / high] deploys go through pipeline X\n- [fact / low] prefers tabs");
    assert_eq!(sections[1].name, "turn_memory");
    assert_eq!(sections[1].content,
        "Relevant memory:\n- [fact / high] deploys go through pipeline X");
}

#[test]
fn sections_carry_resource_provenance() {
    // AC-M3-2: memory sections are Resource-sourced with scheme
    // "memory" and no uri (aggregate over the store; uri precision is
    // deferred to iter3 by design).
    let (root, _store) = fixture_store();
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: "test".into(),
    };
    let r = MemoryResource::new(Some(spec_with_root(&root, true)));
    let ctx = ctx_for(&scope, &root, "deploy", 10_000);
    for section in r.assemble(&ctx).unwrap() {
        assert_eq!(
            section.source,
            SectionSource::Resource {
                scheme: "memory",
                uri: None,
            }
        );
    }
}

#[test]
fn matches_legacy_prerender_pipeline() {
    // AC-M3-1 golden equivalence: MemoryResource.assemble output must
    // be byte-identical (name / source / content) to what the pre-iter2
    // compose path produced — open store → selector →
    // load_bootstrap_and_turn → render → trim → from_resource.
    let (root, _store) = fixture_store();
    let spec = spec_with_root(&root, true);
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: "test".into(),
    };
    let turn_input = "how does the deploy pipeline work";
    let thread_context = "";

    // Legacy pipeline (compose step 2 at iter1 baseline c44fc81).
    let store = crate::open_memory_store(&root, &spec);
    let selector = MemorySelector::new(spec.clone(), None);
    let (boot, turn) =
        load_bootstrap_and_turn(&selector, &store, turn_input, thread_context).unwrap();
    let mut legacy: Vec<(String, SectionSource, String)> = Vec::new();
    let boot_text = MemoryRenderer::render_bootstrap(&boot);
    if !boot_text.trim().is_empty() {
        legacy.push((
            "bootstrap_memory".into(),
            SectionSource::Resource {
                scheme: "memory",
                uri: None,
            },
            boot_text,
        ));
    }
    let turn_text = MemoryRenderer::render_turn(&turn);
    if !turn_text.trim().is_empty() {
        legacy.push((
            "turn_memory".into(),
            SectionSource::Resource {
                scheme: "memory",
                uri: None,
            },
            turn_text,
        ));
    }

    // New path.
    let r = MemoryResource::new(Some(spec));
    let ctx = ctx_for(&scope, &root, turn_input, 10_000);
    let new_sections = r.assemble(&ctx).unwrap();
    let new_out: Vec<(String, SectionSource, String)> = new_sections
        .iter()
        .map(|s| (s.name.to_string(), s.source.clone(), s.content.clone()))
        .collect();

    assert_eq!(new_out, legacy);
}

#[test]
fn honors_skip_not_truncate() {
    // AC-M3-3: under a tiny budget the memory resource must skip
    // sections entirely, never truncate them.
    let (root, _store) = fixture_store();
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: "test".into(),
    };
    let r = MemoryResource::new(Some(spec_with_root(&root, true)));
    let full_ctx = ctx_for(&scope, &root, "deploy", 10_000);
    let tiny_ctx = ctx_for(&scope, &root, "deploy", 1);
    assert_skip_not_truncate(&r, &full_ctx, &tiny_ctx);
}
