//! plugin-memory — the official memory ContextResource plugin.
//!
//! Migrated wholesale from `agent-runtime::memory` (iter2, ARCH design
//! §3): the memory business modules (selector / renderer / store /
//! jsonl_store / record) now live in this standalone crate and are
//! re-exported by `agent-runtime` for API compatibility.
//!
//! Boundary rules (compile-time enforced by Cargo):
//! * depends only on `context-layer-core` (the public contract) plus
//!   `proto` (MemorySpec) and basic libs — never on `agent-runtime`;
//! * NOT registered via `inventory` — the builtin factory in the CLI
//!   captures the actor's `MemorySpec` at construction time (a
//!   zero-arg inventory factory cannot access that spec and would
//!   shadow the builtin; ARCH iter2 裁决二).
//!
//! `MemoryResource` replaces the old `MemoryProvider` wrapper: instead
//! of receiving pre-rendered strings from the compose path, it runs
//! the memory selection itself during `assemble`, using the
//! `AssemblyContext` fields (`profile_dir`, `channel_id`,
//! `turn_input`, `delivery_context`).

use std::path::Path;

use anyhow::Result;
use proto::methods::MemorySpec;
use proto::types::ScopeKind;

use context_layer_core::{AssemblyContext, ContextResource, PromptSection};

pub mod jsonl_store;
pub mod record;
pub mod renderer;
pub mod selector;
pub mod store;

pub use jsonl_store::JsonlMemoryStore;
pub use record::{confidence_rank, MemoryQuery, MemoryRecord, MemorySource};
pub use renderer::MemoryRenderer;
pub use selector::{load_bootstrap_and_turn, MemorySelector};
pub use store::MemoryStore;

/// Memory ContextResource — loads and renders actor memory during
/// assembly.
///
/// Construction captures the actor's effective `MemorySpec`:
/// * `None` → the resource yields no sections (memory disabled);
/// * `Some(spec)` with `delivery.prompt = false` → also no sections
///   (MCP-only mode);
/// * otherwise the selector runs against the JSONL store rooted at
///   `ctx.profile_dir`.
pub struct MemoryResource {
    spec: Option<MemorySpec>,
    effective_scopes: Vec<ScopeKind>,
}

impl MemoryResource {
    /// Capture the actor's memory spec. `None` disables the resource.
    pub fn new(spec: Option<MemorySpec>) -> Self {
        Self {
            spec,
            effective_scopes: vec![ScopeKind::Thread, ScopeKind::Channel],
        }
    }
}

impl ContextResource for MemoryResource {
    fn scheme(&self) -> &str {
        "memory"
    }

    fn priority(&self) -> i32 {
        5
    }

    fn effective_scope(&self) -> &[ScopeKind] {
        &self.effective_scopes
    }

    fn assemble(&self, ctx: &AssemblyContext<'_>) -> Result<Vec<PromptSection>> {
        // Skip entirely when memory is absent or prompt delivery is off.
        let mem = match self.spec.as_ref() {
            Some(mem) if mem.delivery.prompt => mem,
            _ => return Ok(Vec::new()),
        };

        let store = open_memory_store(ctx.profile_dir, mem);
        let selector = MemorySelector::new(mem.clone(), ctx.channel_id.map(String::from));
        match load_bootstrap_and_turn(&selector, &store, ctx.turn_input, ctx.delivery_context) {
            Ok((boot, turn)) => {
                let mut sections = Vec::new();
                let bootstrap = MemoryRenderer::render_bootstrap(&boot);
                if !bootstrap.trim().is_empty() {
                    sections.push(PromptSection::from_resource(
                        "bootstrap_memory",
                        "memory",
                        bootstrap,
                    ));
                }
                let turn = MemoryRenderer::render_turn(&turn);
                if !turn.trim().is_empty() {
                    sections.push(PromptSection::from_resource("turn_memory", "memory", turn));
                }
                Ok(sections)
            }
            Err(err) => {
                // Never fail loud — a broken sidecar must not wedge a turn.
                tracing::warn!(%err, "memory selection failed; skipping memory section");
                Ok(Vec::new())
            }
        }
    }
}

fn build_memory_store(profile_dir: &Path, spec: &MemorySpec) -> JsonlMemoryStore {
    let root_path = if Path::new(&spec.store.root).is_absolute() {
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
pub fn open_memory_store_dyn(profile_dir: &Path, spec: &MemorySpec) -> std::sync::Arc<dyn MemoryStore> {
    std::sync::Arc::new(build_memory_store(profile_dir, spec))
}

#[cfg(test)]
mod tests;
