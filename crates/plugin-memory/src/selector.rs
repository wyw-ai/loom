//! Per-turn record selection. The selector is a pure policy that reads from
//! a [`MemoryStore`]; the spec it carries is a [`proto::methods::MemorySpec`].

use super::jsonl_store::tokenize;
use super::record::{confidence_rank, MemoryQuery, MemoryRecord};
use super::store::MemoryStore;
use proto::methods::MemorySpec;

#[derive(Debug, Clone)]
pub struct MemorySelector {
    config: MemorySpec,
    /// Channel to scope queries by when `config.query.per_channel == true`.
    /// `None` means "don't scope" (the per_channel flag is still respected,
    /// but without a channel id there is nothing to scope by — we fall open).
    channel: Option<String>,
}

impl MemorySelector {
    pub fn new(config: MemorySpec, channel: Option<String>) -> Self {
        Self { config, channel }
    }

    fn channel_scope(&self) -> Option<String> {
        if self.config.query.per_channel {
            self.channel.clone()
        } else {
            None
        }
    }

    /// Bootstrap pool: recent accepted records, re-ranked by (confidence, ts)
    /// so high-confidence facts bubble up even when slightly older. Size =
    /// `bootstrapTopK`.
    pub fn select_bootstrap(&self, store: &dyn MemoryStore) -> Result<Vec<MemoryRecord>, String> {
        let top_k = self.config.query.bootstrap_top_k;
        if top_k == 0 {
            return Ok(Vec::new());
        }
        // Grab a wider candidate window (5x) then re-rank; ensures high-conf
        // records aren't starved by a burst of fresh low-conf ones.
        let window = top_k.saturating_mul(5).max(top_k);
        let candidates = match self.channel_scope() {
            Some(ch) => store.query(&MemoryQuery {
                limit: window,
                channel_scope: Some(ch),
                ..Default::default()
            })?,
            None => store.list_recent(window)?,
        };
        let mut ranked = candidates;
        ranked.sort_by(|a, b| {
            let rank = confidence_rank(&b.confidence).cmp(&confidence_rank(&a.confidence));
            if rank.is_eq() {
                b.ts.cmp(&a.ts)
            } else {
                rank
            }
        });
        ranked.truncate(top_k);
        Ok(ranked)
    }

    /// Turn pool: keyword overlap with current message + thread context,
    /// with recent fallback if the query finds nothing. Size = `turnTopK`.
    pub fn select_for_turn(
        &self,
        store: &dyn MemoryStore,
        latest_message: &str,
        thread_context: &str,
    ) -> Result<Vec<MemoryRecord>, String> {
        let top_k = self.config.query.turn_top_k;
        if top_k == 0 {
            return Ok(Vec::new());
        }
        let query_text = build_turn_query(latest_message, thread_context);
        let channel_scope = self.channel_scope();
        if query_text.trim().is_empty() {
            let recent = match channel_scope.as_ref() {
                Some(ch) => store.query(&MemoryQuery {
                    channel_scope: Some(ch.clone()),
                    limit: top_k,
                    ..Default::default()
                })?,
                None => store.list_recent(top_k)?,
            };
            return Ok(recent);
        }
        let mut items = store.query(&MemoryQuery {
            text: Some(query_text),
            channel_scope: channel_scope.clone(),
            limit: top_k,
            ..Default::default()
        })?;
        if items.is_empty() {
            items = match channel_scope {
                Some(ch) => store.query(&MemoryQuery {
                    channel_scope: Some(ch),
                    limit: top_k,
                    ..Default::default()
                })?,
                None => store.list_recent(top_k)?,
            };
        }
        items.truncate(top_k);
        Ok(items)
    }
}

/// Convenience: run both selections in one call. Returns `(bootstrap, turn)`.
pub fn load_bootstrap_and_turn(
    selector: &MemorySelector,
    store: &dyn MemoryStore,
    latest_message: &str,
    thread_context: &str,
) -> Result<(Vec<MemoryRecord>, Vec<MemoryRecord>), String> {
    let bootstrap = selector.select_bootstrap(store)?;
    let turn = selector.select_for_turn(store, latest_message, thread_context)?;
    Ok((bootstrap, turn))
}

fn build_turn_query(latest_message: &str, thread_context: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut terms = Vec::new();
    for token in tokenize(latest_message)
        .into_iter()
        .chain(tokenize(thread_context))
    {
        if token.len() < 3 {
            continue;
        }
        if seen.insert(token.clone()) {
            terms.push(token);
        }
        if terms.len() >= 12 {
            break;
        }
    }
    terms.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonl_store::JsonlMemoryStore;
    use crate::record::MemorySource;
    use proto::methods::{MemoryDeliverySpec, MemoryQuerySpec, MemorySpec, MemoryStoreSpec};

    fn tmpdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("loom-selector-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

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
                ..Default::default()
            },
            tags: vec![],
        }
    }

    fn spec(per_channel: bool, boot: usize, turn: usize) -> MemorySpec {
        MemorySpec {
            store: MemoryStoreSpec::default(),
            query: MemoryQuerySpec {
                mode: "heuristic".into(),
                bootstrap_top_k: boot,
                turn_top_k: turn,
                per_channel,
            },
            delivery: MemoryDeliverySpec::default(),
            extraction: Default::default(),
            compaction: Default::default(),
        }
    }

    #[test]
    fn bootstrap_promotes_high_confidence() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk("a", "2026-04-05T10:00:00Z", "new low", "ch1", "low"))
            .unwrap();
        store
            .append(&mk(
                "b",
                "2026-04-04T10:00:00Z",
                "older high",
                "ch1",
                "high",
            ))
            .unwrap();
        store
            .append(&mk(
                "c",
                "2026-04-03T10:00:00Z",
                "older med",
                "ch1",
                "medium",
            ))
            .unwrap();
        let sel = MemorySelector::new(spec(false, 3, 0), None);
        let boot = sel.select_bootstrap(&store).unwrap();
        assert_eq!(boot[0].id, "b"); // high first
        assert_eq!(boot[1].id, "c");
        assert_eq!(boot[2].id, "a");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn turn_keyword_match() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk(
                "a",
                "2026-04-05T10:00:00Z",
                "user prefers Rust over Go for networking",
                "ch1",
                "high",
            ))
            .unwrap();
        store
            .append(&mk(
                "b",
                "2026-04-04T10:00:00Z",
                "unrelated gardening tip",
                "ch1",
                "low",
            ))
            .unwrap();
        let sel = MemorySelector::new(spec(false, 0, 4), None);
        let turn = sel
            .select_for_turn(&store, "let's talk about Rust performance", "")
            .unwrap();
        assert_eq!(turn.len(), 1);
        assert_eq!(turn[0].id, "a");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn per_channel_scope_prevents_leak() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk(
                "a",
                "2026-04-05T10:00:00Z",
                "secret",
                "ch_secret",
                "high",
            ))
            .unwrap();
        store
            .append(&mk(
                "b",
                "2026-04-04T10:00:00Z",
                "public fact",
                "ch_public",
                "high",
            ))
            .unwrap();
        let sel = MemorySelector::new(spec(true, 8, 0), Some("ch_public".into()));
        let boot = sel.select_bootstrap(&store).unwrap();
        assert_eq!(boot.len(), 1);
        assert_eq!(boot[0].id, "b");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn per_channel_false_includes_everything() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk("a", "2026-04-05T10:00:00Z", "note", "ch_a", "high"))
            .unwrap();
        store
            .append(&mk("b", "2026-04-04T10:00:00Z", "note", "ch_b", "high"))
            .unwrap();
        let sel = MemorySelector::new(spec(false, 8, 0), Some("ch_a".into()));
        let boot = sel.select_bootstrap(&store).unwrap();
        assert_eq!(boot.len(), 2);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn turn_falls_back_to_recent_when_no_match() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk(
                "a",
                "2026-04-05T10:00:00Z",
                "unrelated x",
                "ch1",
                "high",
            ))
            .unwrap();
        let sel = MemorySelector::new(spec(false, 0, 3), None);
        let turn = sel
            .select_for_turn(&store, "completely orthogonal query", "")
            .unwrap();
        assert_eq!(turn.len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn empty_query_returns_recent() {
        let root = tmpdir();
        let store = JsonlMemoryStore::new(root.clone());
        store
            .append(&mk("a", "2026-04-05T10:00:00Z", "x", "ch1", "high"))
            .unwrap();
        let sel = MemorySelector::new(spec(false, 0, 5), None);
        let turn = sel.select_for_turn(&store, "", "").unwrap();
        assert_eq!(turn.len(), 1);
        std::fs::remove_dir_all(root).ok();
    }
}
