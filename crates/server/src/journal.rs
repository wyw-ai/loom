use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use proto::types::*;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Adjacently tagged: the variant name lives under `op`, the payload under
/// `data`. The previous schema used `tag = "kind"`, which collided with
/// inner struct fields that are also named `kind` (e.g. `Actor.kind`) —
/// serde would happily emit duplicate keys, but every reader (including ours)
/// rejected them. The legacy lines are migrated to the new shape on first
/// load; see `migrate_legacy_format`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "data", rename_all = "snake_case")]
pub enum Mutation {
    ActorUpsert(Actor),
    ActorDelete {
        actor_id: String,
    },
    ChannelCreate(Channel),
    ActorGroupUpsert(ActorGroup),
    ActorGroupDelete {
        group_id: String,
    },
    ActorPresenceUpsert(ActorPresence),
    ThreadCreate(Thread),
    TaskUpsert(Task),
    TaskAssignmentUpsert(TaskAssignment),
    TaskRefUpsert(TaskRef),
    TaskArtifactLinkUpsert(TaskArtifactLink),
    TaskFactUpsert(TaskFact),
    TaskProjectionUpsert(TaskProjection),
    WorkspaceLeaseUpsert(WorkspaceLease),
    TaskChangeUpsert(TaskChange),
    TaskChangeDeliveryUpsert(TaskChangeDelivery),
    TurnOpen(Turn),
    TurnClose {
        turn_id: String,
        status: TurnStatus,
        closed_at: Timestamp,
    },
    RunUpsert(Run),
    RunFrameAppend(RunFrame),
    AgentConfigVersionPublish(AgentConfigVersion),
    AgentConfigActivationUpsert(AgentConfigActivation),
    CoordinationSessionUpsert(CoordinationSession),
    CoordinationStepAppend(CoordinationStep),
    MessageAppend(Message),
    MessageUpdate(Message),
    EventAppend(Event),
    MembershipUpsert(Membership),
    ChannelMemberConfigUpsert(ChannelMemberConfig),
    ChannelMemberConfigDelete {
        channel_id: String,
        actor_id: String,
    },
    DeliveryUpsert(Delivery),
    MachineCommandUpsert(MachineCommand),
    ReminderUpsert(Reminder),
    ArtifactCreate(Artifact),
    /// Legacy turn-private trace frame retained for journal replay. New agent
    /// runtime traces are stored as `RunFrameAppend`.
    TraceAppend(proto::types::trace::TraceFrame),
    ChannelUpdate {
        channel_id: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        topic: Option<String>,
    },
    ChannelDelete {
        channel_id: String,
    },
    /// Set or replace the channel-level `instructions`. Idempotent on replay.
    ChannelInstructionSet {
        channel_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
        /// Actor id of the member who performed the edit. Forward-compat
        /// default `None` so legacy journals replay cleanly.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modified_by: Option<String>,
        /// Server-side UTC timestamp of the edit. Forward-compat default
        /// `None` for legacy journals.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modified_at: Option<Timestamp>,
    },
    ThreadUpdate {
        thread_id: String,
        title: String,
    },
    ThreadArchive {
        thread_id: String,
        archived_at: Option<Timestamp>,
    },
    ThreadDelete {
        thread_id: String,
    },
    /// Set or replace the thread-level `instructions`. Idempotent on replay.
    ThreadInstructionSet {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modified_by: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modified_at: Option<Timestamp>,
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
/// with `Mutation`'s snake_case variant names.
#[allow(dead_code)]
const LEGACY_VARIANTS: &[&str] = &[
    "actor_upsert",
    "actor_delete",
    "channel_create",
    "actor_group_upsert",
    "actor_group_delete",
    "thread_create",
    "task_upsert",
    "task_assignment_upsert",
    "task_ref_upsert",
    "task_artifact_link_upsert",
    "task_fact_upsert",
    "task_projection_upsert",
    "workspace_lease_upsert",
    "task_change_upsert",
    "task_change_delivery_upsert",
    "turn_open",
    "turn_close",
    "run_upsert",
    "run_frame_append",
    "agent_config_version_publish",
    "agent_config_activation_upsert",
    "coordination_session_upsert",
    "coordination_step_append",
    "event_append",
    "membership_upsert",
    "delivery_upsert",
    "machine_command_upsert",
    "reminder_upsert",
    "artifact_create",
    "channel_grant",
    "channel_revoke",
    "thread_archive",
];

#[allow(dead_code)]
enum JournalStorage {
    Jsonl { file: Mutex<std::fs::File> },
    Sqlite { conn: Mutex<Connection> },
}

pub struct Journal {
    path: PathBuf,
    storage: JournalStorage,
}

impl Journal {
    #[allow(dead_code)]
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
            storage: JournalStorage::Jsonl {
                file: Mutex::new(file),
            },
        }))
    }

    pub fn open_sqlite(path: impl Into<PathBuf>) -> std::io::Result<Arc<Self>> {
        let path: PathBuf = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path).map_err(sqlite_io)?;
        init_sqlite_schema(&conn).map_err(sqlite_io)?;
        Ok(Arc::new(Self {
            path,
            storage: JournalStorage::Sqlite {
                conn: Mutex::new(conn),
            },
        }))
    }

    #[allow(dead_code)] // used by store.rs test fixtures
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, mutation: &Mutation) -> std::io::Result<()> {
        let line = serde_json::to_string(mutation)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        match &self.storage {
            JournalStorage::Jsonl { file } => {
                let mut file = file.lock();
                writeln!(file, "{line}")?;
                file.flush()
            }
            JournalStorage::Sqlite { conn } => {
                let conn = conn.lock();
                conn.execute(
                    "insert into journal_records (record_json) values (?1)",
                    params![line],
                )
                .map(|_| ())
                .map_err(sqlite_io)
            }
        }
    }

    pub fn replay(&self) -> std::io::Result<Vec<Mutation>> {
        match &self.storage {
            JournalStorage::Jsonl { .. } => {
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
            JournalStorage::Sqlite { conn } => {
                let conn = conn.lock();
                let mut stmt = conn
                    .prepare("select id, record_json from journal_records order by id")
                    .map_err(sqlite_io)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(sqlite_io)?;
                let mut out = Vec::new();
                for row in rows {
                    let (id, line) = row.map_err(sqlite_io)?;
                    match serde_json::from_str::<Mutation>(&line) {
                        Ok(m) => out.push(m),
                        Err(err) => {
                            tracing::warn!(rowid = id, %err, "skipping unreadable sqlite journal record");
                        }
                    }
                }
                Ok(out)
            }
        }
    }
}

fn init_sqlite_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(
        r#"
        create table if not exists schema_meta (
            key text primary key,
            value text not null
        );
        insert into schema_meta (key, value)
            values ('storage_format', 'sqlite_journal_v1')
            on conflict(key) do update set value = excluded.value;
        create table if not exists journal_records (
            id integer primary key autoincrement,
            record_json text not null,
            created_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
        create table if not exists actors (
            id text primary key,
            record_json text
        );
        create table if not exists actor_groups (
            id text primary key,
            channel_id text not null,
            name text not null,
            wake_agents integer not null default 0,
            record_json text,
            unique(channel_id, name)
        );
        create table if not exists actor_group_members (
            group_id text not null,
            actor_id text not null,
            primary key (group_id, actor_id)
        );
        create table if not exists actor_presences (
            actor_id text not null,
            channel_id text not null,
            thread_id text not null,
            following integer not null default 0,
            muted integer not null default 0,
            attention_policy text not null default '',
            record_json text,
            primary key (actor_id, thread_id)
        );
        create table if not exists channels (
            id text primary key,
            record_json text
        );
        create table if not exists channel_members (
            channel_id text not null,
            actor_id text not null,
            primary key (channel_id, actor_id)
        );
        create table if not exists messages (
            id text primary key,
            target text not null,
            author_actor_id text not null,
            body text not null,
            created_at text not null,
            record_json text
        );
        create table if not exists message_mentions (
            message_id text not null,
            actor_or_group_id text not null,
            kind text not null,
            byte_start integer not null,
            byte_end integer not null,
            unique(message_id, actor_or_group_id, byte_start, byte_end)
        );
        create table if not exists threads (
            id text primary key,
            root_message_id text,
            channel_id text not null,
            record_json text
        );
        create table if not exists tasks (
            id text primary key,
            source_message_id text,
            status text,
            owner_actor_id text,
            record_json text
        );
        create table if not exists coordination_sessions (
            id text primary key,
            status text,
            revision integer not null default 0,
            record_json text
        );
        create table if not exists coordination_steps (
            id text primary key,
            session_id text not null,
            actor_id text not null,
            base_revision integer not null,
            status text not null,
            record_json text
        );
        create table if not exists agent_runs (
            id text primary key,
            actor_id text not null,
            status text not null,
            record_json text
        );
        create table if not exists deliveries (
            source_id text not null,
            actor_id text not null,
            state text,
            record_json text,
            primary key (source_id, actor_id)
        );
        create table if not exists read_states (
            actor_id text not null,
            scope_kind text not null,
            scope_id text not null,
            last_read_message_id text,
            primary key (actor_id, scope_kind, scope_id)
        );
        create table if not exists artifacts (
            id text primary key,
            uri text not null,
            record_json text
        );
        create table if not exists agent_config_versions (
            id text primary key,
            actor_id text not null,
            record_json text
        );
        create table if not exists agent_config_activations (
            actor_id text primary key,
            version_id text not null,
            activated_at text not null
        );
        create virtual table if not exists messages_fts using fts5(id, body);
        "#,
    )?;
    Ok(())
}

fn sqlite_io(err: rusqlite::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, err)
}

/// One-shot migration: if any line in the journal still uses the old
/// `{"kind":"<variant>",...}` envelope, rewrite the whole file in place
/// using the new `{"op":"<variant>","data":{...}}` shape. Lines that look
/// neither legacy nor new (truly malformed) are kept verbatim so `replay`
/// still surfaces them as warnings.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
        let already = r#"{"op":"channel_create","data":{"id":"chan_s","title":"T"}}"#;
        assert!(try_convert_legacy(already).is_none());
    }

    #[test]
    fn sqlite_journal_replays_appended_mutations() {
        let dir = std::env::temp_dir().join(format!(
            "loom-sqlite-journal-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let journal = Journal::open_sqlite(dir.join("loom.sqlite3")).expect("open sqlite journal");
        journal
            .append(&Mutation::ActorDelete {
                actor_id: "actor_old".into(),
            })
            .expect("append");

        let replayed = journal.replay().expect("replay");
        assert!(matches!(
            replayed.as_slice(),
            [Mutation::ActorDelete { actor_id }] if actor_id == "actor_old"
        ));
    }
}
