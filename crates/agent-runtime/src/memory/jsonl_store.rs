//! Month-sharded append-only JSONL implementation of [`MemoryStore`].
//!
//! Layout:
//!
//! ```text
//! {root}/
//!   2026-04.jsonl
//!   2026-03.jsonl
//!   ...
//! ```
//!
//! Each line is one [`MemoryRecord`] serialized as JSON. Read path = concat
//! all `.jsonl` files, parse per-line (skipping blank / malformed lines),
//! sort by `ts` descending. Correct up to a few thousand records; we can
//! swap in a real index without changing the public API.

use chrono::{DateTime, Datelike, Utc};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use super::record::{MemoryQuery, MemoryRecord};
use super::store::MemoryStore;

#[derive(Debug, Clone)]
pub struct JsonlMemoryStore {
    root: PathBuf,
    shard_by: ShardBy,
}

#[derive(Debug, Clone, Copy)]
enum ShardBy {
    Month,
    Day,
}

impl ShardBy {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "day" => Self::Day,
            _ => Self::Month,
        }
    }

    fn shard_name(self, ts: &DateTime<Utc>) -> String {
        match self {
            Self::Month => format!("{:04}-{:02}.jsonl", ts.year(), ts.month()),
            Self::Day => format!("{:04}-{:02}-{:02}.jsonl", ts.year(), ts.month(), ts.day()),
        }
    }
}

impl JsonlMemoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            shard_by: ShardBy::Month,
        }
    }

    pub fn with_shard_by(root: PathBuf, shard_by: &str) -> Self {
        Self {
            root,
            shard_by: ShardBy::parse(shard_by),
        }
    }

    /// Path of the shard a record with the given RFC3339 ts would land in.
    /// Malformed ts falls back to "now"; we never reject on save.
    fn shard_path_for_ts(&self, ts: &str) -> PathBuf {
        let parsed = DateTime::parse_from_rfc3339(ts)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        self.root.join(self.shard_by.shard_name(&parsed))
    }

    fn shard_files(&self) -> Result<Vec<PathBuf>, String> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut files: Vec<PathBuf> = fs::read_dir(&self.root)
            .map_err(|err| format!("read {}: {err}", self.root.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
            .collect();
        // Lexical sort is chronological for both `YYYY-MM` and `YYYY-MM-DD`.
        // Newest-first comes out of the final sort-by-ts anyway.
        files.sort();
        Ok(files)
    }

    fn load_all(&self) -> Result<Vec<MemoryRecord>, String> {
        let mut items = Vec::new();
        for file in self.shard_files()? {
            items.extend(read_jsonl_file(&file)?);
        }
        items.sort_by(|a, b| b.ts.cmp(&a.ts));
        Ok(items)
    }
}

impl MemoryStore for JsonlMemoryStore {
    fn list_recent(&self, limit: usize) -> Result<Vec<MemoryRecord>, String> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut items: Vec<MemoryRecord> = self
            .load_all()?
            .into_iter()
            .filter(|r| r.is_accepted())
            .collect();
        items.truncate(limit);
        Ok(items)
    }

    fn get(&self, id: &str) -> Result<Option<MemoryRecord>, String> {
        Ok(self.load_all()?.into_iter().find(|r| r.id == id))
    }

    fn query(&self, query: &MemoryQuery) -> Result<Vec<MemoryRecord>, String> {
        let text_tokens: Vec<String> = query.text.as_deref().map(tokenize).unwrap_or_default();
        let text_raw = query
            .text
            .as_deref()
            .map(|t| t.to_lowercase())
            .unwrap_or_default();
        let tags: Vec<String> = query.tags.iter().map(|t| t.to_lowercase()).collect();
        let types: Vec<String> = query.types.iter().map(|t| t.to_lowercase()).collect();

        let channel_scope = query.channel_scope.as_deref();

        let mut items: Vec<MemoryRecord> = self
            .load_all()?
            .into_iter()
            .filter(|r| query.include_non_accepted || r.is_accepted())
            .filter(|r| match channel_scope {
                Some(ch) => r.source.channel_id == ch,
                None => true,
            })
            .filter(|r| {
                if types.is_empty() {
                    return true;
                }
                types.contains(&r.record_type.to_ascii_lowercase())
            })
            .filter(|r| {
                if tags.is_empty() {
                    return true;
                }
                let rtags: Vec<String> = r.tags.iter().map(|t| t.to_ascii_lowercase()).collect();
                tags.iter().any(|t| rtags.contains(t))
            })
            .filter(|r| {
                if text_tokens.is_empty() && text_raw.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{}\n{}\n{}",
                    r.summary.to_lowercase(),
                    r.detail.to_lowercase(),
                    r.tags.join(" ").to_lowercase()
                );
                // Token overlap OR whole-string substring (CJK-friendly).
                if text_tokens.iter().any(|t| haystack.contains(t)) {
                    return true;
                }
                if !text_raw.is_empty() && haystack.contains(&text_raw) {
                    return true;
                }
                false
            })
            .collect();
        if query.limit > 0 {
            items.truncate(query.limit);
        }
        Ok(items)
    }

    fn append(&self, record: &MemoryRecord) -> Result<(), String> {
        fs::create_dir_all(&self.root)
            .map_err(|err| format!("create {}: {err}", self.root.display()))?;
        let line =
            serde_json::to_string(record).map_err(|err| format!("serialize record: {err}"))?;
        let path = self.shard_path_for_ts(&record.ts);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|err| format!("open {}: {err}", path.display()))?;
        writeln!(file, "{line}").map_err(|err| format!("write {}: {err}", path.display()))?;
        Ok(())
    }
}

fn read_jsonl_file(path: &Path) -> Result<Vec<MemoryRecord>, String> {
    let file = fs::File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (lineno, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                tracing::warn!(path = %path.display(), lineno, %err,
                    "jsonl read error; skipping remainder of file");
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<MemoryRecord>(trimmed) {
            Ok(rec) => items.push(rec),
            Err(err) => {
                tracing::warn!(path = %path.display(), lineno, %err,
                    "malformed memory record; skipping line");
            }
        }
    }
    Ok(items)
}

/// Tokenize free text: split on non-alphanumeric (Unicode-aware), lowercase
/// (ASCII cast), drop tokens shorter than 2, dedup, cap at 16 terms. CJK
/// runs stay whole (each hanzi is alphanumeric), so substring matching at
/// query time still hits them via the raw-string fallback.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for token in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let t = token.to_lowercase();
        if t.len() < 2 {
            continue;
        }
        if seen.insert(t.clone()) {
            out.push(t);
        }
        if out.len() >= 16 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::record::MemorySource;

    fn mk(id: &str, ts: &str, summary: &str, channel: &str, confidence: &str) -> MemoryRecord {
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
            source: MemorySource {
                channel_id: channel.into(),
                thread_id: String::new(),
                message_ids: vec![],
            },
            tags: vec![],
        }
    }

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("loom-memory-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn append_and_list_recent() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk("a", "2026-04-01T10:00:00Z", "first", "ch1", "high"))
            .unwrap();
        store
            .append(&mk("b", "2026-04-02T10:00:00Z", "second", "ch1", "low"))
            .unwrap();
        let got = store.list_recent(10).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, "b"); // newer first
        assert_eq!(got[1].id, "a");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn month_sharding_splits_files() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk("a", "2026-03-31T10:00:00Z", "mar", "ch1", "high"))
            .unwrap();
        store
            .append(&mk("b", "2026-04-01T10:00:00Z", "apr", "ch1", "high"))
            .unwrap();
        let mut files: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files, vec!["2026-03.jsonl", "2026-04.jsonl"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn channel_scope_filters_records() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk(
                "a",
                "2026-04-01T10:00:00Z",
                "private",
                "ch_secret",
                "high",
            ))
            .unwrap();
        store
            .append(&mk(
                "b",
                "2026-04-02T10:00:00Z",
                "public",
                "ch_public",
                "high",
            ))
            .unwrap();
        let q = MemoryQuery {
            channel_scope: Some("ch_public".into()),
            limit: 10,
            ..Default::default()
        };
        let got = store.query(&q).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "b");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn query_text_overlap_and_cjk_substring() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk(
                "a",
                "2026-04-01T10:00:00Z",
                "user prefers Rust",
                "ch1",
                "high",
            ))
            .unwrap();
        store
            .append(&mk(
                "b",
                "2026-04-02T10:00:00Z",
                "用户喜欢中文响应",
                "ch1",
                "high",
            ))
            .unwrap();

        let ascii = store
            .query(&MemoryQuery {
                text: Some("rust".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ascii.len(), 1);
        assert_eq!(ascii[0].id, "a");

        let cjk = store
            .query(&MemoryQuery {
                text: Some("中文".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(cjk.len(), 1);
        assert_eq!(cjk[0].id, "b");

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn non_accepted_excluded_by_default() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        let mut r = mk("a", "2026-04-01T10:00:00Z", "draft", "ch1", "high");
        r.status = "pending".into();
        store.append(&r).unwrap();
        assert!(store.list_recent(10).unwrap().is_empty());
        let all = store
            .query(&MemoryQuery {
                include_non_accepted: true,
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all.len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        let good = mk("a", "2026-04-01T10:00:00Z", "ok", "ch1", "high");
        store.append(&good).unwrap();
        // Append garbage manually.
        let path = root.join("2026-04.jsonl");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "this is not json").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "{{\"partial\": true}}").unwrap(); // missing required fields
        let got = store.list_recent(10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "a");
        std::fs::remove_dir_all(root).ok();
    }
}
