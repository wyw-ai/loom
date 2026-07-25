//! Per-actor memory subsystem.
//!
//! Three layers composed:
//!   * [`store`] — pluggable persistence. Only [`JsonlMemoryStore`] is
//!     implemented today (month-sharded append-only JSONL under
//!     `{agent.profile}/memory/records/`).
//!   * [`selector`] — per-turn selection: a "bootstrap" pool (recent +
//!     high-confidence, size = `bootstrapTopK`) and a "turn" pool (keyword
//!     overlap with current prompt + thread context, size = `turnTopK`).
//!   * [`renderer`] — formats a pool into a labeled markdown section for
//!     inclusion in the prompt envelope.
//!
//! Channel-scoped filtering (off-by-default override `perChannel: false`) is
//! threaded all the way down to the selector; it prevents records sourced
//! from channel A from being recalled when the agent is operating in
//! channel B. This is the multi-actor safety net that the upstream
//! legacy single-user design did not need.

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
