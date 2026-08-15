//! Wire / on-disk record shape.

use serde::{Deserialize, Serialize};

/// Single memory entry. JSONL line = one of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub actor_id: String,
    /// RFC3339 UTC timestamp.
    pub ts: String,
    /// Free-form taxonomy. Conventional values: `"fact"` / `"decision"` /
    /// `"task"` / `"note"` / `"preference"` — but not enforced.
    #[serde(rename = "type")]
    pub record_type: String,
    /// `"accepted"` / `"pending"` / `"rejected"` / `"archived"`. Only
    /// `"accepted"` (case-insensitive) is selected by default.
    pub status: String,
    /// Short one-liner — this is what goes into the prompt section.
    pub summary: String,
    #[serde(default)]
    pub detail: String,
    /// `"high"` / `"medium"` / `"low"` / anything else → unknown.
    pub confidence: String,
    #[serde(default)]
    pub source: MemorySource,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySource {
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub message_ids: Vec<String>,
}

impl MemoryRecord {
    pub fn is_accepted(&self) -> bool {
        self.status.eq_ignore_ascii_case("accepted")
    }
}

/// Selection request for [`super::store::MemoryStore::query`].
#[derive(Debug, Clone, Default)]
pub struct MemoryQuery {
    /// Free-text query. Tokenized + substring-matched against summary /
    /// detail / tags. `None` means "no text filter".
    pub text: Option<String>,
    pub tags: Vec<String>,
    pub types: Vec<String>,
    pub limit: usize,
    /// When `Some(channel_id)`, only records whose `source.channelId` equals
    /// this value are returned. Orthogonal to text / tag / type filters.
    pub channel_scope: Option<String>,
    pub include_non_accepted: bool,
}

pub fn confidence_rank(confidence: &str) -> u8 {
    match confidence.to_ascii_lowercase().as_str() {
        "high" => 3,
        "medium" | "med" => 2,
        "low" => 1,
        _ => 0,
    }
}

fn default_schema_version() -> u32 {
    1
}
