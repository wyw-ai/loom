//! Pluggable memory persistence.

use super::record::{MemoryQuery, MemoryRecord};

pub trait MemoryStore: Send + Sync {
    /// Recent accepted records, newest first. Respects nothing else (no
    /// channel scope) — the selector layers that on top.
    fn list_recent(&self, limit: usize) -> Result<Vec<MemoryRecord>, String>;

    /// Lookup by id. Returns `Ok(None)` if absent.
    fn get(&self, id: &str) -> Result<Option<MemoryRecord>, String>;

    /// Filtered scan. Implementations may be dumb — the JSONL one does a
    /// load-all-then-filter. Swap for a real index when corpus grows.
    fn query(&self, query: &MemoryQuery) -> Result<Vec<MemoryRecord>, String>;

    /// Append a single record. Persistence must be durable before return
    /// (fsync not required, but the byte has to be flushed so a concurrent
    /// reader sees it).
    fn append(&self, record: &MemoryRecord) -> Result<(), String>;
}
