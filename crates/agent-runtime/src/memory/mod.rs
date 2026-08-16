//! Per-actor memory subsystem — re-export shim.
//!
//! As of iter2 (ARCH design §3) the memory business modules live in the
//! standalone `plugin-context-memory` crate (the official memory
//! ContextResource plugin, contract-bound to `context-layer-core`).
//! This module re-exports them so existing consumers
//! (`envelope.rs`, the CLI `memory` / `mcp-memory` subcommands) keep
//! their `agent_runtime::memory::*` import paths unchanged.

pub use plugin_context_memory::{
    jsonl_store, record, renderer, selector, store, JsonlMemoryStore, confidence_rank,
    load_bootstrap_and_turn, MemoryQuery, MemoryRecord, MemoryRenderer, MemorySelector,
    MemorySource, MemoryStore,
};