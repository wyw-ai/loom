use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use proto::types::*;
use serde::{Deserialize, Serialize};

/// Adjacently tagged: the variant name lives under `op`, the payload under
/// `data`. The previous schema used `tag = "kind"`, which collided with
/// inner struct fields that are also named `kind` (e.g. `Actor.kind`,
/// `Receipt.kind`) — serde would happily emit duplicate keys, but every
/// reader (including ours) rejected them. The legacy lines are migrated to
/// the new shape on first load; see `migrate_legacy_format`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "data", rename_all = "snake_case")]
pub enum Mutation {
    ActorUpsert(Actor),
    #[serde(alias = "space_create")]
    ChannelCreate(Channel),
    #[serde(alias = "conversation_create")]
    ThreadCreate(Thread),
    TurnOpen(Turn),
    TurnClose {
        turn_id: String,
        status: TurnStatus,
        closed_at: Timestamp,
    },
    EventAppend(Event),
    MembershipUpsert(Membership),
    DeliveryUpsert(Delivery),
    ReceiptRecord(Receipt),
    ArtifactCreate(Artifact),
    #[serde(alias = "conversation_root_set")]
    ThreadRootSet {
        #[serde(alias = "conversation_id")]
        thread_id: String,
        root_event_id: String,
    },
    /// Turn-private trace frame. Stored on the journal so that the turn
    /// owner can re-read the frames after a reconnect via `turn/trace.read`.
    /// These never enter `events_by_scope` and are never broadcast on a
    /// scope subscription.
    TraceAppend(proto::types::trace::TraceFrame),
    ChannelUpdate {
        channel_id: String,
        title: String,
    },
    ChannelDelete {
        channel_id: String,
    },
    ThreadUpdate {
        thread_id: String,
        title: String,
    },
    ThreadDelete {
        thread_id: String,
    },
    /// Add `actor_id` to `channel_id`'s member set. Idempotent on replay.
    ChannelGrant {
        channel_id: String,
        actor_id: String,
    },
    /// Remove `actor_id` from `channel_id`'s member set. Idempotent on replay.
    ChannelRevoke {
        channel_id: String,
        actor_id: String,
    },
}

/// Variant names accepted as legacy envelope discriminators. Keep in sync
/// with `Mutation`'s snake_case variant names AND any pre-rename names that
/// can still appear on disk (e.g. `space_create`, `conversation_create`,
/// `conversation_root_set` from before the channel/thread rename).
const LEGACY_VARIANTS: &[&str] = &[
    "actor_upsert",
    "space_create",
    "channel_create",
    "conversation_create",
    "thread_create",
    "turn_open",
    "turn_close",
    "event_append",
    "membership_upsert",
    "delivery_upsert",
    "receipt_record",
    "artifact_create",
    "conversation_root_set",
    "thread_root_set",
    "channel_grant",
    "channel_revoke",
];

pub struct Journal {
    path: PathBuf,
    file: Mutex<std::fs::File>,
}

impl Journal {
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Arc<Self>> {
        let path: PathBuf = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Repair the on-disk file (if needed) before holding it open for
        // append, so the rewrite is a clean replace.
        migrate_legacy_format(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        Ok(Arc::new(Self {
            path,
            file: Mutex::new(file),
        }))
    }

    #[allow(dead_code)] // used by store.rs test fixtures
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, mutation: &Mutation) -> std::io::Result<()> {
        let mut file = self.file.lock();
        let line = serde_json::to_string(mutation)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{line}")?;
        file.flush()
    }

    pub fn replay(&self) -> std::io::Result<Vec<Mutation>> {
        let file = OpenOptions::new().read(true).open(&self.path)?;
        let mut out = Vec::new();
        for (idx, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Mutation>(&line) {
                Ok(m) => out.push(m),
                Err(err) => {
                    tracing::warn!(line = idx + 1, %err, "skipping unreadable journal line");
                }
            }
        }
        Ok(out)
    }
}

/// One-shot migration: if any line in the journal still uses the old
/// `{"kind":"<variant>",...}` envelope, rewrite the whole file in place
/// using the new `{"op":"<variant>","data":{...}}` shape. Lines that look
/// neither legacy nor new (truly malformed) are kept verbatim so `replay`
/// still surfaces them as warnings.
fn migrate_legacy_format(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(path)?;
    if raw.is_empty() {
        return Ok(());
    }
    let mut needs_rewrite = false;
    let mut migrated_lines = Vec::with_capacity(raw.lines().count());
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            migrated_lines.push(String::new());
            continue;
        }
        match try_convert_legacy(trimmed) {
            Some(new_line) => {
                needs_rewrite = true;
                migrated_lines.push(new_line);
            }
            None => migrated_lines.push(line.to_string()),
        }
    }
    if !needs_rewrite {
        return Ok(());
    }

    // Atomic replace: write to <path>.migrating then rename.
    let tmp = path.with_extension("jsonl.migrating");
    {
        let mut tmp_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        for line in &migrated_lines {
            writeln!(tmp_file, "{line}")?;
        }
        tmp_file.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    tracing::info!(
        lines = migrated_lines.len(),
        "migrated journal lines from `kind`-tagged to `op`/`data` envelope"
    );
    Ok(())
}

/// Convert a single legacy `{"kind":"<variant>",...}` line to the new
/// `{"op":"<variant>","data":{...}}` shape. Returns `None` when the line
/// doesn't match the legacy header (already migrated, or unrelated junk).
///
/// We deliberately work with raw text rather than `serde_json::Value`
/// because the legacy lines are *already* invalid JSON for any strict
/// parser (duplicate `kind` keys), so a Value-based round-trip would lose
/// the discriminator.
fn try_convert_legacy(line: &str) -> Option<String> {
    for variant in LEGACY_VARIANTS {
        let header = format!("{{\"kind\":\"{}\"", variant);
        if !line.starts_with(&header) {
            continue;
        }
        let after = &line[header.len()..];
        // Strip the closing `}` that ended the envelope.
        let body = after.strip_suffix('}')?;
        // Either `"foo":...` (variant has fields after the tag) or empty
        // (variant has no payload — none in this enum, but safe to handle).
        let inner = body.strip_prefix(',').unwrap_or(body);
        let new_line = if inner.is_empty() {
            format!("{{\"op\":\"{variant}\",\"data\":{{}}}}")
        } else {
            format!("{{\"op\":\"{variant}\",\"data\":{{{inner}}}}}")
        };
        return Some(new_line);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_actor_upsert_with_inner_kind() {
        let legacy = r#"{"kind":"actor_upsert","id":"actor_x","kind":"human","displayName":"X","capabilities":{}}"#;
        let new = try_convert_legacy(legacy).unwrap();
        assert_eq!(
            new,
            r#"{"op":"actor_upsert","data":{"id":"actor_x","kind":"human","displayName":"X","capabilities":{}}}"#,
        );
        // Round-trips through the new schema.
        let _: Mutation = serde_json::from_str(&new).unwrap();
    }

    #[test]
    fn converts_struct_variant() {
        let legacy = r#"{"kind":"turn_close","turn_id":"t1","status":"closed","closed_at":"2026-04-20T00:00:00Z"}"#;
        let new = try_convert_legacy(legacy).unwrap();
        let _: Mutation = serde_json::from_str(&new).unwrap();
    }

    #[test]
    fn leaves_already_migrated_lines_alone() {
        let already = r#"{"op":"space_create","data":{"id":"s","title":"T"}}"#;
        assert!(try_convert_legacy(already).is_none());
    }

    /// Pre-rename journals carry `{"op":"space_create",...}` envelopes; the
    /// serde alias on `ChannelCreate` should accept them so existing data
    /// keeps replaying after the rename.
    #[test]
    fn space_create_envelope_deserializes_as_channel_create() {
        let line = r#"{"op":"space_create","data":{"id":"chan_demo","title":"Demo"}}"#;
        let m: Mutation = serde_json::from_str(line).unwrap();
        match m {
            Mutation::ChannelCreate(c) => {
                assert_eq!(c.id, "chan_demo");
                assert_eq!(c.title, "Demo");
            }
            other => panic!("expected ChannelCreate, got {other:?}"),
        }
    }

    /// Same story for conversation_create → ThreadCreate, and the inner
    /// `spaceId` field aliases onto the new `channel_id` field.
    #[test]
    fn conversation_create_envelope_deserializes_as_thread_create() {
        let line = r#"{"op":"conversation_create","data":{"id":"thread_demo","spaceId":"chan_demo","title":"task1"}}"#;
        let m: Mutation = serde_json::from_str(line).unwrap();
        match m {
            Mutation::ThreadCreate(t) => {
                assert_eq!(t.id, "thread_demo");
                assert_eq!(t.channel_id, "chan_demo");
                assert_eq!(t.title, "task1");
                assert!(t.root_event_id.is_none());
            }
            other => panic!("expected ThreadCreate, got {other:?}"),
        }
    }

    /// `conversation_root_set` carries `conversation_id`; the variant alias
    /// + field alias must both fire to deserialize as `ThreadRootSet`.
    #[test]
    fn conversation_root_set_envelope_deserializes_as_thread_root_set() {
        let line = r#"{"op":"conversation_root_set","data":{"conversation_id":"thread_demo","root_event_id":"evt_1"}}"#;
        let m: Mutation = serde_json::from_str(line).unwrap();
        match m {
            Mutation::ThreadRootSet {
                thread_id,
                root_event_id,
            } => {
                assert_eq!(thread_id, "thread_demo");
                assert_eq!(root_event_id, "evt_1");
            }
            other => panic!("expected ThreadRootSet, got {other:?}"),
        }
    }

    /// End-to-end replay against the real ./data/journal.jsonl committed in
    /// the repo. Skipped when the file isn't present (e.g. CI without seed
    /// data). Catches any field/variant rename that would break existing
    /// users on first server restart after pulling the rename commit.
    #[test]
    fn replays_real_pre_rename_journal_if_present() {
        let candidate = std::path::Path::new("../../data/journal.jsonl");
        if !candidate.exists() {
            eprintln!("skipping: no ./data/journal.jsonl available");
            return;
        }
        let raw = std::fs::read_to_string(candidate).unwrap();
        let mut total = 0;
        let mut failures: Vec<(usize, String)> = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            total += 1;
            if let Err(e) = serde_json::from_str::<Mutation>(line) {
                failures.push((i + 1, e.to_string()));
            }
        }
        assert!(
            failures.is_empty(),
            "{}/{} legacy journal lines failed to parse: first={:?}",
            failures.len(),
            total,
            failures.first()
        );
    }

    /// EventAppend carrying a legacy `"kind":"conversation"` scope still
    /// loads via the `ScopeKind::Thread` alias.
    #[test]
    fn event_append_with_conversation_scope_loads() {
        let line = r#"{"op":"event_append","data":{"id":"evt_1","type":"content.add","actorId":"actor_a","scope":{"kind":"conversation","id":"thread_demo"},"turnId":null,"seq":1,"occurredAt":"2026-04-20T00:00:00Z","payload":{"contentType":"text/markdown","text":"hi"},"relations":[]}}"#;
        let m: Mutation = serde_json::from_str(line).unwrap();
        match m {
            Mutation::EventAppend(ev) => {
                assert!(matches!(ev.scope.kind, ScopeKind::Thread));
                assert_eq!(ev.scope.id, "thread_demo");
            }
            other => panic!("expected EventAppend, got {other:?}"),
        }
    }
}
