use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use chrono::{Datelike, Duration as ChronoDuration, Utc, Weekday};
use parking_lot::{Mutex, RwLock};
use proto::types::trace::TraceFrame;
use proto::types::*;
use thiserror::Error;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::journal::{Journal, Mutation};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone)]
pub enum StoreEvent {
    EventCreated(Event),
    TurnOpened(Turn),
    TurnClosed(Turn),
    ThreadCreated(Thread),
    TaskChanged(Task),
    TaskAssignmentChanged {
        assignment: TaskAssignment,
        task: Task,
    },
    ChannelCreated(Channel),
    ThreadUpdated(Thread),
    ArtifactPublished(Artifact),
    ReceiptRecorded(Receipt),
    DeliveryUpdated(Delivery),
    MachineCommandUpdated(MachineCommand),
    /// Turn-private trace frame. Carried on the same broadcast channel as
    /// scope events purely so the websocket layer can route it; the fanout
    /// must NOT broadcast it to scope subscribers — see `ws::fanout`.
    TraceAppended(TraceFrame),
    /// `actor_id` was just added to `channel_id`'s ACL. ws::fanout pushes
    /// this directly to the affected actor's connection (if any) — never
    /// broadcast to scope subscribers.
    ChannelGranted {
        channel: Channel,
        actor_id: String,
    },
    /// `actor_id` was just removed from `channel_id`'s ACL. Same routing
    /// as `ChannelGranted`.
    ChannelRevoked {
        channel_id: String,
        actor_id: String,
    },
}

impl StoreEvent {
    pub fn scope(&self) -> Option<ScopeRef> {
        match self {
            StoreEvent::EventCreated(e) => Some(e.scope.clone()),
            StoreEvent::TurnOpened(t) | StoreEvent::TurnClosed(t) => Some(t.scope.clone()),
            StoreEvent::ThreadCreated(c) => Some(ScopeRef {
                kind: ScopeKind::Channel,
                id: c.channel_id.clone(),
            }),
            StoreEvent::ThreadUpdated(t) => Some(ScopeRef {
                kind: ScopeKind::Channel,
                id: t.channel_id.clone(),
            }),
            StoreEvent::TaskChanged(t) => Some(ScopeRef {
                kind: ScopeKind::Channel,
                id: t.channel_id.clone(),
            }),
            StoreEvent::TaskAssignmentChanged { task, .. } => Some(ScopeRef {
                kind: ScopeKind::Channel,
                id: task.channel_id.clone(),
            }),
            StoreEvent::ArtifactPublished(_) => None,
            StoreEvent::ReceiptRecorded(_) => None,
            StoreEvent::DeliveryUpdated(_) => None,
            StoreEvent::MachineCommandUpdated(_) => None,
            // Trace frames are owner-private; ws fanout routes them by
            // turn owner, never by scope.
            StoreEvent::TraceAppended(_) => None,
            // ACL grants/revokes are direct-to-actor notifications; ws
            // fanout routes them via `send_to_actor`, not scope subs.
            StoreEvent::ChannelGranted { .. } => None,
            StoreEvent::ChannelRevoked { .. } => None,
            // ChannelCreated is fanned out to all connections (public) or
            // to the creator only (private) — routed in ws::fanout.
            StoreEvent::ChannelCreated(_) => None,
        }
    }
}

#[derive(Default)]
struct Inner {
    actors: HashMap<String, Actor>,
    channels: HashMap<String, Channel>,
    threads: HashMap<String, Thread>,
    tasks: HashMap<String, Task>,
    assignments: HashMap<String, TaskAssignment>,
    task_refs: HashMap<String, TaskRef>,
    task_artifact_links: HashMap<String, TaskArtifactLink>,
    task_facts: HashMap<String, TaskFact>,
    task_projections: HashMap<String, TaskProjection>,
    workspace_leases: HashMap<String, WorkspaceLease>,
    task_changes: HashMap<String, TaskChange>,
    task_change_deliveries: HashMap<(String, String), TaskChangeDelivery>,
    task_change_seq: u64,
    turns: HashMap<String, Turn>,
    /// scope ref -> ordered events
    events_by_scope: HashMap<ScopeRef, Vec<String>>,
    events: HashMap<String, Event>,
    /// (turn_id) -> next seq
    turn_seq: HashMap<String, u64>,
    /// implicit turn used when an event/append arrives with no turn_id, keyed by (actor_id, scope)
    memberships: HashMap<(String, ScopeRef), Membership>,
    deliveries: HashMap<(String, String), Delivery>,
    machine_commands: HashMap<String, MachineCommand>,
    receipts: HashMap<(String, String, ReceiptKind), Receipt>,
    reminders: HashMap<String, Reminder>,
    artifacts: HashMap<String, Artifact>,
    /// turn id -> ordered trace frames (owner-private; never broadcast)
    trace_by_turn: HashMap<String, Vec<TraceFrame>>,
    /// turn id -> next trace seq
    trace_seq: HashMap<String, u64>,
}

pub struct Store {
    journal: Arc<Journal>,
    inner: RwLock<Inner>,
    structure_lock: Mutex<()>,
    broadcaster: broadcast::Sender<StoreEvent>,
}

impl Store {
    pub fn open(journal: Arc<Journal>) -> StoreResult<Arc<Self>> {
        let (tx, _) = broadcast::channel(1024);
        let store = Arc::new(Self {
            journal: journal.clone(),
            inner: RwLock::new(Inner::default()),
            structure_lock: Mutex::new(()),
            broadcaster: tx,
        });
        store.replay()?;
        Ok(store)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StoreEvent> {
        self.broadcaster.subscribe()
    }

    fn emit(&self, event: StoreEvent) {
        let _ = self.broadcaster.send(event);
    }

    fn replay(&self) -> StoreResult<()> {
        for m in self.journal.replay()? {
            self.apply_replay(m);
        }
        Ok(())
    }

    fn apply_replay(&self, m: Mutation) {
        let mut inner = self.inner.write();
        apply(&mut inner, m);
    }

    // -------- Actors --------

    pub fn upsert_actor(&self, actor: Actor) -> StoreResult<Actor> {
        self.journal.append(&Mutation::ActorUpsert(actor.clone()))?;
        let mut inner = self.inner.write();
        inner.actors.insert(actor.id.clone(), actor.clone());
        Ok(actor)
    }

    pub fn delete_actor(&self, actor_id: &str) -> StoreResult<bool> {
        let mutation = Mutation::ActorDelete {
            actor_id: actor_id.to_string(),
        };
        self.journal.append(&mutation)?;
        let mut inner = self.inner.write();
        let existed = inner.actors.contains_key(actor_id);
        apply(&mut inner, mutation);
        Ok(existed)
    }

    pub fn get_actor(&self, id: &str) -> Option<Actor> {
        self.inner.read().actors.get(id).cloned()
    }

    pub fn list_actors(&self) -> Vec<Actor> {
        self.inner.read().actors.values().cloned().collect()
    }

    // -------- Channels --------

    /// Create a channel. When `creator_actor_id` is `Some`, the new channel
    /// is `Private` and the creator is its sole initial member. When `None`
    /// (legacy / test callers), the channel is `Public` so the ACL gate
    /// is skipped — matches the behavior journals predating the ACL roll-out
    /// replay with.
    pub fn create_channel(
        &self,
        title: String,
        creator_actor_id: Option<String>,
    ) -> StoreResult<Channel> {
        let (visibility, members) = match creator_actor_id {
            Some(id) => (ChannelVisibility::Private, vec![id]),
            None => (ChannelVisibility::Public, Vec::new()),
        };
        let channel = Channel {
            id: format!("chan_{}", short_id()),
            title,
            visibility,
            members,
            _meta: None,
        };
        self.journal
            .append(&Mutation::ChannelCreate(channel.clone()))?;
        self.inner
            .write()
            .channels
            .insert(channel.id.clone(), channel.clone());
        self.emit(StoreEvent::ChannelCreated(channel.clone()));
        Ok(channel)
    }

    /// `true` when `actor_id` is allowed to read/write `channel_id`.
    /// Public channels always return `true`; private channels check the
    /// `members` set. Returns `false` if the channel doesn't exist.
    pub fn is_channel_member(&self, channel_id: &str, actor_id: &str) -> bool {
        let inner = self.inner.read();
        let Some(ch) = inner.channels.get(channel_id) else {
            return false;
        };
        match ch.visibility {
            ChannelVisibility::Public => true,
            ChannelVisibility::Private => ch.members.iter().any(|m| m == actor_id),
        }
    }

    /// Resolve a `ScopeRef` to its owning channel and check membership.
    /// Returns `Ok(())` when allowed, `Err(InvalidState)` when denied,
    /// `Err(NotFound)` when the scope doesn't exist.
    pub fn check_scope_access(&self, scope: &ScopeRef, actor_id: &str) -> StoreResult<()> {
        let channel_id = match scope.kind {
            ScopeKind::Channel => scope.id.clone(),
            ScopeKind::Thread => match self.get_thread(&scope.id) {
                Some(t) => t.channel_id,
                None => return Err(StoreError::NotFound(format!("thread {}", scope.id))),
            },
        };
        if !self.is_channel_member(&channel_id, actor_id) {
            return Err(StoreError::InvalidState(format!(
                "actor {actor_id} is not a member of channel {channel_id}"
            )));
        }
        Ok(())
    }

    /// Add `actor_id` to `channel_id`'s member set. Idempotent. Emits
    /// `ChannelGranted` for the websocket layer to push to the affected
    /// actor.
    pub fn grant_channel(&self, channel_id: &str, actor_id: &str) -> StoreResult<Channel> {
        let updated = {
            let mut inner = self.inner.write();
            let ch = inner
                .channels
                .get_mut(channel_id)
                .ok_or_else(|| StoreError::NotFound(format!("channel {channel_id}")))?;
            if !ch.members.iter().any(|m| m == actor_id) {
                ch.members.push(actor_id.to_string());
            }
            ch.clone()
        };
        self.journal.append(&Mutation::ChannelGrant {
            channel_id: channel_id.to_string(),
            actor_id: actor_id.to_string(),
        })?;
        self.emit(StoreEvent::ChannelGranted {
            channel: updated.clone(),
            actor_id: actor_id.to_string(),
        });
        Ok(updated)
    }

    /// Remove `actor_id` from `channel_id`'s member set. Idempotent. Emits
    /// `ChannelRevoked` for the websocket layer.
    pub fn revoke_channel(&self, channel_id: &str, actor_id: &str) -> StoreResult<Channel> {
        let updated = {
            let mut inner = self.inner.write();
            let ch = inner
                .channels
                .get_mut(channel_id)
                .ok_or_else(|| StoreError::NotFound(format!("channel {channel_id}")))?;
            ch.members.retain(|m| m != actor_id);
            ch.clone()
        };
        self.journal.append(&Mutation::ChannelRevoke {
            channel_id: channel_id.to_string(),
            actor_id: actor_id.to_string(),
        })?;
        self.emit(StoreEvent::ChannelRevoked {
            channel_id: channel_id.to_string(),
            actor_id: actor_id.to_string(),
        });
        Ok(updated)
    }

    pub fn list_channels(&self) -> Vec<Channel> {
        self.inner.read().channels.values().cloned().collect()
    }

    pub fn get_channel(&self, id: &str) -> Option<Channel> {
        self.inner.read().channels.get(id).cloned()
    }

    pub fn update_channel(&self, id: &str, title: String) -> StoreResult<Channel> {
        if self.get_channel(id).is_none() {
            return Err(StoreError::NotFound(format!("channel {id}")));
        }
        self.journal.append(&Mutation::ChannelUpdate {
            channel_id: id.to_string(),
            title: title.clone(),
        })?;
        let mut inner = self.inner.write();
        let ch = inner
            .channels
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("channel {id}")))?;
        ch.title = title;
        Ok(ch.clone())
    }

    /// Default (`cascade = false`) refuses when the channel still contains
    /// threads — caller must delete children first. With `cascade = true`,
    /// every child thread is deleted (via the existing `delete_thread` path
    /// — soft-delete, events left orphaned) before the channel row is
    /// removed. Returns `(removed_channel, removed_thread_count)`.
    pub fn delete_channel(&self, id: &str, cascade: bool) -> StoreResult<(bool, u32)> {
        if self.get_channel(id).is_none() {
            return Err(StoreError::NotFound(format!("channel {id}")));
        }
        // Snapshot child ids under read lock so we can release it before the
        // per-thread `delete_thread` calls (each takes its own write lock).
        let child_ids: Vec<String> = self
            .inner
            .read()
            .threads
            .values()
            .filter(|t| t.channel_id == id)
            .map(|t| t.id.clone())
            .collect();

        if !child_ids.is_empty() && !cascade {
            return Err(StoreError::Conflict(format!(
                "channel {id} still has {} thread(s); delete them first or pass cascade=true",
                child_ids.len()
            )));
        }

        let mut deleted_threads: u32 = 0;
        for tid in &child_ids {
            // delete_thread is best-effort during cascade — a NotFound on a
            // child would mean someone else already removed it between our
            // snapshot and now, which is fine. Anything else (e.g. journal
            // write failure) bubbles up so we don't strand a half-deleted
            // channel.
            match self.delete_thread(tid) {
                Ok(true) => deleted_threads += 1,
                Ok(false) => {}
                Err(StoreError::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        self.journal.append(&Mutation::ChannelDelete {
            channel_id: id.to_string(),
        })?;
        let removed = {
            let mut inner = self.inner.write();
            let removed = inner.channels.remove(id).is_some();
            let task_ids: std::collections::HashSet<String> = inner
                .tasks
                .values()
                .filter(|task| task.channel_id == id)
                .map(|task| task.id.clone())
                .collect();
            inner.tasks.retain(|_, task| task.channel_id != id);
            inner
                .assignments
                .retain(|_, assignment| !task_ids.contains(&assignment.task_id));
            removed
        };
        Ok((removed, deleted_threads))
    }

    // -------- Threads --------

    pub fn create_thread(
        &self,
        channel_id: String,
        title: String,
        root_event_id: String,
    ) -> StoreResult<Thread> {
        let _guard = self.structure_lock.lock();
        self.create_thread_locked(channel_id, title, root_event_id)
    }

    fn create_thread_locked(
        &self,
        channel_id: String,
        title: String,
        root_event_id: String,
    ) -> StoreResult<Thread> {
        if self.get_channel(&channel_id).is_none() {
            return Err(StoreError::NotFound(format!("channel {channel_id}")));
        }
        let root = self
            .get_event(&root_event_id)
            .ok_or_else(|| StoreError::NotFound(format!("event {root_event_id}")))?;
        if root.scope.kind != ScopeKind::Channel || root.scope.id != channel_id {
            return Err(StoreError::InvalidState(format!(
                "thread root event {root_event_id} must belong to channel {channel_id}"
            )));
        }
        if let Some(existing) = self.find_thread_by_root(&channel_id, &root_event_id) {
            return Err(StoreError::Conflict(format!(
                "thread {} already uses root event {root_event_id}",
                existing.id
            )));
        }
        let thread = Thread {
            id: format!("thread_{}", short_id()),
            channel_id,
            title,
            root_event_id,
            archived_at: None,
            _meta: None,
        };
        self.journal
            .append(&Mutation::ThreadCreate(thread.clone()))?;
        self.inner
            .write()
            .threads
            .insert(thread.id.clone(), thread.clone());
        self.emit(StoreEvent::ThreadCreated(thread.clone()));
        Ok(thread)
    }

    pub fn list_threads(&self, channel_id: Option<&str>) -> Vec<Thread> {
        let inner = self.inner.read();
        inner
            .threads
            .values()
            .filter(|t| match channel_id {
                Some(id) => t.channel_id == id,
                None => true,
            })
            .cloned()
            .collect()
    }

    pub fn list_threads_filtered(&self, channel_id: Option<&str>, archived: bool) -> Vec<Thread> {
        let inner = self.inner.read();
        let mut threads = inner
            .threads
            .values()
            .filter(|t| match channel_id {
                Some(id) => t.channel_id == id,
                None => true,
            })
            .filter(|t| t.archived_at.is_some() == archived)
            .cloned()
            .collect::<Vec<_>>();
        if archived {
            threads.sort_by(|a, b| {
                b.archived_at
                    .cmp(&a.archived_at)
                    .then_with(|| a.title.cmp(&b.title))
            });
        }
        threads
    }

    pub fn get_thread(&self, id: &str) -> Option<Thread> {
        self.inner.read().threads.get(id).cloned()
    }

    pub fn find_thread_by_root(&self, channel_id: &str, root_event_id: &str) -> Option<Thread> {
        self.inner
            .read()
            .threads
            .values()
            .find(|t| t.channel_id == channel_id && t.root_event_id == root_event_id)
            .cloned()
    }

    pub fn update_thread(&self, id: &str, title: String) -> StoreResult<Thread> {
        if self.get_thread(id).is_none() {
            return Err(StoreError::NotFound(format!("thread {id}")));
        }
        self.journal.append(&Mutation::ThreadUpdate {
            thread_id: id.to_string(),
            title: title.clone(),
        })?;
        let mut inner = self.inner.write();
        let t = inner
            .threads
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("thread {id}")))?;
        t.title = title;
        let thread = t.clone();
        drop(inner);
        self.emit(StoreEvent::ThreadUpdated(thread.clone()));
        Ok(thread)
    }

    pub fn archive_thread(&self, id: &str, archived: bool) -> StoreResult<Thread> {
        if self.get_thread(id).is_none() {
            return Err(StoreError::NotFound(format!("thread {id}")));
        }
        let archived_at = archived.then(Utc::now);
        self.journal.append(&Mutation::ThreadArchive {
            thread_id: id.to_string(),
            archived_at,
        })?;
        let mut inner = self.inner.write();
        let t = inner
            .threads
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("thread {id}")))?;
        t.archived_at = archived_at;
        let thread = t.clone();
        drop(inner);
        self.emit(StoreEvent::ThreadUpdated(thread.clone()));
        Ok(thread)
    }

    /// Hard-removes the thread row and the events_by_scope index for its
    /// scope so the thread no longer appears in `list_threads` / `read_scope`.
    /// Event rows, deliveries, receipts, and trace frames are kept on disk;
    /// they become orphaned but harmless because their thread is gone.
    pub fn delete_thread(&self, id: &str) -> StoreResult<bool> {
        if self.get_thread(id).is_none() {
            return Err(StoreError::NotFound(format!("thread {id}")));
        }
        self.journal.append(&Mutation::ThreadDelete {
            thread_id: id.to_string(),
        })?;
        let mut inner = self.inner.write();
        let removed = inner.threads.remove(id).is_some();
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: id.to_string(),
        };
        inner.events_by_scope.remove(&scope);
        let task_ids: std::collections::HashSet<String> = inner
            .tasks
            .values()
            .filter(|task| task.canonical_thread_id == id)
            .map(|task| task.id.clone())
            .collect();
        inner.tasks.retain(|_, task| task.canonical_thread_id != id);
        inner
            .assignments
            .retain(|_, assignment| !task_ids.contains(&assignment.task_id));
        Ok(removed)
    }

    // -------- Tasks --------

    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &self,
        source_event_id: String,
        title: Option<String>,
        description: String,
        requester_actor_id: String,
        owner_actor_id: Option<String>,
        status: Option<TaskStatus>,
        parent_source_event_id: Option<String>,
        parent_task_id: Option<String>,
        practice_contract_epoch: Option<String>,
    ) -> StoreResult<Task> {
        let _guard = self.structure_lock.lock();
        let source = self
            .get_event(&source_event_id)
            .ok_or_else(|| StoreError::NotFound(format!("event {source_event_id}")))?;
        if source.scope.kind != ScopeKind::Channel {
            return Err(StoreError::InvalidState(format!(
                "task source event {source_event_id} must be a top-level channel event"
            )));
        }
        let channel_id = source.scope.id.clone();
        if !self.is_channel_member(&channel_id, &requester_actor_id) {
            return Err(StoreError::InvalidState(format!(
                "actor {requester_actor_id} is not a member of channel {channel_id}"
            )));
        }
        if let Some(owner) = owner_actor_id.as_ref() {
            if !self.is_channel_member(&channel_id, owner) {
                return Err(StoreError::InvalidState(format!(
                    "task owner {owner} is not a member of channel {channel_id}"
                )));
            }
        }
        if self.find_task_by_source(&source_event_id).is_some() {
            return Err(StoreError::Conflict(format!(
                "task already exists for source event {source_event_id}"
            )));
        }

        let task_title = title
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| event_title(&source));
        let canonical_thread_id = match self.find_thread_by_root(&channel_id, &source_event_id) {
            Some(thread) => thread.id,
            None => match self.create_thread_locked(
                channel_id.clone(),
                task_title.clone(),
                source_event_id.clone(),
            ) {
                Ok(thread) => thread.id,
                Err(StoreError::Conflict(_)) => {
                    self.find_thread_by_root(&channel_id, &source_event_id)
                        .ok_or_else(|| {
                            StoreError::Conflict(format!(
                                "thread already exists for root event {source_event_id}"
                            ))
                        })?
                        .id
                }
                Err(e) => return Err(e),
            },
        };

        let now = Utc::now();
        let number = self.next_task_number(&channel_id);
        let task = Task {
            id: format!("task_{}", short_id()),
            number,
            channel_id,
            source_event_id,
            canonical_thread_id,
            parent_source_event_id,
            parent_task_id,
            title: task_title,
            description,
            requester_actor_id,
            owner_actor_id: owner_actor_id.clone(),
            status: status.unwrap_or(if owner_actor_id.is_some() {
                TaskStatus::Claimed
            } else {
                TaskStatus::Todo
            }),
            result_summary: String::new(),
            artifact_ids: Vec::new(),
            assignment_ids: Vec::new(),
            practice_contract_epoch,
            created_at: now,
            updated_at: now,
            _meta: None,
        };
        self.journal.append(&Mutation::TaskUpsert(task.clone()))?;
        self.inner
            .write()
            .tasks
            .insert(task.id.clone(), task.clone());
        self.emit(StoreEvent::TaskChanged(task.clone()));
        Ok(task)
    }

    fn next_task_number(&self, channel_id: &str) -> u64 {
        self.inner
            .read()
            .tasks
            .values()
            .filter(|task| task.channel_id == channel_id)
            .map(|task| task.number)
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn get_task(&self, id: &str) -> Option<Task> {
        self.inner.read().tasks.get(id).cloned()
    }

    pub fn get_assignment(&self, id: &str) -> Option<TaskAssignment> {
        self.inner.read().assignments.get(id).cloned()
    }

    pub fn list_task_assignments(&self, task_id: &str) -> Vec<TaskAssignment> {
        let inner = self.inner.read();
        let mut rows: Vec<TaskAssignment> = inner
            .assignments
            .values()
            .filter(|assignment| assignment.task_id == task_id)
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        rows
    }

    pub fn find_task_by_source(&self, source_event_id: &str) -> Option<Task> {
        self.inner
            .read()
            .tasks
            .values()
            .find(|task| task.source_event_id == source_event_id)
            .cloned()
    }

    pub fn list_tasks(
        &self,
        channel_id: Option<&str>,
        source_event_id: Option<&str>,
        owner_actor_id: Option<&str>,
        statuses: &[TaskStatus],
    ) -> Vec<Task> {
        let inner = self.inner.read();
        let mut rows: Vec<Task> = inner
            .tasks
            .values()
            .filter(|task| channel_id.is_none_or(|id| task.channel_id == id))
            .filter(|task| source_event_id.is_none_or(|id| task.source_event_id == id))
            .filter(|task| {
                owner_actor_id.is_none_or(|id| task.owner_actor_id.as_deref() == Some(id))
            })
            .filter(|task| statuses.is_empty() || statuses.contains(&task.status))
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            a.channel_id
                .cmp(&b.channel_id)
                .then_with(|| a.number.cmp(&b.number))
        });
        rows
    }

    pub fn update_task(
        &self,
        task_id: &str,
        status: Option<TaskStatus>,
        owner_actor_id: Option<String>,
        result_summary: Option<String>,
        artifact_ids: Option<Vec<String>>,
        append_artifact_ids: Vec<String>,
    ) -> StoreResult<Task> {
        let mut task = self
            .get_task(task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {task_id}")))?;
        if let Some(owner) = owner_actor_id {
            if !self.is_channel_member(&task.channel_id, &owner) {
                return Err(StoreError::InvalidState(format!(
                    "task owner {owner} is not a member of channel {}",
                    task.channel_id
                )));
            }
            task.owner_actor_id = Some(owner);
        }
        if let Some(status) = status {
            task.status = status;
        }
        if let Some(summary) = result_summary {
            task.result_summary = summary;
        }
        if let Some(ids) = artifact_ids {
            task.artifact_ids = unique_nonempty(ids);
        }
        for id in append_artifact_ids {
            if !id.trim().is_empty() && !task.artifact_ids.iter().any(|x| x == &id) {
                task.artifact_ids.push(id);
            }
        }
        task.updated_at = Utc::now();
        self.journal.append(&Mutation::TaskUpsert(task.clone()))?;
        self.inner
            .write()
            .tasks
            .insert(task.id.clone(), task.clone());
        self.emit(StoreEvent::TaskChanged(task.clone()));
        Ok(task)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn attach_task_ref(
        &self,
        task_id: &str,
        kind: String,
        subtype: String,
        value: String,
        normalized: String,
        fields: serde_json::Value,
        confidence: TaskRefConfidence,
        status: TaskRefStatus,
        superseded_by: Option<String>,
        source_event_id: Option<String>,
        created_by_actor_id: String,
    ) -> StoreResult<TaskRef> {
        let _guard = self.structure_lock.lock();
        let task = self
            .get_task(task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {task_id}")))?;
        let normalized = if normalized.trim().is_empty() {
            normalize_task_ref(&value)
        } else {
            normalized.trim().to_string()
        };
        let now = Utc::now();
        let mut to_upsert = Vec::new();
        {
            let inner = self.inner.read();
            if let Some(existing) = inner.task_refs.values().find(|r| {
                r.task_id == task_id
                    && r.kind == kind
                    && r.subtype == subtype
                    && r.normalized == normalized
                    && r.status == status
            }) {
                return Ok(existing.clone());
            }
            if status == TaskRefStatus::Active {
                for existing in inner.task_refs.values().filter(|r| {
                    r.channel_id == task.channel_id
                        && r.kind == kind
                        && r.subtype == subtype
                        && r.normalized == normalized
                        && r.status == TaskRefStatus::Active
                        && r.task_id != task_id
                }) {
                    let Some(existing_task) = inner.tasks.get(&existing.task_id) else {
                        continue;
                    };
                    if is_terminal_task_status(existing_task.status) {
                        continue;
                    }
                    if confidence == TaskRefConfidence::Confirmed
                        && existing.confidence == TaskRefConfidence::Inferred
                    {
                        let mut superseded = existing.clone();
                        superseded.status = TaskRefStatus::Superseded;
                        superseded.superseded_by = Some(format!("tref_pending_{}", short_id()));
                        superseded.updated_at = now;
                        to_upsert.push(superseded);
                    } else {
                        return Err(StoreError::Conflict(format!(
                            "active task ref {} / {} / {} already belongs to non-terminal task {}",
                            kind, subtype, normalized, existing.task_id
                        )));
                    }
                }
            }
        }

        let id = format!("tref_{}", short_id());
        for superseded in &mut to_upsert {
            if superseded
                .superseded_by
                .as_deref()
                .is_some_and(|id| id.starts_with("tref_pending_"))
            {
                superseded.superseded_by = Some(id.clone());
            }
            self.journal
                .append(&Mutation::TaskRefUpsert(superseded.clone()))?;
        }
        let task_ref = TaskRef {
            id,
            task_id: task.id.clone(),
            channel_id: task.channel_id.clone(),
            kind,
            subtype,
            value,
            normalized,
            fields,
            confidence,
            status,
            superseded_by,
            source_event_id,
            created_by_actor_id,
            created_at: now,
            updated_at: now,
            _meta: None,
        };
        self.journal
            .append(&Mutation::TaskRefUpsert(task_ref.clone()))?;
        {
            let mut inner = self.inner.write();
            for superseded in to_upsert {
                inner.task_refs.insert(superseded.id.clone(), superseded);
            }
            inner
                .task_refs
                .insert(task_ref.id.clone(), task_ref.clone());
        }
        self.record_task_change(
            &task,
            TaskChangeType::Action,
            vec![task_ref.id.clone()],
            format!(
                "task_ref:{}:{}:{}",
                task_ref.kind, task_ref.subtype, task_ref.normalized
            ),
            format!("task ref {} attached", task_ref.kind),
            default_task_change_recipients(&task, &[]),
            true,
        )?;
        Ok(task_ref)
    }

    pub fn find_task_refs(
        &self,
        channel_id: Option<&str>,
        kind: &str,
        subtype: &str,
        normalized: &str,
        confidence: Option<TaskRefConfidence>,
        status: Option<TaskRefStatus>,
    ) -> (Vec<TaskRef>, Vec<Task>) {
        let inner = self.inner.read();
        let mut refs: Vec<TaskRef> = inner
            .task_refs
            .values()
            .filter(|r| channel_id.is_none_or(|id| r.channel_id == id))
            .filter(|r| r.kind == kind)
            .filter(|r| r.subtype == subtype)
            .filter(|r| r.normalized == normalized)
            .filter(|r| confidence.is_none_or(|c| r.confidence == c))
            .filter(|r| status.is_none_or(|s| r.status == s))
            .cloned()
            .collect();
        refs.sort_by(|a, b| {
            a.channel_id
                .cmp(&b.channel_id)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.subtype.cmp(&b.subtype))
                .then_with(|| a.normalized.cmp(&b.normalized))
        });
        let tasks = refs
            .iter()
            .filter_map(|r| inner.tasks.get(&r.task_id).cloned())
            .collect();
        (refs, tasks)
    }

    pub fn list_task_refs(&self, task_id: &str) -> Vec<TaskRef> {
        let inner = self.inner.read();
        let mut refs: Vec<TaskRef> = inner
            .task_refs
            .values()
            .filter(|r| r.task_id == task_id)
            .cloned()
            .collect();
        refs.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.subtype.cmp(&b.subtype))
                .then_with(|| a.normalized.cmp(&b.normalized))
        });
        refs
    }

    #[allow(clippy::too_many_arguments)]
    pub fn attach_task_artifact_link(
        &self,
        task_id: &str,
        artifact_id: String,
        schema: String,
        role: String,
        sequence: Option<u64>,
        status: TaskArtifactLinkStatus,
        lineage: serde_json::Value,
        binding: serde_json::Value,
        created_by_actor_id: String,
    ) -> StoreResult<TaskArtifactLink> {
        let _guard = self.structure_lock.lock();
        let task = self
            .get_task(task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {task_id}")))?;
        {
            let inner = self.inner.read();
            if !inner.artifacts.contains_key(&artifact_id) {
                return Err(StoreError::NotFound(format!("artifact {artifact_id}")));
            }
            if status == TaskArtifactLinkStatus::Active {
                let target_key = binding_string(&binding, "target_key");
                let purpose = binding_string(&binding, "purpose");
                if let Some(existing) = inner.task_artifact_links.values().find(|l| {
                    l.task_id == task_id
                        && l.status == TaskArtifactLinkStatus::Active
                        && l.schema == schema
                        && l.role == role
                        && binding_string(&l.binding, "target_key") == target_key
                        && binding_string(&l.binding, "purpose") == purpose
                }) {
                    return Err(StoreError::Conflict(format!(
                        "active artifact link {} already covers schema={} role={} target={} purpose={}",
                        existing.id, schema, role, target_key, purpose
                    )));
                }
            }
        }
        let seq = sequence.unwrap_or_else(|| {
            self.inner
                .read()
                .task_artifact_links
                .values()
                .filter(|l| l.task_id == task_id)
                .map(|l| l.sequence)
                .max()
                .unwrap_or(0)
                + 1
        });
        let now = Utc::now();
        let link = TaskArtifactLink {
            id: format!("tal_{}", short_id()),
            task_id: task.id.clone(),
            artifact_id,
            schema,
            role,
            sequence: seq,
            status,
            lineage,
            binding,
            created_by_actor_id,
            created_at: now,
            updated_at: now,
            _meta: None,
        };
        self.journal
            .append(&Mutation::TaskArtifactLinkUpsert(link.clone()))?;
        self.inner
            .write()
            .task_artifact_links
            .insert(link.id.clone(), link.clone());
        self.record_task_change(
            &task,
            TaskChangeType::ArtifactLink,
            vec![link.id.clone()],
            format!(
                "artifact_link:{}:{}:{}",
                link.schema, link.role, link.sequence
            ),
            format!("artifact link {} attached", link.role),
            default_task_change_recipients(&task, &[]),
            true,
        )?;
        Ok(link)
    }

    pub fn activate_task_artifact_link(
        &self,
        link_id: &str,
        supersede_link_ids: Vec<String>,
    ) -> StoreResult<(TaskArtifactLink, Vec<TaskArtifactLink>)> {
        let _guard = self.structure_lock.lock();
        let mut link = self
            .inner
            .read()
            .task_artifact_links
            .get(link_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("artifact link {link_id}")))?;
        let task = self
            .get_task(&link.task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {}", link.task_id)))?;
        let mut superseded = Vec::new();
        {
            let inner = self.inner.read();
            let target_key = binding_string(&link.binding, "target_key");
            let purpose = binding_string(&link.binding, "purpose");
            for existing in inner.task_artifact_links.values().filter(|l| {
                l.id != link.id
                    && l.task_id == link.task_id
                    && l.status == TaskArtifactLinkStatus::Active
                    && l.schema == link.schema
                    && l.role == link.role
                    && binding_string(&l.binding, "target_key") == target_key
                    && binding_string(&l.binding, "purpose") == purpose
            }) {
                if supersede_link_ids.iter().any(|id| id == &existing.id) {
                    let mut s = existing.clone();
                    s.status = TaskArtifactLinkStatus::Superseded;
                    s.updated_at = Utc::now();
                    superseded.push(s);
                } else {
                    return Err(StoreError::Conflict(format!(
                        "active artifact link {} must be superseded before {} can activate",
                        existing.id, link.id
                    )));
                }
            }
        }
        link.status = TaskArtifactLinkStatus::Active;
        link.updated_at = Utc::now();
        for item in &superseded {
            self.journal
                .append(&Mutation::TaskArtifactLinkUpsert(item.clone()))?;
        }
        self.journal
            .append(&Mutation::TaskArtifactLinkUpsert(link.clone()))?;
        {
            let mut inner = self.inner.write();
            for item in &superseded {
                inner
                    .task_artifact_links
                    .insert(item.id.clone(), item.clone());
            }
            inner
                .task_artifact_links
                .insert(link.id.clone(), link.clone());
        }
        self.record_task_change(
            &task,
            TaskChangeType::ArtifactLink,
            std::iter::once(link.id.clone())
                .chain(superseded.iter().map(|l| l.id.clone()))
                .collect(),
            format!("artifact_link_activate:{}", link.id),
            format!("artifact link {} activated", link.role),
            default_task_change_recipients(&task, &[]),
            true,
        )?;
        Ok((link, superseded))
    }

    pub fn list_task_artifact_links(
        &self,
        task_id: &str,
        status: Option<TaskArtifactLinkStatus>,
    ) -> Vec<TaskArtifactLink> {
        let mut links: Vec<TaskArtifactLink> = self
            .inner
            .read()
            .task_artifact_links
            .values()
            .filter(|l| l.task_id == task_id)
            .filter(|l| status.is_none_or(|s| l.status == s))
            .cloned()
            .collect();
        links.sort_by(|a, b| a.sequence.cmp(&b.sequence).then_with(|| a.id.cmp(&b.id)));
        links
    }

    pub fn get_task_artifact_link(&self, link_id: &str) -> Option<TaskArtifactLink> {
        self.inner.read().task_artifact_links.get(link_id).cloned()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_task_fact(
        &self,
        task_id: &str,
        target_key: String,
        kind: String,
        fact_type: TaskFactType,
        subject: serde_json::Value,
        signature: Option<String>,
        status: TaskFactStatus,
        replaces: Vec<String>,
        retracted_by: Option<String>,
        authority: String,
        authority_binding: serde_json::Value,
        observed_at: Option<Timestamp>,
        source_cursor: Option<String>,
        source_snapshot_id: Option<String>,
        external_updated_at: Option<Timestamp>,
        observed_fields: Vec<String>,
        unobserved_fields: Vec<String>,
        unavailable_reason: Option<String>,
        snapshot_completeness: Option<TaskSnapshotCompleteness>,
        producer_id: String,
        summary: String,
        raw_refs: Vec<String>,
        artifact_id: Option<String>,
        payload_schema: String,
        payload: serde_json::Value,
    ) -> StoreResult<(TaskFact, bool)> {
        let _guard = self.structure_lock.lock();
        let task = self
            .get_task(task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {task_id}")))?;
        let signature = signature.unwrap_or_else(|| {
            task_fact_signature(
                &target_key,
                &kind,
                &subject,
                &authority,
                &authority_binding,
                &payload_schema,
                &payload,
                &observed_fields,
                &unobserved_fields,
                snapshot_completeness,
            )
        });
        {
            let inner = self.inner.read();
            if let Some(existing) = inner.task_facts.values().find(|f| {
                f.task_id == task_id
                    && f.signature == signature
                    && f.status == status
                    && f.target_key == target_key
                    && f.kind == kind
            }) {
                return Ok((existing.clone(), false));
            }
        }
        let now = Utc::now();
        let mut lifecycle_updates = Vec::new();
        {
            let inner = self.inner.read();
            for old_id in &replaces {
                if let Some(old) = inner.task_facts.get(old_id) {
                    if old.task_id != task.id {
                        return Err(StoreError::InvalidState(format!(
                            "replacement fact {old_id} does not belong to task {}",
                            task.id
                        )));
                    }
                    let mut updated = old.clone();
                    updated.status = match status {
                        TaskFactStatus::Retracted => TaskFactStatus::Retracted,
                        TaskFactStatus::Conflict => TaskFactStatus::Conflict,
                        _ => TaskFactStatus::Superseded,
                    };
                    updated.retracted_by = if status == TaskFactStatus::Retracted {
                        Some(format!("fact_pending_{}", short_id()))
                    } else {
                        updated.retracted_by
                    };
                    updated.updated_at = now;
                    lifecycle_updates.push(updated);
                }
            }
        }
        let id = format!("fact_{}", short_id());
        for update in &mut lifecycle_updates {
            if update
                .retracted_by
                .as_deref()
                .is_some_and(|id| id.starts_with("fact_pending_"))
            {
                update.retracted_by = Some(id.clone());
            }
            self.journal
                .append(&Mutation::TaskFactUpsert(update.clone()))?;
        }
        let fact = TaskFact {
            id,
            task_id: task.id.clone(),
            target_key,
            kind,
            fact_type,
            subject,
            signature,
            status,
            replaces,
            retracted_by,
            authority,
            authority_binding,
            observed_at: observed_at.unwrap_or(now),
            source_cursor,
            source_snapshot_id,
            external_updated_at,
            observed_fields,
            unobserved_fields,
            unavailable_reason,
            snapshot_completeness,
            producer_id,
            summary,
            raw_refs,
            artifact_id,
            payload_schema,
            payload,
            created_at: now,
            updated_at: now,
            _meta: None,
        };
        self.journal
            .append(&Mutation::TaskFactUpsert(fact.clone()))?;
        {
            let mut inner = self.inner.write();
            for update in lifecycle_updates {
                inner.task_facts.insert(update.id.clone(), update);
            }
            inner.task_facts.insert(fact.id.clone(), fact.clone());
        }
        self.record_task_change(
            &task,
            TaskChangeType::Fact,
            vec![fact.id.clone()],
            format!("fact:{}:{}:{}", fact.target_key, fact.kind, fact.signature),
            if fact.summary.trim().is_empty() {
                format!("task fact {} appended", fact.kind)
            } else {
                fact.summary.clone()
            },
            default_task_change_recipients(&task, &[]),
            true,
        )?;
        Ok((fact, true))
    }

    pub fn list_task_facts(
        &self,
        task_id: &str,
        kind: Option<&str>,
        status: Option<TaskFactStatus>,
        target_key: Option<&str>,
    ) -> Vec<TaskFact> {
        let mut facts: Vec<TaskFact> = self
            .inner
            .read()
            .task_facts
            .values()
            .filter(|f| f.task_id == task_id)
            .filter(|f| kind.is_none_or(|kind| f.kind == kind))
            .filter(|f| status.is_none_or(|status| f.status == status))
            .filter(|f| target_key.is_none_or(|target| f.target_key == target))
            .cloned()
            .collect();
        facts.sort_by(|a, b| {
            a.observed_at
                .cmp(&b.observed_at)
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        facts
    }

    pub fn put_task_projection(
        &self,
        task_id: &str,
        projection_type: String,
        producer_actor_id: String,
        health: TaskProjectionHealth,
        watermark: serde_json::Value,
        payload_schema: String,
        payload: serde_json::Value,
    ) -> StoreResult<TaskProjection> {
        let _guard = self.structure_lock.lock();
        let task = self
            .get_task(task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {task_id}")))?;
        let now = Utc::now();
        let id = self
            .inner
            .read()
            .task_projections
            .values()
            .find(|p| p.task_id == task_id && p.projection_type == projection_type)
            .map(|p| p.id.clone())
            .unwrap_or_else(|| format!("tproj_{}", short_id()));
        let projection = TaskProjection {
            id,
            task_id: task.id.clone(),
            projection_type,
            producer_actor_id,
            health,
            watermark,
            payload_schema,
            payload,
            updated_at: now,
            _meta: None,
        };
        self.journal
            .append(&Mutation::TaskProjectionUpsert(projection.clone()))?;
        self.inner
            .write()
            .task_projections
            .insert(projection.id.clone(), projection.clone());
        self.record_task_change(
            &task,
            TaskChangeType::Projection,
            vec![projection.id.clone()],
            format!(
                "projection:{}:{:?}:{}",
                projection.projection_type,
                projection.health,
                json_pair_hash(&projection.watermark, &projection.payload)
            ),
            format!("task projection {} updated", projection.projection_type),
            default_task_change_recipients(&task, &[]),
            true,
        )?;
        Ok(projection)
    }

    pub fn get_task_projection(
        &self,
        task_id: &str,
        projection_type: &str,
    ) -> Option<TaskProjection> {
        self.inner
            .read()
            .task_projections
            .values()
            .find(|p| p.task_id == task_id && p.projection_type == projection_type)
            .cloned()
    }

    pub fn list_task_projections(&self, task_id: &str) -> Vec<TaskProjection> {
        let mut projections: Vec<TaskProjection> = self
            .inner
            .read()
            .task_projections
            .values()
            .filter(|p| p.task_id == task_id)
            .cloned()
            .collect();
        projections.sort_by(|a, b| {
            a.projection_type
                .cmp(&b.projection_type)
                .then_with(|| a.updated_at.cmp(&b.updated_at))
        });
        projections
    }

    pub fn create_task_assignment(
        &self,
        task_id: &str,
        from_actor_id: String,
        to_actor_id: String,
        assignment_type: TaskAssignmentType,
        instruction: String,
        contract: Option<serde_json::Value>,
        idempotency_key: Option<String>,
    ) -> StoreResult<(TaskAssignment, Task, bool)> {
        let _guard = self.structure_lock.lock();
        let mut task = self
            .get_task(task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {task_id}")))?;
        if !self.is_channel_member(&task.channel_id, &from_actor_id) {
            return Err(StoreError::InvalidState(format!(
                "assignment sender {from_actor_id} is not a member of channel {}",
                task.channel_id
            )));
        }
        if !self.is_channel_member(&task.channel_id, &to_actor_id) {
            return Err(StoreError::InvalidState(format!(
                "assignment recipient {to_actor_id} is not a member of channel {}",
                task.channel_id
            )));
        }
        if is_terminal_task_status(task.status)
            && matches!(
                assignment_type,
                TaskAssignmentType::Fix | TaskAssignmentType::Review
            )
        {
            return Err(StoreError::InvalidState(format!(
                "terminal task {} rejects {:?} assignment",
                task.id, assignment_type
            )));
        }
        let Some(contract_value) = contract.as_ref() else {
            return Err(StoreError::InvalidState(format!(
                "assignment for task {} requires machine-readable contract",
                task.id
            )));
        };
        if !contract_value.is_object() {
            return Err(StoreError::InvalidState(format!(
                "assignment for task {} requires object contract",
                task.id
            )));
        }
        let effective_idempotency = idempotency_key.clone().or_else(|| {
            contract
                .as_ref()
                .and_then(|c| json_path_string(c, &["idempotency_key"]))
        });
        if let Some(key) = effective_idempotency.as_ref() {
            let inner = self.inner.read();
            if let Some(existing) = inner.assignments.values().find(|a| {
                a.task_id == task_id
                    && a.idempotency_key.as_deref() == Some(key)
                    && matches!(
                        a.status,
                        TaskAssignmentStatus::Pending | TaskAssignmentStatus::Running
                    )
            }) {
                return Ok((existing.clone(), task, false));
            }
            if inner.assignments.values().any(|a| {
                a.task_id == task_id
                    && a.idempotency_key.as_deref() == Some(key)
                    && a.status == TaskAssignmentStatus::Completed
            }) {
                return Err(StoreError::Conflict(format!(
                    "completed assignment already exists for idempotency key {key}"
                )));
            }
        }
        self.validate_assignment_contract(&task, &to_actor_id, contract_value)?;
        let now = Utc::now();
        let assignment = TaskAssignment {
            id: format!("asgn_{}", short_id()),
            task_id: task.id.clone(),
            from_actor_id,
            to_actor_id,
            assignment_type,
            instruction,
            status: TaskAssignmentStatus::Pending,
            result_event_id: None,
            result_summary: String::new(),
            contract,
            idempotency_key: effective_idempotency,
            lease_id: None,
            result_artifact_ids: Vec::new(),
            result_fact_ids: Vec::new(),
            evidence_refs: Vec::new(),
            result_envelope: None,
            created_at: now,
            updated_at: now,
            _meta: None,
        };
        task.assignment_ids.push(assignment.id.clone());
        task.assignment_ids = unique_nonempty(task.assignment_ids);
        if !matches!(
            task.status,
            TaskStatus::Done | TaskStatus::Failed | TaskStatus::Canceled
        ) {
            task.status = match assignment_type {
                TaskAssignmentType::Review => TaskStatus::WaitingReview,
                _ => TaskStatus::InProgress,
            };
        }
        task.updated_at = now;
        self.journal
            .append(&Mutation::TaskAssignmentUpsert(assignment.clone()))?;
        self.journal.append(&Mutation::TaskUpsert(task.clone()))?;
        {
            let mut inner = self.inner.write();
            inner
                .assignments
                .insert(assignment.id.clone(), assignment.clone());
            inner.tasks.insert(task.id.clone(), task.clone());
        }
        self.emit(StoreEvent::TaskAssignmentChanged {
            assignment: assignment.clone(),
            task: task.clone(),
        });
        self.emit(StoreEvent::TaskChanged(task.clone()));
        self.record_task_change(
            &task,
            TaskChangeType::Assignment,
            vec![assignment.id.clone()],
            format!("assignment:create:{}", assignment.id),
            format!("assignment {} created", assignment.id),
            default_task_change_recipients(&task, &[assignment.from_actor_id.clone()]),
            true,
        )?;
        Ok((assignment, task, true))
    }

    pub fn update_task_assignment(
        &self,
        assignment_id: &str,
        status: Option<TaskAssignmentStatus>,
        result_event_id: Option<String>,
        result_summary: Option<String>,
        result_envelope: Option<serde_json::Value>,
        result_artifact_ids: Vec<String>,
        result_fact_ids: Vec<String>,
        evidence_refs: Vec<String>,
    ) -> StoreResult<(TaskAssignment, Task)> {
        let _guard = self.structure_lock.lock();
        let mut assignment = self
            .get_assignment(assignment_id)
            .ok_or_else(|| StoreError::NotFound(format!("assignment {assignment_id}")))?;
        if let Some(event_id) = result_event_id.as_ref() {
            if self.get_event(event_id).is_none() {
                return Err(StoreError::NotFound(format!("event {event_id}")));
            }
        }
        if let Some(next_status) = status {
            if next_status == TaskAssignmentStatus::Completed {
                if assignment.status != TaskAssignmentStatus::Running {
                    return Err(StoreError::InvalidState(format!(
                        "assignment {} must be running before it can complete",
                        assignment.id
                    )));
                }
                let completion_envelope = result_envelope
                    .as_ref()
                    .or(assignment.result_envelope.as_ref())
                    .ok_or_else(|| {
                        StoreError::InvalidState(format!(
                            "completed assignment {} requires result envelope",
                            assignment.id
                        ))
                    })?;
                validate_completion_result_envelope(&assignment, completion_envelope)?;
                let guards = self.assignment_guards(&assignment);
                if !assignment_guards_allow_completion(&guards) {
                    return Err(StoreError::InvalidState(format!(
                        "assignment {} cannot complete because guards are not clean: {}",
                        assignment.id, guards
                    )));
                }
            }
            assignment.status = next_status;
        }
        if let Some(event_id) = result_event_id {
            assignment.result_event_id = Some(event_id);
        }
        if let Some(summary) = result_summary {
            assignment.result_summary = summary;
        }
        if let Some(envelope) = result_envelope {
            assignment.result_envelope = Some(envelope);
        }
        if !result_artifact_ids.is_empty() {
            assignment.result_artifact_ids = unique_nonempty(result_artifact_ids);
        }
        if !result_fact_ids.is_empty() {
            assignment.result_fact_ids = unique_nonempty(result_fact_ids);
        }
        if !evidence_refs.is_empty() {
            assignment.evidence_refs = unique_nonempty(evidence_refs);
        }
        assignment.updated_at = Utc::now();
        let mut task = self
            .get_task(&assignment.task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {}", assignment.task_id)))?;
        task.updated_at = assignment.updated_at;
        self.journal
            .append(&Mutation::TaskAssignmentUpsert(assignment.clone()))?;
        self.journal.append(&Mutation::TaskUpsert(task.clone()))?;
        {
            let mut inner = self.inner.write();
            inner
                .assignments
                .insert(assignment.id.clone(), assignment.clone());
            inner.tasks.insert(task.id.clone(), task.clone());
        }
        self.emit(StoreEvent::TaskAssignmentChanged {
            assignment: assignment.clone(),
            task: task.clone(),
        });
        self.emit(StoreEvent::TaskChanged(task.clone()));
        self.record_task_change(
            &task,
            TaskChangeType::Assignment,
            vec![assignment.id.clone()],
            format!(
                "assignment:update:{}:{:?}",
                assignment.id, assignment.status
            ),
            format!("assignment {} updated", assignment.id),
            default_task_change_recipients(
                &task,
                &[
                    assignment.from_actor_id.clone(),
                    assignment.to_actor_id.clone(),
                ],
            ),
            is_terminal_assignment_status_for_store(assignment.status),
        )?;
        Ok((assignment, task))
    }

    pub fn assignment_context(
        &self,
        assignment_id: &str,
    ) -> StoreResult<(
        Task,
        TaskAssignment,
        Vec<TaskRef>,
        Vec<TaskArtifactLink>,
        Vec<TaskFact>,
        Option<TaskProjection>,
        serde_json::Value,
    )> {
        let assignment = self
            .get_assignment(assignment_id)
            .ok_or_else(|| StoreError::NotFound(format!("assignment {assignment_id}")))?;
        let task = self
            .get_task(&assignment.task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {}", assignment.task_id)))?;
        let refs = self.list_task_refs(&task.id);
        let artifact_links = self.list_task_artifact_links(&task.id, None);
        let facts = self.list_task_facts(&task.id, None, None, None);
        let projection = self
            .get_task_projection(&task.id, "summary")
            .or_else(|| self.list_task_projections(&task.id).into_iter().next());
        let guards = self.assignment_guards(&assignment);
        Ok((
            task,
            assignment,
            refs,
            artifact_links,
            facts,
            projection,
            guards,
        ))
    }

    pub fn assignment_preflight(
        &self,
        assignment_id: &str,
        target_key: String,
        head: String,
        effect: String,
    ) -> StoreResult<TaskPreflightResult> {
        self.expire_workspace_leases()?;
        let assignment = self
            .get_assignment(assignment_id)
            .ok_or_else(|| StoreError::NotFound(format!("assignment {assignment_id}")))?;
        let checked_at = Utc::now();
        let guards = self.assignment_guards_with_request(&assignment, &target_key, &head, &effect);
        let allowed = guards
            .get("freshness")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|v| v == "matched")
            && guards
                .get("lease")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|v| v == "valid" || v == "unclaimed")
            && guards
                .get("runtime_revision")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|v| v == "matched" || v == "unknown")
            && guards
                .get("assignment_status")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|v| v == "running");
        let reason = if allowed {
            String::new()
        } else {
            "preflight guard failed".to_string()
        };
        Ok(TaskPreflightResult {
            assignment_id: assignment.id,
            allowed,
            checked_at,
            target_key,
            head,
            effect,
            guards,
            reason,
        })
    }

    pub fn acquire_workspace_lease(
        &self,
        assignment_id: &str,
        resource_key: String,
        mode: WorkspaceLeaseMode,
        expires_at: Timestamp,
    ) -> StoreResult<(Option<WorkspaceLease>, Vec<WorkspaceLease>)> {
        let _guard = self.structure_lock.lock();
        let mut assignment = self
            .get_assignment(assignment_id)
            .ok_or_else(|| StoreError::NotFound(format!("assignment {assignment_id}")))?;
        let task = self
            .get_task(&assignment.task_id)
            .ok_or_else(|| StoreError::NotFound(format!("task {}", assignment.task_id)))?;
        let now = Utc::now();
        let active: Vec<WorkspaceLease> = self
            .inner
            .read()
            .workspace_leases
            .values()
            .filter(|lease| {
                lease.resource_key == resource_key
                    && lease.status == WorkspaceLeaseStatus::Active
                    && lease.expires_at > now
            })
            .cloned()
            .collect();
        let conflicts: Vec<WorkspaceLease> = active
            .into_iter()
            .filter(|lease| {
                lease.holder_assignment_id != assignment_id
                    && (mode == WorkspaceLeaseMode::Write
                        || lease.mode == WorkspaceLeaseMode::Write)
            })
            .collect();
        if !conflicts.is_empty() {
            self.record_task_change(
                &task,
                TaskChangeType::Lease,
                conflicts.iter().map(|l| l.id.clone()).collect(),
                format!("lease_conflict:{resource_key}"),
                format!("workspace lease conflict on {resource_key}"),
                default_task_change_recipients(&task, &[assignment.from_actor_id.clone()]),
                true,
            )?;
            return Ok((None, conflicts));
        }
        let lease = WorkspaceLease {
            id: format!("lease_{}", short_id()),
            resource_key,
            holder_assignment_id: assignment.id.clone(),
            holder_actor_id: assignment.to_actor_id.clone(),
            mode,
            status: WorkspaceLeaseStatus::Active,
            expires_at,
            created_at: now,
            updated_at: now,
            _meta: None,
        };
        assignment.lease_id = Some(lease.id.clone());
        assignment.updated_at = now;
        self.journal
            .append(&Mutation::WorkspaceLeaseUpsert(lease.clone()))?;
        self.journal
            .append(&Mutation::TaskAssignmentUpsert(assignment.clone()))?;
        {
            let mut inner = self.inner.write();
            inner
                .workspace_leases
                .insert(lease.id.clone(), lease.clone());
            inner
                .assignments
                .insert(assignment.id.clone(), assignment.clone());
        }
        self.record_task_change(
            &task,
            TaskChangeType::Lease,
            vec![lease.id.clone()],
            format!("lease_acquired:{}", lease.resource_key),
            format!("workspace lease acquired on {}", lease.resource_key),
            default_task_change_recipients(&task, &[assignment.from_actor_id.clone()]),
            true,
        )?;
        Ok((Some(lease), Vec::new()))
    }

    pub fn release_workspace_lease(&self, lease_id: &str) -> StoreResult<WorkspaceLease> {
        let _guard = self.structure_lock.lock();
        let mut lease = self
            .inner
            .read()
            .workspace_leases
            .get(lease_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("lease {lease_id}")))?;
        lease.status = WorkspaceLeaseStatus::Released;
        lease.updated_at = Utc::now();
        self.journal
            .append(&Mutation::WorkspaceLeaseUpsert(lease.clone()))?;
        self.inner
            .write()
            .workspace_leases
            .insert(lease.id.clone(), lease.clone());
        if let Some(assignment) = self.get_assignment(&lease.holder_assignment_id) {
            if let Some(task) = self.get_task(&assignment.task_id) {
                self.record_task_change(
                    &task,
                    TaskChangeType::Lease,
                    vec![lease.id.clone()],
                    format!("lease_released:{}", lease.resource_key),
                    format!("workspace lease released on {}", lease.resource_key),
                    default_task_change_recipients(&task, &[assignment.from_actor_id]),
                    true,
                )?;
            }
        }
        Ok(lease)
    }

    pub fn get_workspace_lease(&self, lease_id: &str) -> Option<WorkspaceLease> {
        self.inner.read().workspace_leases.get(lease_id).cloned()
    }

    pub fn list_workspace_leases(
        &self,
        resource_key: Option<&str>,
        assignment_id: Option<&str>,
        active_only: bool,
    ) -> Vec<WorkspaceLease> {
        let _ = self.expire_workspace_leases();
        let now = Utc::now();
        let mut leases: Vec<WorkspaceLease> = self
            .inner
            .read()
            .workspace_leases
            .values()
            .filter(|lease| resource_key.is_none_or(|key| lease.resource_key == key))
            .filter(|lease| assignment_id.is_none_or(|id| lease.holder_assignment_id == id))
            .filter(|lease| {
                !active_only
                    || (lease.status == WorkspaceLeaseStatus::Active && lease.expires_at > now)
            })
            .cloned()
            .collect();
        leases.sort_by(|a, b| {
            a.resource_key
                .cmp(&b.resource_key)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        leases
    }

    fn expire_workspace_leases(&self) -> StoreResult<()> {
        let _guard = self.structure_lock.lock();
        let now = Utc::now();
        let expired: Vec<WorkspaceLease> = self
            .inner
            .read()
            .workspace_leases
            .values()
            .filter(|lease| lease.status == WorkspaceLeaseStatus::Active && lease.expires_at <= now)
            .cloned()
            .collect();
        for mut lease in expired {
            lease.status = WorkspaceLeaseStatus::Expired;
            lease.updated_at = now;
            self.journal
                .append(&Mutation::WorkspaceLeaseUpsert(lease.clone()))?;
            self.inner
                .write()
                .workspace_leases
                .insert(lease.id.clone(), lease.clone());
            if let Some(assignment) = self.get_assignment(&lease.holder_assignment_id) {
                if let Some(task) = self.get_task(&assignment.task_id) {
                    self.record_task_change(
                        &task,
                        TaskChangeType::Lease,
                        vec![lease.id.clone()],
                        format!("lease_expired:{}", lease.id),
                        format!("workspace lease expired on {}", lease.resource_key),
                        default_task_change_recipients(&task, &[assignment.from_actor_id]),
                        true,
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn list_task_changes(
        &self,
        recipient_actor_id: &str,
        task_id: Option<&str>,
        include_handled: bool,
        after_cursor: Option<u64>,
        limit: Option<usize>,
    ) -> Vec<TaskChangeDelivery> {
        let mut rows: Vec<TaskChangeDelivery> = self
            .inner
            .read()
            .task_change_deliveries
            .values()
            .filter(|delivery| delivery.recipient_actor_id == recipient_actor_id)
            .filter(|delivery| task_id.is_none_or(|id| delivery.change.task_id == id))
            .filter(|delivery| {
                include_handled || delivery.status != TaskChangeDeliveryStatus::Handled
            })
            .filter(|delivery| after_cursor.is_none_or(|cursor| delivery.change.cursor > cursor))
            .cloned()
            .collect();
        rows.sort_by(|a, b| a.change.cursor.cmp(&b.change.cursor));
        rows.truncate(limit.unwrap_or(100).clamp(1, 500));
        rows
    }

    pub fn ack_task_change(
        &self,
        change_id: &str,
        recipient_actor_id: &str,
        disposition: TaskChangeAckDisposition,
        result_ref_ids: Vec<String>,
        reason: String,
    ) -> StoreResult<TaskChangeDelivery> {
        let key = (change_id.to_string(), recipient_actor_id.to_string());
        let mut delivery = self
            .inner
            .read()
            .task_change_deliveries
            .get(&key)
            .cloned()
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "task change {change_id} for recipient {recipient_actor_id}"
                ))
            })?;
        if delivery.status == TaskChangeDeliveryStatus::Handled {
            return Err(StoreError::InvalidState(format!(
                "task change {change_id} for recipient {recipient_actor_id} is already handled"
            )));
        }
        if result_ref_ids.iter().all(|id| id.trim().is_empty()) && reason.trim().is_empty() {
            return Err(StoreError::InvalidState(format!(
                "task change {change_id} ack requires result refs or reason"
            )));
        }
        delivery.status = TaskChangeDeliveryStatus::Handled;
        delivery.disposition = Some(disposition);
        delivery.result_ref_ids = unique_nonempty(result_ref_ids);
        delivery.reason = reason;
        delivery.acked_at = Some(Utc::now());
        self.journal
            .append(&Mutation::TaskChangeDeliveryUpsert(delivery.clone()))?;
        self.inner
            .write()
            .task_change_deliveries
            .insert(key, delivery.clone());
        Ok(delivery)
    }

    fn validate_assignment_contract(
        &self,
        task: &Task,
        to_actor_id: &str,
        contract: &serde_json::Value,
    ) -> StoreResult<()> {
        let required_artifacts =
            json_path_array_strings(contract, &["context", "required_artifacts"]);
        let required_facts = json_path_array_strings(contract, &["context", "required_facts"]);
        let required_validation_facts =
            json_path_array_strings(contract, &["context", "required_validation_facts"]);
        let required_capabilities = json_path_array_strings(contract, &["required_capabilities"]);
        let inner = self.inner.read();
        for artifact_id in required_artifacts {
            let exists = task.artifact_ids.iter().any(|id| id == &artifact_id)
                || inner.task_artifact_links.values().any(|link| {
                    link.task_id == task.id
                        && link.artifact_id == artifact_id
                        && link.status == TaskArtifactLinkStatus::Active
                });
            if !exists {
                return Err(StoreError::InvalidState(format!(
                    "required artifact {artifact_id} is not active for task {}",
                    task.id
                )));
            }
        }
        for fact_id in required_facts {
            let Some(fact) = inner.task_facts.get(&fact_id) else {
                return Err(StoreError::InvalidState(format!(
                    "required fact {fact_id} is missing"
                )));
            };
            if fact.task_id != task.id || fact.status != TaskFactStatus::Active {
                return Err(StoreError::InvalidState(format!(
                    "required fact {fact_id} is not active for task {}",
                    task.id
                )));
            }
        }
        for fact_id in required_validation_facts {
            let Some(fact) = inner.task_facts.get(&fact_id) else {
                return Err(StoreError::InvalidState(format!(
                    "required validation fact {fact_id} is missing"
                )));
            };
            if fact.task_id != task.id || fact.status != TaskFactStatus::Active {
                return Err(StoreError::InvalidState(format!(
                    "required validation fact {fact_id} is not active for task {}",
                    task.id
                )));
            }
        }
        if !required_capabilities.is_empty() {
            let actor = inner
                .actors
                .get(to_actor_id)
                .ok_or_else(|| StoreError::NotFound(format!("actor {to_actor_id}")))?;
            let available = actor_capabilities(actor);
            for capability in required_capabilities {
                if !available.iter().any(|c| c == &capability) {
                    return Err(StoreError::InvalidState(format!(
                        "actor {to_actor_id} lacks capability {capability}"
                    )));
                }
            }
        }
        if let Some(expected) =
            json_path_string(contract, &["versions", "target_actor_spec_revision"])
        {
            let actor = inner
                .actors
                .get(to_actor_id)
                .ok_or_else(|| StoreError::NotFound(format!("actor {to_actor_id}")))?;
            let current = actor_revision(actor).ok_or_else(|| {
                StoreError::InvalidState(format!(
                    "actor {to_actor_id} does not expose runtime revision"
                ))
            })?;
            if current != expected {
                return Err(StoreError::InvalidState(format!(
                    "actor {to_actor_id} revision mismatch: expected {expected}, current {current}"
                )));
            }
        }
        Ok(())
    }

    fn assignment_guards(&self, assignment: &TaskAssignment) -> serde_json::Value {
        self.assignment_guards_with_request(assignment, "", "", "")
    }

    fn assignment_guards_with_request(
        &self,
        assignment: &TaskAssignment,
        target_key: &str,
        head: &str,
        effect: &str,
    ) -> serde_json::Value {
        let contract = assignment.contract.as_ref();
        let freshness = match contract {
            Some(contract) => {
                let expected_target =
                    json_path_string(contract, &["target", "target_key"]).unwrap_or_default();
                let expected_head =
                    json_path_string(contract, &["target", "head"]).unwrap_or_default();
                let effect_allowed = effect.is_empty()
                    || json_path_array_strings(contract, &["effects", "authorized"]).is_empty()
                    || json_path_array_strings(contract, &["effects", "authorized"])
                        .iter()
                        .any(|item| item == effect);
                if (!target_key.is_empty()
                    && !expected_target.is_empty()
                    && expected_target != target_key)
                    || (!head.is_empty() && !expected_head.is_empty() && expected_head != head)
                    || !effect_allowed
                {
                    "stale"
                } else {
                    "matched"
                }
            }
            None => "matched",
        };
        let lease = match assignment.lease_id.as_deref() {
            Some(lease_id) => {
                let now = Utc::now();
                match self.inner.read().workspace_leases.get(lease_id).cloned() {
                    Some(lease)
                        if lease.status == WorkspaceLeaseStatus::Active
                            && lease.expires_at > now =>
                    {
                        "valid"
                    }
                    Some(_) => "expired",
                    None => "missing",
                }
            }
            None => {
                let write_mode = contract
                    .and_then(|c| json_path_string(c, &["workspace", "write_mode"]))
                    .unwrap_or_default();
                if write_mode == "write" {
                    "missing"
                } else {
                    "unclaimed"
                }
            }
        };
        let runtime_revision = contract
            .and_then(|c| json_path_string(c, &["versions", "target_actor_spec_revision"]))
            .map(|expected| {
                let current = self
                    .inner
                    .read()
                    .actors
                    .get(&assignment.to_actor_id)
                    .and_then(actor_revision);
                match current {
                    Some(current) if current == expected => "matched",
                    Some(_) => "mismatch",
                    None => "unknown",
                }
            })
            .unwrap_or("unknown");
        let assignment_status = match assignment.status {
            TaskAssignmentStatus::Pending => "pending",
            TaskAssignmentStatus::Running => "running",
            TaskAssignmentStatus::Canceled => "canceled",
            TaskAssignmentStatus::Completed | TaskAssignmentStatus::Failed => "terminal",
        };
        serde_json::json!({
            "missing": guard_missing_inputs(self, assignment),
            "stale": [],
            "freshness": freshness,
            "lease": lease,
            "runtime_revision": runtime_revision,
            "assignment_status": assignment_status
        })
    }

    fn record_task_change(
        &self,
        task: &Task,
        change_type: TaskChangeType,
        source_ids: Vec<String>,
        signature: String,
        summary: String,
        recipients: Vec<String>,
        requires_ack: bool,
    ) -> StoreResult<TaskChange> {
        let recipients = unique_nonempty(recipients);
        let mut inner = self.inner.write();
        if let Some(existing) = inner
            .task_changes
            .values()
            .find(|change| change.task_id == task.id && change.signature == signature)
        {
            return Ok(existing.clone());
        }
        inner.task_change_seq += 1;
        let change = TaskChange {
            id: format!("tchg_{}", short_id()),
            cursor: inner.task_change_seq,
            task_id: task.id.clone(),
            change_type,
            source_ids: unique_nonempty(source_ids),
            signature,
            summary,
            occurred_at: Utc::now(),
            recipients,
            requires_ack,
        };
        let deliveries: Vec<TaskChangeDelivery> = change
            .recipients
            .iter()
            .map(|recipient| TaskChangeDelivery {
                change: change.clone(),
                recipient_actor_id: recipient.clone(),
                status: TaskChangeDeliveryStatus::Pending,
                disposition: None,
                result_ref_ids: Vec::new(),
                reason: String::new(),
                acked_at: None,
            })
            .collect();
        self.journal
            .append(&Mutation::TaskChangeUpsert(change.clone()))?;
        for delivery in &deliveries {
            self.journal
                .append(&Mutation::TaskChangeDeliveryUpsert(delivery.clone()))?;
        }
        inner.task_changes.insert(change.id.clone(), change.clone());
        for delivery in deliveries {
            inner.task_change_deliveries.insert(
                (
                    delivery.change.id.clone(),
                    delivery.recipient_actor_id.clone(),
                ),
                delivery,
            );
        }
        drop(inner);
        self.emit(StoreEvent::TaskChanged(task.clone()));
        Ok(change)
    }

    fn record_action_response_task_change(
        &self,
        response: &Event,
    ) -> StoreResult<Option<TaskChange>> {
        let request = {
            let inner = self.inner.read();
            response
                .relations
                .iter()
                .find(|r| {
                    matches!(r.kind, RelationKind::RespondsTo) && r.target.kind == RefKind::Event
                })
                .and_then(|r| inner.events.get(&r.target.id))
                .filter(|event| event.kind == "action.request")
                .cloned()
        };
        let Some(request) = request else {
            return Ok(None);
        };
        let task_id = json_field_string(&request.payload, "taskId")
            .or_else(|| json_field_string(&request.payload, "task_id"));
        let Some(task_id) = task_id else {
            return Ok(None);
        };
        let Some(task) = self.get_task(&task_id) else {
            return Ok(None);
        };
        let conversion_owner = json_field_string(&request.payload, "conversionOwnerActorId")
            .or_else(|| json_field_string(&request.payload, "conversion_owner_actor_id"));
        let recipients = default_task_change_recipients(
            &task,
            &conversion_owner.into_iter().collect::<Vec<_>>(),
        );
        let change = self.record_task_change(
            &task,
            TaskChangeType::Action,
            vec![request.id.clone(), response.id.clone()],
            format!("action_response:{}:{}", request.id, response.id),
            "action response requires conversion".into(),
            recipients,
            true,
        )?;
        Ok(Some(change))
    }

    // -------- Turns --------

    pub fn open_turn(
        &self,
        actor_id: String,
        scope: ScopeRef,
        trigger_event_id: Option<String>,
    ) -> StoreResult<Turn> {
        let turn = Turn {
            id: format!("turn_{}", short_id()),
            actor_id,
            scope,
            trigger_event_id,
            status: TurnStatus::Open,
            opened_at: Utc::now(),
            closed_at: None,
            _meta: None,
        };
        self.journal.append(&Mutation::TurnOpen(turn.clone()))?;
        self.inner
            .write()
            .turns
            .insert(turn.id.clone(), turn.clone());
        self.emit(StoreEvent::TurnOpened(turn.clone()));
        Ok(turn)
    }

    pub fn close_turn(&self, turn_id: &str, status: TurnStatus) -> StoreResult<Turn> {
        let now = Utc::now();
        self.journal.append(&Mutation::TurnClose {
            turn_id: turn_id.into(),
            status,
            closed_at: now,
        })?;
        let mut inner = self.inner.write();
        let turn = inner
            .turns
            .get_mut(turn_id)
            .ok_or_else(|| StoreError::NotFound(format!("turn {turn_id}")))?;
        turn.status = status;
        turn.closed_at = Some(now);
        let cloned = turn.clone();
        drop(inner);
        self.emit(StoreEvent::TurnClosed(cloned.clone()));
        Ok(cloned)
    }

    pub fn get_turn(&self, id: &str) -> Option<Turn> {
        self.inner.read().turns.get(id).cloned()
    }

    // -------- Turn-private trace --------

    /// Append a turn-private trace frame. Caller passes `kind` and `payload`;
    /// this method assigns the frame's monotonic per-turn `seq` and a
    /// `occurred_at` timestamp, persists it to the journal, and emits a
    /// `StoreEvent::TraceAppended` for the websocket layer to route to the
    /// turn owner only.
    ///
    /// Returns the persisted frame.
    pub fn append_trace_frame(
        &self,
        turn_id: &str,
        kind: proto::types::trace::TraceKind,
        payload: serde_json::Value,
    ) -> StoreResult<TraceFrame> {
        // Validate turn exists. We do not require it to be Open: callers may
        // emit a final trace frame as part of the same handler that closes
        // the turn (order is best-effort; the frame is owner-private anyway).
        if !self.inner.read().turns.contains_key(turn_id) {
            return Err(StoreError::NotFound(format!("turn {turn_id}")));
        }

        let now = Utc::now();
        let seq = {
            let mut inner = self.inner.write();
            let entry = inner.trace_seq.entry(turn_id.to_string()).or_insert(0);
            *entry += 1;
            *entry
        };

        let frame = TraceFrame {
            turn_id: turn_id.to_string(),
            seq,
            kind,
            occurred_at: now,
            payload,
            _meta: None,
        };
        self.journal.append(&Mutation::TraceAppend(frame.clone()))?;
        self.inner
            .write()
            .trace_by_turn
            .entry(turn_id.to_string())
            .or_default()
            .push(frame.clone());
        self.emit(StoreEvent::TraceAppended(frame.clone()));
        Ok(frame)
    }

    /// Read trace frames for a turn. `before_seq` selects frames with
    /// `seq < before_seq` (older); when `None`, the latest `limit` frames
    /// are returned. Returns frames in ascending `seq` order along with
    /// `has_more`.
    pub fn read_turn_trace(
        &self,
        turn_id: &str,
        limit: u32,
        before_seq: Option<u64>,
    ) -> StoreResult<(Vec<TraceFrame>, bool)> {
        if !self.inner.read().turns.contains_key(turn_id) {
            return Err(StoreError::NotFound(format!("turn {turn_id}")));
        }
        let inner = self.inner.read();
        let frames = match inner.trace_by_turn.get(turn_id) {
            Some(v) => v.clone(),
            None => return Ok((vec![], false)),
        };
        let end = match before_seq {
            Some(before) => frames
                .iter()
                .position(|f| f.seq >= before)
                .unwrap_or(frames.len()),
            None => frames.len(),
        };
        let limit = limit.max(1) as usize;
        let start = end.saturating_sub(limit);
        let has_more = start > 0;
        Ok((frames[start..end].to_vec(), has_more))
    }

    // -------- Events --------

    /// Append an event. Implicit-turn behavior: if `turn_id` is None, an implicit Turn
    /// is opened+closed around this single event.
    #[allow(clippy::too_many_arguments)]
    pub fn append_event(
        &self,
        kind: String,
        actor_id: String,
        scope: ScopeRef,
        turn_id: Option<String>,
        payload: serde_json::Value,
        relations: Vec<Relation>,
        meta: Option<Meta>,
    ) -> StoreResult<Event> {
        // Validate scope exists.
        match scope.kind {
            ScopeKind::Channel => {
                if self.get_channel(&scope.id).is_none() {
                    return Err(StoreError::NotFound(format!("channel {}", scope.id)));
                }
            }
            ScopeKind::Thread => {
                if self.get_thread(&scope.id).is_none() {
                    return Err(StoreError::NotFound(format!("thread {}", scope.id)));
                }
            }
        }

        // ACL gate: a non-member can't append into a private channel/thread.
        // Public channels short-circuit to allow.
        self.check_scope_access(&scope, &actor_id)?;

        let (assigned_turn_id, implicit_turn) = if let Some(tid) = turn_id {
            let t = self
                .get_turn(&tid)
                .ok_or_else(|| StoreError::NotFound(format!("turn {tid}")))?;
            if t.status != TurnStatus::Open {
                return Err(StoreError::InvalidState(format!("turn {tid} is not open")));
            }
            (tid, false)
        } else {
            let t = self.open_turn(actor_id.clone(), scope.clone(), None)?;
            (t.id, true)
        };

        let now = Utc::now();
        let (event_id, seq) = {
            let mut inner = self.inner.write();
            let seq_entry = inner.turn_seq.entry(assigned_turn_id.clone()).or_insert(0);
            *seq_entry += 1;
            let seq = *seq_entry;
            (format!("evt_{}", short_id()), seq)
        };

        let event = Event {
            id: event_id,
            kind,
            actor_id: actor_id.clone(),
            scope: scope.clone(),
            turn_id: Some(assigned_turn_id.clone()),
            seq,
            occurred_at: now,
            payload,
            relations,
            _meta: meta,
        };
        self.journal.append(&Mutation::EventAppend(event.clone()))?;
        {
            let mut inner = self.inner.write();
            inner.events.insert(event.id.clone(), event.clone());
            inner
                .events_by_scope
                .entry(scope.clone())
                .or_default()
                .push(event.id.clone());
        }

        // Memberships: speaker is now a member of scope.
        let _ = self.touch_membership(actor_id.clone(), scope.clone(), Some(event.id.clone()));

        // Memberships: any actor explicitly targeted joins too.
        for r in &event.relations {
            if matches!(r.kind, RelationKind::HandsOffTo) && r.target.kind == RefKind::Actor {
                let _ = self.touch_membership(r.target.id.clone(), scope.clone(), None);
            }
        }

        // Deliveries: explicit directed receivers only.
        for r in &event.relations {
            if matches!(r.kind, RelationKind::HandsOffTo) && r.target.kind == RefKind::Actor {
                let delivery = Delivery {
                    event_id: event.id.clone(),
                    actor_id: r.target.id.clone(),
                    state: DeliveryState::Pending,
                    updated_at: now,
                    _meta: None,
                };
                self.journal
                    .append(&Mutation::DeliveryUpsert(delivery.clone()))?;
                self.inner.write().deliveries.insert(
                    (delivery.event_id.clone(), delivery.actor_id.clone()),
                    delivery.clone(),
                );
                self.emit(StoreEvent::DeliveryUpdated(delivery));
            }
        }

        // Reverse-delivery for RespondsTo: when an event responds to a prior
        // event, the original event's actor is an implicit recipient. This is
        // what lets a service plugin that emitted a hand-off receive the
        // agent's reply via actor-inbox without subscribing to every scope it
        // touches. Self-responses (replying to your own event) are skipped to
        // avoid pending rows the speaker would have to acknowledge themselves.
        let has_explicit_actor_handoff = event
            .relations
            .iter()
            .any(|r| matches!(r.kind, RelationKind::HandsOffTo) && r.target.kind == RefKind::Actor);
        let mut reverse_targets: Vec<String> = Vec::new();
        {
            let inner = self.inner.read();
            for r in &event.relations {
                if !matches!(r.kind, RelationKind::RespondsTo) || r.target.kind != RefKind::Event {
                    continue;
                }
                if has_explicit_actor_handoff && event.kind != "action.response" {
                    continue;
                }
                let Some(orig) = inner.events.get(&r.target.id) else {
                    continue;
                };
                // Permission responses must be reverse-delivered even when
                // the responder uses the same actor id as the action.request
                // author. That can happen when a GUI workspace is configured
                // with the agent actor id; the long-lived agent connection
                // still owns the actor inbox and needs the response.
                let force_self = event.kind == "action.response" && orig.kind == "action.request";
                if orig.actor_id == actor_id && !force_self {
                    continue;
                }
                if reverse_targets.contains(&orig.actor_id) {
                    continue;
                }
                reverse_targets.push(orig.actor_id.clone());
            }
        }
        for target in reverse_targets {
            let delivery = Delivery {
                event_id: event.id.clone(),
                actor_id: target,
                state: DeliveryState::Pending,
                updated_at: now,
                _meta: None,
            };
            self.journal
                .append(&Mutation::DeliveryUpsert(delivery.clone()))?;
            self.inner.write().deliveries.insert(
                (delivery.event_id.clone(), delivery.actor_id.clone()),
                delivery.clone(),
            );
            self.emit(StoreEvent::DeliveryUpdated(delivery));
        }

        if event.kind == "action.response" {
            let _ = self.record_action_response_task_change(&event);
        }

        self.emit(StoreEvent::EventCreated(event.clone()));

        if implicit_turn {
            let _ = self.close_turn(&assigned_turn_id, TurnStatus::Closed);
        }

        Ok(event)
    }

    pub fn get_event(&self, id: &str) -> Option<Event> {
        self.inner.read().events.get(id).cloned()
    }

    pub fn find_assignment_handoff_event(&self, assignment_id: &str) -> Option<Event> {
        self.inner
            .read()
            .events
            .values()
            .find(|event| {
                event
                    .payload
                    .get("_meta")
                    .and_then(|meta| meta.get("assignmentId"))
                    .and_then(serde_json::Value::as_str)
                    == Some(assignment_id)
            })
            .cloned()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ensure_assignment_handoff_event(
        &self,
        assignment_id: &str,
        kind: String,
        actor_id: String,
        scope: ScopeRef,
        payload: serde_json::Value,
        relations: Vec<Relation>,
        meta: Option<Meta>,
    ) -> StoreResult<Event> {
        let _guard = self.structure_lock.lock();
        if let Some(event) = self.find_assignment_handoff_event(assignment_id) {
            return Ok(event);
        }
        self.append_event(kind, actor_id, scope, None, payload, relations, meta)
    }

    pub fn read_scope(
        &self,
        scope: &ScopeRef,
        limit: u32,
        before_event_id: Option<&str>,
    ) -> (Vec<Event>, bool) {
        let inner = self.inner.read();
        let ids = match inner.events_by_scope.get(scope) {
            Some(v) => v.clone(),
            None => return (vec![], false),
        };
        let end = match before_event_id {
            Some(before) => ids.iter().position(|id| id == before).unwrap_or(ids.len()),
            None => ids.len(),
        };
        let limit = limit.max(1) as usize;
        let start = end.saturating_sub(limit);
        let has_more = start > 0;
        let slice = ids[start..end]
            .iter()
            .filter_map(|id| inner.events.get(id).cloned())
            .collect();
        (slice, has_more)
    }

    pub fn search_messages(
        &self,
        actor_id: &str,
        query: &str,
        scope: Option<&ScopeRef>,
        limit: u32,
    ) -> Vec<Event> {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let limit = limit.max(1) as usize;
        let inner = self.inner.read();
        let mut events: Vec<Event> = inner
            .events
            .values()
            .filter(|event| scope.is_none_or(|s| &event.scope == s))
            .filter(|event| {
                scope.is_some() || can_access_scope_inner(&inner, &event.scope, actor_id)
            })
            .filter(|event| event_search_text(event).contains(&needle))
            .cloned()
            .collect();
        events.sort_by(|a, b| {
            b.occurred_at
                .cmp(&a.occurred_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        events.truncate(limit);
        events
    }

    /// List deliveries for `actor_id`, sorted ascending by `(updated_at,
    /// event_id)` so the oldest pending row is first — that's the order a
    /// host wants to drain its inbox after restart. `state_filter = None`
    /// returns all states; pass `Some(DeliveryState::Pending)` for the
    /// common "what do I still owe processing" query.
    ///
    /// `after` is the exclusive lower bound: only rows strictly greater
    /// than the supplied `(updated_at, event_id)` pair are returned. The
    /// handler converts the opaque cursor in `delivery/list` params into
    /// this pair (see §9.2).
    ///
    /// Caller must apply ACL — this method does not check who is asking.
    pub fn list_deliveries(
        &self,
        actor_id: &str,
        state_filter: Option<DeliveryState>,
        limit: usize,
        after: Option<(Timestamp, String)>,
    ) -> Vec<Delivery> {
        let inner = self.inner.read();
        let mut rows: Vec<Delivery> = inner
            .deliveries
            .values()
            .filter(|d| d.actor_id == actor_id)
            .filter(|d| state_filter.is_none_or(|s| d.state == s))
            .filter(|d| match &after {
                None => true,
                Some((ts, eid)) => {
                    if d.updated_at != *ts {
                        d.updated_at > *ts
                    } else {
                        d.event_id > *eid
                    }
                }
            })
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            a.updated_at
                .cmp(&b.updated_at)
                .then_with(|| a.event_id.cmp(&b.event_id))
        });
        rows.truncate(limit);
        rows
    }

    // -------- Machine commands --------

    pub fn upsert_machine_command(&self, command: MachineCommand) -> StoreResult<MachineCommand> {
        self.journal
            .append(&Mutation::MachineCommandUpsert(command.clone()))?;
        let mut inner = self.inner.write();
        apply(&mut inner, Mutation::MachineCommandUpsert(command.clone()));
        drop(inner);
        self.emit(StoreEvent::MachineCommandUpdated(command.clone()));
        Ok(command)
    }

    pub fn get_machine_command(&self, command_id: &str) -> Option<MachineCommand> {
        self.inner.read().machine_commands.get(command_id).cloned()
    }

    pub fn list_machine_commands(
        &self,
        machine_id: Option<&str>,
        machine_actor_id: Option<&str>,
        statuses: &[MachineCommandStatus],
        requested_by: Option<&str>,
        limit: usize,
    ) -> Vec<MachineCommand> {
        let mut rows: Vec<MachineCommand> = self
            .inner
            .read()
            .machine_commands
            .values()
            .filter(|command| machine_id.is_none_or(|id| command.machine_id == id))
            .filter(|command| machine_actor_id.is_none_or(|id| command.machine_actor_id == id))
            .filter(|command| requested_by.is_none_or(|id| command.requested_by == id))
            .filter(|command| statuses.is_empty() || statuses.contains(&command.status))
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.command_id.cmp(&b.command_id))
        });
        rows.truncate(limit);
        rows
    }

    // -------- Membership / Delivery / Receipt --------

    pub fn touch_membership(
        &self,
        actor_id: String,
        scope: ScopeRef,
        last_read_event_id: Option<String>,
    ) -> StoreResult<Membership> {
        let now = Utc::now();
        let key = (actor_id.clone(), scope.clone());
        let mut inner = self.inner.write();
        let m = inner
            .memberships
            .entry(key)
            .and_modify(|m| {
                m.updated_at = now;
                if last_read_event_id.is_some() {
                    m.last_read_event_id = last_read_event_id.clone();
                }
            })
            .or_insert_with(|| Membership {
                actor_id,
                scope,
                joined_at: now,
                updated_at: now,
                last_read_event_id,
                _meta: None,
            })
            .clone();
        drop(inner);
        let _ = self.journal.append(&Mutation::MembershipUpsert(m.clone()));
        Ok(m)
    }

    pub fn record_receipt(
        &self,
        event_id: String,
        actor_id: String,
        kind: ReceiptKind,
    ) -> StoreResult<Receipt> {
        if self.get_event(&event_id).is_none() {
            return Err(StoreError::NotFound(format!("event {event_id}")));
        }
        let receipt = Receipt {
            event_id: event_id.clone(),
            actor_id: actor_id.clone(),
            kind,
            recorded_at: Utc::now(),
            _meta: None,
        };
        self.journal
            .append(&Mutation::ReceiptRecord(receipt.clone()))?;
        self.inner
            .write()
            .receipts
            .insert((event_id, actor_id, kind), receipt.clone());
        // Mark related delivery as delivered if pending.
        {
            let mut inner = self.inner.write();
            let key = (receipt.event_id.clone(), receipt.actor_id.clone());
            if let Some(d) = inner.deliveries.get_mut(&key) {
                d.state = DeliveryState::Delivered;
                d.updated_at = receipt.recorded_at;
                let cloned = d.clone();
                drop(inner);
                let _ = self
                    .journal
                    .append(&Mutation::DeliveryUpsert(cloned.clone()));
                self.emit(StoreEvent::DeliveryUpdated(cloned));
            }
        }
        self.emit(StoreEvent::ReceiptRecorded(receipt.clone()));
        Ok(receipt)
    }

    // -------- Reminders --------

    pub fn schedule_reminder(
        &self,
        actor_id: String,
        title: String,
        scope: Option<ScopeRef>,
        msg_id: Option<String>,
        fire_at: Timestamp,
        repeat: Option<String>,
    ) -> StoreResult<Reminder> {
        if title.trim().is_empty() {
            return Err(StoreError::InvalidState("reminder title is empty".into()));
        }
        if let Some(scope) = scope.as_ref() {
            self.check_scope_access(scope, &actor_id)?;
        }
        let now = Utc::now();
        let reminder = Reminder {
            id: format!("rem_{}", short_id()),
            actor_id,
            title,
            scope,
            msg_id,
            fire_at,
            repeat,
            status: ReminderStatus::Scheduled,
            created_at: now,
            updated_at: now,
            last_fired_at: None,
            _meta: None,
        };
        self.put_reminder(reminder)
    }

    pub fn list_reminders(
        &self,
        actor_id: &str,
        statuses: &[ReminderStatus],
        all: bool,
    ) -> Vec<Reminder> {
        let mut reminders: Vec<Reminder> = self
            .inner
            .read()
            .reminders
            .values()
            .filter(|r| r.actor_id == actor_id)
            .filter(|r| {
                if all {
                    true
                } else if statuses.is_empty() {
                    matches!(r.status, ReminderStatus::Scheduled)
                } else {
                    statuses.contains(&r.status)
                }
            })
            .cloned()
            .collect();
        reminders.sort_by(|a, b| a.fire_at.cmp(&b.fire_at).then_with(|| a.id.cmp(&b.id)));
        reminders
    }

    pub fn cancel_reminder(&self, actor_id: &str, id_or_prefix: &str) -> StoreResult<Reminder> {
        let mut reminder = self.resolve_reminder_for_actor(actor_id, id_or_prefix)?;
        reminder.status = ReminderStatus::Cancelled;
        reminder.updated_at = Utc::now();
        self.put_reminder(reminder)
    }

    pub fn snooze_reminder(
        &self,
        actor_id: &str,
        id_or_prefix: &str,
        by_seconds: i64,
    ) -> StoreResult<Reminder> {
        if by_seconds <= 0 {
            return Err(StoreError::InvalidState(
                "snooze duration must be positive".into(),
            ));
        }
        let mut reminder = self.resolve_reminder_for_actor(actor_id, id_or_prefix)?;
        reminder.fire_at = Utc::now() + ChronoDuration::seconds(by_seconds);
        reminder.status = ReminderStatus::Scheduled;
        reminder.updated_at = Utc::now();
        self.put_reminder(reminder)
    }

    pub fn update_reminder(
        &self,
        actor_id: &str,
        id_or_prefix: &str,
        title: Option<String>,
        fire_at: Option<Timestamp>,
        repeat: Option<String>,
    ) -> StoreResult<Reminder> {
        let mut reminder = self.resolve_reminder_for_actor(actor_id, id_or_prefix)?;
        if let Some(title) = title {
            if title.trim().is_empty() {
                return Err(StoreError::InvalidState("reminder title is empty".into()));
            }
            reminder.title = title;
        }
        if let Some(fire_at) = fire_at {
            reminder.fire_at = fire_at;
            reminder.status = ReminderStatus::Scheduled;
        }
        if repeat.is_some() {
            reminder.repeat = repeat;
        }
        reminder.updated_at = Utc::now();
        self.put_reminder(reminder)
    }

    pub fn fire_due_reminders(&self) -> Vec<Reminder> {
        let now = Utc::now();
        let due: Vec<Reminder> = {
            let inner = self.inner.read();
            inner
                .reminders
                .values()
                .filter(|r| matches!(r.status, ReminderStatus::Scheduled) && r.fire_at <= now)
                .cloned()
                .collect()
        };
        let mut fired = Vec::new();
        for mut reminder in due {
            if let Some(scope) = reminder.scope.clone() {
                let mut relations = vec![Relation {
                    kind: RelationKind::HandsOffTo,
                    target: Ref {
                        kind: RefKind::Actor,
                        id: reminder.actor_id.clone(),
                        _meta: None,
                    },
                    _meta: None,
                }];
                if let Some(msg_id) = reminder.msg_id.as_ref() {
                    relations.push(Relation {
                        kind: RelationKind::RespondsTo,
                        target: Ref {
                            kind: RefKind::Event,
                            id: msg_id.clone(),
                            _meta: None,
                        },
                        _meta: None,
                    });
                }
                let payload = serde_json::json!({
                    "reminderId": reminder.id,
                    "title": reminder.title,
                    "msgId": reminder.msg_id,
                    "fireAt": reminder.fire_at,
                });
                if let Err(e) = self.append_event(
                    "reminder.fire".into(),
                    reminder.actor_id.clone(),
                    scope,
                    None,
                    payload,
                    relations,
                    None,
                ) {
                    tracing::warn!(
                        reminder = %reminder.id,
                        error = %e,
                        "failed to append reminder.fire event"
                    );
                }
            }

            reminder.last_fired_at = Some(now);
            reminder.updated_at = now;
            if let Some(next) = reminder
                .repeat
                .as_deref()
                .and_then(|rule| next_repeat_after(now, rule))
            {
                reminder.fire_at = next;
                reminder.status = ReminderStatus::Scheduled;
            } else {
                reminder.status = ReminderStatus::Fired;
            }
            if let Ok(saved) = self.put_reminder(reminder) {
                fired.push(saved);
            }
        }
        fired
    }

    fn resolve_reminder_for_actor(
        &self,
        actor_id: &str,
        id_or_prefix: &str,
    ) -> StoreResult<Reminder> {
        let inner = self.inner.read();
        let matches: Vec<Reminder> = inner
            .reminders
            .values()
            .filter(|r| r.actor_id == actor_id && r.id.starts_with(id_or_prefix))
            .cloned()
            .collect();
        match matches.len() {
            0 => Err(StoreError::NotFound(format!("reminder {id_or_prefix}"))),
            1 => Ok(matches.into_iter().next().unwrap()),
            _ => Err(StoreError::Conflict(format!(
                "reminder prefix {id_or_prefix} is ambiguous"
            ))),
        }
    }

    fn put_reminder(&self, reminder: Reminder) -> StoreResult<Reminder> {
        self.journal
            .append(&Mutation::ReminderUpsert(reminder.clone()))?;
        self.inner
            .write()
            .reminders
            .insert(reminder.id.clone(), reminder.clone());
        Ok(reminder)
    }

    // -------- Artifacts --------

    pub fn put_artifact(&self, artifact: Artifact) -> StoreResult<Artifact> {
        self.journal
            .append(&Mutation::ArtifactCreate(artifact.clone()))?;
        self.inner
            .write()
            .artifacts
            .insert(artifact.id.clone(), artifact.clone());
        self.emit(StoreEvent::ArtifactPublished(artifact.clone()));
        Ok(artifact)
    }

    pub fn get_artifact(&self, id: &str) -> Option<Artifact> {
        self.inner.read().artifacts.get(id).cloned()
    }

    pub fn get_artifact_by_uri(&self, uri: &str) -> Option<Artifact> {
        self.inner
            .read()
            .artifacts
            .values()
            .find(|a| a.uri == uri)
            .cloned()
    }
}

fn apply(inner: &mut Inner, m: Mutation) {
    match m {
        Mutation::ActorUpsert(a) => {
            inner.actors.insert(a.id.clone(), a);
        }
        Mutation::ActorDelete { actor_id } => {
            inner.actors.remove(&actor_id);
            for channel in inner.channels.values_mut() {
                channel.members.retain(|member| member != &actor_id);
            }
            for task in inner.tasks.values_mut() {
                if task.owner_actor_id.as_deref() == Some(&actor_id) {
                    task.owner_actor_id = None;
                }
            }
            inner
                .memberships
                .retain(|(member_actor_id, _), _| member_actor_id != &actor_id);
            inner
                .deliveries
                .retain(|(_, target_actor_id), _| target_actor_id != &actor_id);
            inner
                .receipts
                .retain(|(_, receipt_actor_id, _), _| receipt_actor_id != &actor_id);
            inner.assignments.retain(|_, assignment| {
                assignment.from_actor_id != actor_id && assignment.to_actor_id != actor_id
            });
        }
        Mutation::ChannelCreate(c) => {
            inner.channels.insert(c.id.clone(), c);
        }
        Mutation::ThreadCreate(t) => {
            inner.threads.insert(t.id.clone(), t);
        }
        Mutation::TaskUpsert(t) => {
            inner.tasks.insert(t.id.clone(), t);
        }
        Mutation::TaskAssignmentUpsert(a) => {
            inner.assignments.insert(a.id.clone(), a);
        }
        Mutation::TaskRefUpsert(r) => {
            inner.task_refs.insert(r.id.clone(), r);
        }
        Mutation::TaskArtifactLinkUpsert(l) => {
            inner.task_artifact_links.insert(l.id.clone(), l);
        }
        Mutation::TaskFactUpsert(f) => {
            inner.task_facts.insert(f.id.clone(), f);
        }
        Mutation::TaskProjectionUpsert(p) => {
            inner.task_projections.insert(p.id.clone(), p);
        }
        Mutation::WorkspaceLeaseUpsert(l) => {
            inner.workspace_leases.insert(l.id.clone(), l);
        }
        Mutation::TaskChangeUpsert(c) => {
            inner.task_change_seq = inner.task_change_seq.max(c.cursor);
            inner.task_changes.insert(c.id.clone(), c);
        }
        Mutation::TaskChangeDeliveryUpsert(d) => {
            inner
                .task_change_deliveries
                .insert((d.change.id.clone(), d.recipient_actor_id.clone()), d);
        }
        Mutation::TurnOpen(t) => {
            inner.turns.insert(t.id.clone(), t);
        }
        Mutation::TurnClose {
            turn_id,
            status,
            closed_at,
        } => {
            if let Some(t) = inner.turns.get_mut(&turn_id) {
                t.status = status;
                t.closed_at = Some(closed_at);
            }
        }
        Mutation::EventAppend(e) => {
            let scope = e.scope.clone();
            inner
                .events_by_scope
                .entry(scope)
                .or_default()
                .push(e.id.clone());
            if let Some(tid) = &e.turn_id {
                let entry = inner.turn_seq.entry(tid.clone()).or_insert(0);
                if e.seq > *entry {
                    *entry = e.seq;
                }
            }
            inner.events.insert(e.id.clone(), e);
        }
        Mutation::MembershipUpsert(m) => {
            inner
                .memberships
                .insert((m.actor_id.clone(), m.scope.clone()), m);
        }
        Mutation::DeliveryUpsert(d) => {
            inner
                .deliveries
                .insert((d.event_id.clone(), d.actor_id.clone()), d);
        }
        Mutation::MachineCommandUpsert(command) => {
            inner
                .machine_commands
                .insert(command.command_id.clone(), command);
        }
        Mutation::ReceiptRecord(r) => {
            inner
                .receipts
                .insert((r.event_id.clone(), r.actor_id.clone(), r.kind), r);
        }
        Mutation::ReminderUpsert(r) => {
            inner.reminders.insert(r.id.clone(), r);
        }
        Mutation::ArtifactCreate(a) => {
            inner.artifacts.insert(a.id.clone(), a);
        }
        Mutation::TraceAppend(frame) => {
            let entry = inner.trace_seq.entry(frame.turn_id.clone()).or_insert(0);
            if frame.seq > *entry {
                *entry = frame.seq;
            }
            inner
                .trace_by_turn
                .entry(frame.turn_id.clone())
                .or_default()
                .push(frame);
        }
        Mutation::ChannelUpdate { channel_id, title } => {
            if let Some(c) = inner.channels.get_mut(&channel_id) {
                c.title = title;
            }
        }
        Mutation::ChannelDelete { channel_id } => {
            inner.channels.remove(&channel_id);
            let task_ids: std::collections::HashSet<String> = inner
                .tasks
                .values()
                .filter(|task| task.channel_id == channel_id)
                .map(|task| task.id.clone())
                .collect();
            inner.tasks.retain(|_, task| task.channel_id != channel_id);
            inner
                .assignments
                .retain(|_, assignment| !task_ids.contains(&assignment.task_id));
        }
        Mutation::ThreadUpdate { thread_id, title } => {
            if let Some(t) = inner.threads.get_mut(&thread_id) {
                t.title = title;
            }
        }
        Mutation::ThreadArchive {
            thread_id,
            archived_at,
        } => {
            if let Some(t) = inner.threads.get_mut(&thread_id) {
                t.archived_at = archived_at;
            }
        }
        Mutation::ThreadDelete { thread_id } => {
            inner.threads.remove(&thread_id);
            let scope = ScopeRef {
                kind: ScopeKind::Thread,
                id: thread_id,
            };
            inner.events_by_scope.remove(&scope);
            let task_ids: std::collections::HashSet<String> = inner
                .tasks
                .values()
                .filter(|task| task.canonical_thread_id == scope.id)
                .map(|task| task.id.clone())
                .collect();
            inner
                .tasks
                .retain(|_, task| task.canonical_thread_id != scope.id);
            inner
                .assignments
                .retain(|_, assignment| !task_ids.contains(&assignment.task_id));
        }
        Mutation::ChannelGrant {
            channel_id,
            actor_id,
        } => {
            if let Some(c) = inner.channels.get_mut(&channel_id) {
                if !c.members.iter().any(|m| m == &actor_id) {
                    c.members.push(actor_id);
                }
            }
        }
        Mutation::ChannelRevoke {
            channel_id,
            actor_id,
        } => {
            if let Some(c) = inner.channels.get_mut(&channel_id) {
                c.members.retain(|m| m != &actor_id);
            }
        }
    }
}

fn short_id() -> String {
    let id = Uuid::new_v4().simple().to_string();
    id[..12].to_string()
}

fn can_access_scope_inner(inner: &Inner, scope: &ScopeRef, actor_id: &str) -> bool {
    let channel_id = match scope.kind {
        ScopeKind::Channel => scope.id.as_str(),
        ScopeKind::Thread => match inner.threads.get(&scope.id) {
            Some(thread) => thread.channel_id.as_str(),
            None => return false,
        },
    };
    inner
        .channels
        .get(channel_id)
        .is_some_and(|channel| match channel.visibility {
            ChannelVisibility::Public => true,
            ChannelVisibility::Private => channel.members.iter().any(|member| member == actor_id),
        })
}

fn event_search_text(event: &Event) -> String {
    let mut parts = Vec::new();
    if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
        parts.push(text);
    }
    if let Some(title) = event.payload.get("title").and_then(|v| v.as_str()) {
        parts.push(title);
    }
    parts.join("\n").to_ascii_lowercase()
}

fn event_title(event: &Event) -> String {
    for key in ["title", "text", "message"] {
        if let Some(value) = event.payload.get(key).and_then(|v| v.as_str()) {
            if let Some(line) = value.lines().find(|line| !line.trim().is_empty()) {
                return line.trim().chars().take(120).collect();
            }
        }
    }
    format!("Task from {}", event.id)
}

fn unique_nonempty(ids: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for id in ids {
        if id.trim().is_empty() {
            continue;
        }
        if seen.insert(id.clone()) {
            out.push(id);
        }
    }
    out
}

fn is_terminal_task_status(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Done | TaskStatus::Failed | TaskStatus::Canceled
    )
}

fn is_terminal_assignment_status_for_store(status: TaskAssignmentStatus) -> bool {
    matches!(
        status,
        TaskAssignmentStatus::Completed
            | TaskAssignmentStatus::Failed
            | TaskAssignmentStatus::Canceled
    )
}

fn normalize_task_ref(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn binding_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn task_fact_signature(
    target_key: &str,
    kind: &str,
    subject: &serde_json::Value,
    authority: &str,
    authority_binding: &serde_json::Value,
    payload_schema: &str,
    payload: &serde_json::Value,
    observed_fields: &[String],
    unobserved_fields: &[String],
    snapshot_completeness: Option<TaskSnapshotCompleteness>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    target_key.hash(&mut hasher);
    kind.hash(&mut hasher);
    subject.to_string().hash(&mut hasher);
    authority.hash(&mut hasher);
    authority_binding.to_string().hash(&mut hasher);
    payload_schema.hash(&mut hasher);
    payload.to_string().hash(&mut hasher);
    observed_fields.hash(&mut hasher);
    unobserved_fields.hash(&mut hasher);
    snapshot_completeness.hash(&mut hasher);
    format!("hash:{:016x}", hasher.finish())
}

fn json_pair_hash(a: &serde_json::Value, b: &serde_json::Value) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    a.to_string().hash(&mut hasher);
    b.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn json_path_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

fn json_field_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn json_path_array_strings(value: &serde_json::Value, path: &[&str]) -> Vec<String> {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return Vec::new();
        };
        current = next;
    }
    current
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn actor_capabilities(actor: &Actor) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(value) = actor.capabilities.as_ref() {
        if let Some(arr) = value.as_array() {
            out.extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
        if let Some(arr) = value
            .get("capabilities")
            .and_then(serde_json::Value::as_array)
        {
            out.extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
        if let Some(arr) = value.get("tools").and_then(serde_json::Value::as_array) {
            out.extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
    }
    unique_nonempty(out)
}

fn actor_revision(actor: &Actor) -> Option<String> {
    actor
        .capabilities
        .as_ref()
        .and_then(|v| {
            v.get("revision")
                .and_then(serde_json::Value::as_str)
                .or_else(|| v.get("specRevision").and_then(serde_json::Value::as_str))
                .or_else(|| v.get("profileRevision").and_then(serde_json::Value::as_str))
        })
        .map(str::to_string)
        .or_else(|| {
            actor
                ._meta
                .as_ref()
                .and_then(|m| m.get("revision"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
}

fn default_task_change_recipients(task: &Task, extra: &[String]) -> Vec<String> {
    let mut recipients = Vec::new();
    if let Some(owner) = task.owner_actor_id.as_ref() {
        recipients.push(owner.clone());
    }
    recipients.push(task.requester_actor_id.clone());
    recipients.extend(extra.iter().cloned());
    unique_nonempty(recipients)
}

fn guard_missing_inputs(store: &Store, assignment: &TaskAssignment) -> Vec<String> {
    let mut missing = Vec::new();
    let Some(contract) = assignment.contract.as_ref() else {
        return missing;
    };
    let inner = store.inner.read();
    for artifact_id in json_path_array_strings(contract, &["context", "required_artifacts"]) {
        let Some(task) = inner.tasks.get(&assignment.task_id) else {
            missing.push(format!("task:{}", assignment.task_id));
            return missing;
        };
        let exists = task.artifact_ids.iter().any(|id| id == &artifact_id)
            || inner.task_artifact_links.values().any(|link| {
                link.task_id == assignment.task_id
                    && link.artifact_id == artifact_id
                    && link.status == TaskArtifactLinkStatus::Active
            });
        if !exists {
            missing.push(format!("artifact:{artifact_id}"));
        }
    }
    for fact_id in json_path_array_strings(contract, &["context", "required_facts"]) {
        let exists = inner.task_facts.get(&fact_id).is_some_and(|fact| {
            fact.task_id == assignment.task_id && fact.status == TaskFactStatus::Active
        });
        if !exists {
            missing.push(format!("fact:{fact_id}"));
        }
    }
    for fact_id in json_path_array_strings(contract, &["context", "required_validation_facts"]) {
        let exists = inner.task_facts.get(&fact_id).is_some_and(|fact| {
            fact.task_id == assignment.task_id && fact.status == TaskFactStatus::Active
        });
        if !exists {
            missing.push(format!("validation_fact:{fact_id}"));
        }
    }
    missing
}

fn assignment_guards_allow_completion(guards: &serde_json::Value) -> bool {
    let missing_clean = guards
        .get("missing")
        .and_then(serde_json::Value::as_array)
        .is_none_or(|items| items.is_empty());
    let stale_clean = guards
        .get("stale")
        .and_then(serde_json::Value::as_array)
        .is_none_or(|items| items.is_empty());
    let lease_clean = guards
        .get("lease")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|v| v == "valid" || v == "unclaimed");
    let runtime_clean = guards
        .get("runtime_revision")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|v| v == "matched" || v == "unknown");
    let assignment_running = guards
        .get("assignment_status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|v| v == "running");
    missing_clean && stale_clean && lease_clean && runtime_clean && assignment_running
}

fn validate_completion_result_envelope(
    assignment: &TaskAssignment,
    envelope: &serde_json::Value,
) -> StoreResult<()> {
    if !envelope.is_object() {
        return Err(StoreError::InvalidState(format!(
            "completed assignment {} result envelope must be an object",
            assignment.id
        )));
    }
    let envelope_assignment_id = json_field_string(envelope, "assignmentId")
        .or_else(|| json_field_string(envelope, "assignment_id"));
    if envelope_assignment_id.as_deref() != Some(assignment.id.as_str()) {
        return Err(StoreError::InvalidState(format!(
            "completed assignment {} result envelope must reference assignment_id",
            assignment.id
        )));
    }
    let status = json_field_string(envelope, "status").unwrap_or_default();
    if status.trim().is_empty() {
        return Err(StoreError::InvalidState(format!(
            "completed assignment {} result envelope requires status",
            assignment.id
        )));
    }
    if envelope
        .get("resultArtifacts")
        .or_else(|| envelope.get("result_artifacts"))
        .and_then(serde_json::Value::as_array)
        .is_none_or(|items| items.is_empty())
        && envelope
            .get("resultFacts")
            .or_else(|| envelope.get("result_facts"))
            .and_then(serde_json::Value::as_array)
            .is_none_or(|items| items.is_empty())
        && envelope
            .get("evidenceRefs")
            .or_else(|| envelope.get("evidence_refs"))
            .and_then(serde_json::Value::as_array)
            .is_none_or(|items| items.is_empty())
    {
        return Err(StoreError::InvalidState(format!(
            "completed assignment {} result envelope requires result artifact, fact, or evidence refs",
            assignment.id
        )));
    }
    Ok(())
}

fn next_repeat_after(from: chrono::DateTime<Utc>, rule: &str) -> Option<chrono::DateTime<Utc>> {
    if let Some(raw) = rule.strip_prefix("every:") {
        return parse_duration_seconds(raw)
            .filter(|seconds| *seconds > 0)
            .map(|seconds| from + ChronoDuration::seconds(seconds));
    }

    if let Some(raw) = rule.strip_prefix("daily@") {
        let (hour, minute) = parse_hh_mm(raw)?;
        let today = from.date_naive().and_hms_opt(hour, minute, 0)?.and_utc();
        return Some(if today > from {
            today
        } else {
            today + ChronoDuration::days(1)
        });
    }

    if let Some(raw) = rule.strip_prefix("weekly:") {
        let (days_raw, time_raw) = raw.split_once('@')?;
        let (hour, minute) = parse_hh_mm(time_raw)?;
        let days: Vec<Weekday> = days_raw.split(',').filter_map(parse_weekday).collect();
        if days.is_empty() {
            return None;
        }
        for offset in 0..=7 {
            let date = from.date_naive() + ChronoDuration::days(offset);
            if !days.contains(&date.weekday()) {
                continue;
            }
            let candidate = date.and_hms_opt(hour, minute, 0)?.and_utc();
            if candidate > from {
                return Some(candidate);
            }
        }
    }

    None
}

fn parse_duration_seconds(raw: &str) -> Option<i64> {
    if raw.is_empty() {
        return None;
    }
    let (num, unit) = raw.split_at(raw.len().saturating_sub(1));
    let value: i64 = num.parse().ok()?;
    match unit {
        "s" => Some(value),
        "m" => Some(value * 60),
        "h" => Some(value * 60 * 60),
        "d" => Some(value * 60 * 60 * 24),
        _ => raw.parse().ok(),
    }
}

fn parse_hh_mm(raw: &str) -> Option<(u32, u32)> {
    let (hh, mm) = raw.split_once(':')?;
    let hour: u32 = hh.parse().ok()?;
    let minute: u32 = mm.parse().ok()?;
    if hour < 24 && minute < 60 {
        Some((hour, minute))
    } else {
        None
    }
}

fn parse_weekday(raw: &str) -> Option<Weekday> {
    match raw.to_ascii_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

// silence unused-import lint while keeping BTreeMap available for future expansion
#[allow(dead_code)]
fn _meta_keep() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};

    /// Per-test journal file under the OS temp dir. We don't bother cleaning
    /// up — the file is tiny and lives in /tmp which the OS will sweep.
    fn fresh_store() -> Arc<Store> {
        let dir = std::env::temp_dir().join(format!("joi-store-test-{}", Uuid::new_v4().simple()));
        let path: PathBuf = dir.join("journal.jsonl");
        let journal = Journal::open(path).expect("open journal");
        Store::open(journal).expect("open store")
    }

    #[test]
    fn delete_actor_removes_actor_and_channel_membership_on_replay() {
        let store = fresh_store();
        let actor = Actor {
            id: "actor_agent_qa".into(),
            kind: ActorKind::Agent,
            display_name: "QA".into(),
            capabilities: None,
            _meta: None,
        };
        store.upsert_actor(actor).expect("upsert actor");
        let channel = store
            .create_channel("private".into(), Some("actor_agent_qa".into()))
            .expect("create channel");

        assert!(store.delete_actor("actor_agent_qa").expect("delete actor"));
        assert!(store.get_actor("actor_agent_qa").is_none());
        assert!(!store.is_channel_member(&channel.id, "actor_agent_qa"));

        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let replayed = Store::open(journal).unwrap();
        assert!(replayed.get_actor("actor_agent_qa").is_none());
        assert!(!replayed.is_channel_member(&channel.id, "actor_agent_qa"));
    }

    fn append_channel_root(
        store: &Arc<Store>,
        channel_id: &str,
        actor_id: &str,
        text: &str,
    ) -> String {
        store
            .append_event(
                "content.add".into(),
                actor_id.into(),
                ScopeRef {
                    kind: ScopeKind::Channel,
                    id: channel_id.into(),
                },
                None,
                serde_json::json!({ "text": text }),
                vec![],
                None,
            )
            .expect("append root event")
            .id
    }

    fn create_thread_under(
        store: &Arc<Store>,
        channel_id: &str,
        actor_id: &str,
        title: &str,
    ) -> Thread {
        let root_event_id = append_channel_root(store, channel_id, actor_id, title);
        store
            .create_thread(channel_id.into(), title.into(), root_event_id)
            .expect("create thread")
    }

    fn create_owned_task(store: &Arc<Store>, channel_id: &str, title: &str) -> Task {
        let root_event_id = append_channel_root(store, channel_id, "actor_owner", title);
        store
            .create_task(
                root_event_id,
                Some(title.into()),
                String::new(),
                "actor_owner".into(),
                Some("actor_owner".into()),
                Some(TaskStatus::InProgress),
                None,
                None,
                Some("test@epoch".into()),
            )
            .expect("create task")
    }

    fn put_test_artifact(store: &Arc<Store>, id: &str) -> Artifact {
        let artifact = Artifact {
            id: id.into(),
            uri: format!("artifact://{id}"),
            kind: ArtifactKind::File,
            name: id.into(),
            media_type: "application/json".into(),
            size: 2,
            checksum: format!("sha256:{id}"),
            created_by: "actor_owner".into(),
            created_at: Utc::now(),
            _meta: None,
        };
        store.put_artifact(artifact).expect("put artifact")
    }

    #[test]
    fn update_channel_changes_title_and_persists_via_replay() {
        let store = fresh_store();
        let ch = store
            .create_channel("orig title".into(), None)
            .expect("create channel");
        let updated = store
            .update_channel(&ch.id, "renamed".into())
            .expect("update channel");
        assert_eq!(updated.title, "renamed");
        assert_eq!(store.get_channel(&ch.id).unwrap().title, "renamed");

        // Re-open from the same journal: the rename must replay.
        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store2 = Store::open(journal).unwrap();
        assert_eq!(store2.get_channel(&ch.id).unwrap().title, "renamed");
    }

    #[test]
    fn delete_channel_refuses_when_threads_exist() {
        let store = fresh_store();
        let ch = store.create_channel("keep me".into(), None).unwrap();
        let _t = create_thread_under(&store, &ch.id, "actor_owner", "t");

        let err = store
            .delete_channel(&ch.id, false)
            .expect_err("delete should refuse");
        match err {
            StoreError::Conflict(msg) => assert!(msg.contains("thread"), "got {msg}"),
            other => panic!("expected Conflict, got {other:?}"),
        }
        assert!(store.get_channel(&ch.id).is_some(), "channel must remain");
    }

    #[test]
    fn create_thread_requires_channel_root_event() {
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        let t = create_thread_under(&store, &ch.id, "actor_owner", "root");
        let child_event = store
            .append_event(
                "content.add".into(),
                "actor_owner".into(),
                ScopeRef {
                    kind: ScopeKind::Thread,
                    id: t.id.clone(),
                },
                None,
                serde_json::json!({ "text": "thread reply" }),
                vec![],
                None,
            )
            .expect("append thread event")
            .id;

        let err = store
            .create_thread(ch.id.clone(), "nested".into(), child_event)
            .expect_err("thread-scoped root must be rejected");
        assert!(matches!(err, StoreError::InvalidState(_)), "got {err:?}");
    }

    #[test]
    fn create_thread_rejects_duplicate_root_event() {
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        let root_event_id = append_channel_root(&store, &ch.id, "actor_owner", "root");
        store
            .create_thread(ch.id.clone(), "first".into(), root_event_id.clone())
            .expect("first thread");

        let err = store
            .create_thread(ch.id.clone(), "second".into(), root_event_id)
            .expect_err("duplicate root must be rejected");
        assert!(matches!(err, StoreError::Conflict(_)), "got {err:?}");
    }

    #[test]
    fn archive_thread_hides_from_default_list_and_replays() {
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        let t1 = create_thread_under(&store, &ch.id, "actor_owner", "first");
        let t2 = create_thread_under(&store, &ch.id, "actor_owner", "second");

        let archived = store.archive_thread(&t1.id, true).expect("archive thread");
        assert!(archived.archived_at.is_some());
        assert_eq!(
            store
                .list_threads_filtered(Some(&ch.id), false)
                .into_iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            vec![t2.id.clone()]
        );
        assert_eq!(
            store
                .list_threads_filtered(Some(&ch.id), true)
                .into_iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            vec![t1.id.clone()]
        );

        let replayed = Store::open(Journal::open(store.journal.path().to_path_buf()).unwrap())
            .expect("replay");
        assert!(replayed.get_thread(&t1.id).unwrap().archived_at.is_some());
        assert_eq!(
            replayed
                .list_threads_filtered(Some(&ch.id), false)
                .into_iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            vec![t2.id]
        );
    }

    #[test]
    fn unarchive_thread_returns_to_default_list() {
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        let t = create_thread_under(&store, &ch.id, "actor_owner", "thread");

        store.archive_thread(&t.id, true).expect("archive thread");
        let restored = store.archive_thread(&t.id, false).expect("restore thread");

        assert!(restored.archived_at.is_none());
        assert_eq!(
            store
                .list_threads_filtered(Some(&ch.id), false)
                .into_iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            vec![t.id]
        );
        assert!(store.list_threads_filtered(Some(&ch.id), true).is_empty());
    }

    #[test]
    fn create_task_anchors_to_channel_event_and_creates_canonical_thread() {
        let store = fresh_store();
        let ch = store
            .create_channel("tasks".into(), Some("actor_owner".into()))
            .unwrap();
        let root_event_id = append_channel_root(&store, &ch.id, "actor_owner", "write docs");

        let task = store
            .create_task(
                root_event_id.clone(),
                None,
                "write docs".into(),
                "actor_owner".into(),
                Some("actor_owner".into()),
                None,
                None,
                None,
                None,
            )
            .expect("create task");

        assert_eq!(task.number, 1);
        assert_eq!(task.source_event_id, root_event_id);
        assert_eq!(task.status, TaskStatus::Claimed);
        let thread = store
            .get_thread(&task.canonical_thread_id)
            .expect("canonical thread");
        assert_eq!(thread.root_event_id, task.source_event_id);
        assert_eq!(thread.channel_id, ch.id);

        let replayed = Store::open(Journal::open(store.journal.path().to_path_buf()).unwrap())
            .expect("replay");
        let replayed_task = replayed.get_task(&task.id).expect("replayed task");
        assert_eq!(replayed_task.canonical_thread_id, task.canonical_thread_id);
    }

    #[test]
    fn task_parent_fields_support_compound_request_split() {
        let store = fresh_store();
        let ch = store
            .create_channel("split".into(), Some("actor_owner".into()))
            .unwrap();
        let parent_event =
            append_channel_root(&store, &ch.id, "actor_owner", "fix A and inspect B");
        let child_event = append_channel_root(&store, &ch.id, "actor_owner", "fix A");
        let child = store
            .create_task(
                child_event,
                Some("fix A".into()),
                String::new(),
                "actor_owner".into(),
                Some("actor_owner".into()),
                Some(TaskStatus::Claimed),
                Some(parent_event.clone()),
                None,
                Some("test@epoch".into()),
            )
            .unwrap();
        assert_eq!(
            child.parent_source_event_id.as_deref(),
            Some(parent_event.as_str())
        );
    }

    #[test]
    fn create_task_rejects_thread_event_and_duplicate_source() {
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        let thread = create_thread_under(&store, &ch.id, "actor_owner", "root");
        let thread_event = store
            .append_event(
                "content.add".into(),
                "actor_owner".into(),
                ScopeRef {
                    kind: ScopeKind::Thread,
                    id: thread.id,
                },
                None,
                serde_json::json!({ "text": "child" }),
                vec![],
                None,
            )
            .unwrap();
        let err = store
            .create_task(
                thread_event.id,
                None,
                String::new(),
                "actor_owner".into(),
                None,
                None,
                None,
                None,
                None,
            )
            .expect_err("thread event cannot anchor task");
        assert!(matches!(err, StoreError::InvalidState(_)), "got {err:?}");

        let root_event_id = append_channel_root(&store, &ch.id, "actor_owner", "work");
        store
            .create_task(
                root_event_id.clone(),
                None,
                String::new(),
                "actor_owner".into(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let err = store
            .create_task(
                root_event_id,
                None,
                String::new(),
                "actor_owner".into(),
                None,
                None,
                None,
                None,
                None,
            )
            .expect_err("source event is unique");
        assert!(matches!(err, StoreError::Conflict(_)), "got {err:?}");
    }

    #[test]
    fn create_task_is_unique_under_concurrent_source_event_claims() {
        let store = fresh_store();
        let ch = store
            .create_channel("race".into(), Some("actor_owner".into()))
            .unwrap();
        let root_event_id = append_channel_root(&store, &ch.id, "actor_owner", "race task");
        let barrier = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let store = store.clone();
                let root_event_id = root_event_id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.create_task(
                        root_event_id,
                        Some("race task".into()),
                        String::new(),
                        "actor_owner".into(),
                        Some("actor_owner".into()),
                        None,
                        None,
                        None,
                        None,
                    )
                })
            })
            .collect::<Vec<_>>();

        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("task thread"))
            .collect::<Vec<_>>();
        let successes = results.iter().filter(|result| result.is_ok()).count();
        let conflicts = results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::Conflict(_))))
            .count();
        assert_eq!(successes, 1);
        assert_eq!(conflicts, 1);
        assert_eq!(store.list_tasks(Some(&ch.id), None, None, &[]).len(), 1);
        assert_eq!(store.list_threads(Some(&ch.id)).len(), 1);
    }

    #[test]
    fn task_assignment_result_round_trips_through_replay() {
        let store = fresh_store();
        let ch = store
            .create_channel("review".into(), Some("actor_owner".into()))
            .unwrap();
        store.grant_channel(&ch.id, "actor_reviewer").unwrap();
        let root_event_id = append_channel_root(&store, &ch.id, "actor_owner", "story");
        let task = store
            .create_task(
                root_event_id,
                Some("story".into()),
                String::new(),
                "actor_owner".into(),
                Some("actor_owner".into()),
                Some(TaskStatus::InProgress),
                None,
                None,
                None,
            )
            .unwrap();
        let (assignment, task, _) = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_reviewer".into(),
                TaskAssignmentType::Review,
                "review story".into(),
                Some(serde_json::json!({})),
                None,
            )
            .unwrap();
        assert_eq!(task.status, TaskStatus::WaitingReview);
        let result_event = store
            .append_event(
                "content.add".into(),
                "actor_reviewer".into(),
                ScopeRef {
                    kind: ScopeKind::Thread,
                    id: task.canonical_thread_id.clone(),
                },
                None,
                serde_json::json!({ "text": "looks good" }),
                vec![],
                None,
            )
            .unwrap();
        let (updated, _) = store
            .update_task_assignment(
                &assignment.id,
                Some(TaskAssignmentStatus::Running),
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
        assert_eq!(updated.status, TaskAssignmentStatus::Running);
        let envelope = serde_json::json!({
            "assignment_id": assignment.id,
            "status": "completed",
            "verdict": "pass",
            "evidence_refs": [result_event.id.clone()]
        });
        let (updated, _) = store
            .update_task_assignment(
                &assignment.id,
                Some(TaskAssignmentStatus::Completed),
                Some(result_event.id.clone()),
                Some("looks good".into()),
                Some(envelope),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
        assert_eq!(updated.status, TaskAssignmentStatus::Completed);
        assert_eq!(
            updated.result_event_id.as_deref(),
            Some(result_event.id.as_str())
        );

        let replayed = Store::open(Journal::open(store.journal.path().to_path_buf()).unwrap())
            .expect("replay");
        let replayed_assignment = replayed
            .get_assignment(&assignment.id)
            .expect("replayed assignment");
        assert_eq!(replayed_assignment.result_summary, "looks good");
        assert_eq!(replayed_assignment.status, TaskAssignmentStatus::Completed);
    }

    #[test]
    fn task_ref_conflict_and_lookup_are_durable() {
        let store = fresh_store();
        let ch = store
            .create_channel("refs".into(), Some("actor_owner".into()))
            .unwrap();
        let task_a = create_owned_task(&store, &ch.id, "task a");
        let task_b = create_owned_task(&store, &ch.id, "task b");

        let tref = store
            .attach_task_ref(
                &task_a.id,
                "branch".into(),
                "git_branch".into(),
                "feature/x".into(),
                "repo#feature/x".into(),
                serde_json::json!({}),
                TaskRefConfidence::Confirmed,
                TaskRefStatus::Active,
                None,
                Some(task_a.source_event_id.clone()),
                "actor_owner".into(),
            )
            .unwrap();
        let conflict = store
            .attach_task_ref(
                &task_b.id,
                "branch".into(),
                "git_branch".into(),
                "feature/x".into(),
                "repo#feature/x".into(),
                serde_json::json!({}),
                TaskRefConfidence::Confirmed,
                TaskRefStatus::Active,
                None,
                None,
                "actor_owner".into(),
            )
            .expect_err("same active confirmed ref cannot bind two non-terminal tasks");
        assert!(matches!(conflict, StoreError::Conflict(_)));

        let (refs, tasks) = store.find_task_refs(
            Some(&ch.id),
            "branch",
            "git_branch",
            "repo#feature/x",
            Some(TaskRefConfidence::Confirmed),
            Some(TaskRefStatus::Active),
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].id, tref.id);
        assert_eq!(tasks[0].id, task_a.id);

        let replayed = Store::open(Journal::open(store.journal.path().to_path_buf()).unwrap())
            .expect("replay");
        assert_eq!(replayed.list_task_refs(&task_a.id).len(), 1);
        for (kind, subtype, normalized) in [
            ("external_url", "code_mr", "code:example/repo!1"),
            ("external_url", "feedback", "feedback:42"),
            ("thread_alias", "legacy_thread", "thread_old"),
            ("title_alias", "human_title", "fix-login"),
        ] {
            store
                .attach_task_ref(
                    &task_a.id,
                    kind.into(),
                    subtype.into(),
                    normalized.into(),
                    normalized.into(),
                    serde_json::json!({}),
                    TaskRefConfidence::Inferred,
                    TaskRefStatus::Active,
                    None,
                    None,
                    "actor_owner".into(),
                )
                .unwrap();
        }
        assert_eq!(store.list_task_refs(&task_a.id).len(), 5);
    }

    #[test]
    fn concurrent_confirmed_task_ref_attach_keeps_single_active_owner() {
        let store = fresh_store();
        let ch = store
            .create_channel("ref-race".into(), Some("actor_owner".into()))
            .unwrap();
        let task_a = create_owned_task(&store, &ch.id, "task a");
        let task_b = create_owned_task(&store, &ch.id, "task b");
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = [task_a.id.clone(), task_b.id.clone()]
            .into_iter()
            .map(|task_id| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.attach_task_ref(
                        &task_id,
                        "branch".into(),
                        "git_branch".into(),
                        "repo#race".into(),
                        "repo#race".into(),
                        serde_json::json!({}),
                        TaskRefConfidence::Confirmed,
                        TaskRefStatus::Active,
                        None,
                        None,
                        "actor_owner".into(),
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(StoreError::Conflict(_))))
                .count(),
            1
        );
        let (refs, tasks) = store.find_task_refs(
            Some(&ch.id),
            "branch",
            "git_branch",
            "repo#race",
            Some(TaskRefConfidence::Confirmed),
            Some(TaskRefStatus::Active),
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn task_artifact_link_activate_supersedes_current_link() {
        let store = fresh_store();
        let ch = store
            .create_channel("artifacts".into(), Some("actor_owner".into()))
            .unwrap();
        let task = create_owned_task(&store, &ch.id, "artifact task");
        put_test_artifact(&store, "art_a");
        put_test_artifact(&store, "art_b");

        let first = store
            .attach_task_artifact_link(
                &task.id,
                "art_a".into(),
                "effective-context.v1".into(),
                "current".into(),
                None,
                TaskArtifactLinkStatus::Active,
                serde_json::json!({}),
                serde_json::json!({"target_key":"repo#branch","purpose":"context"}),
                "actor_owner".into(),
            )
            .unwrap();
        let second = store
            .attach_task_artifact_link(
                &task.id,
                "art_b".into(),
                "effective-context.v1".into(),
                "current".into(),
                None,
                TaskArtifactLinkStatus::Proposal,
                serde_json::json!({"supersedes":[first.artifact_id]}),
                serde_json::json!({"target_key":"repo#branch","purpose":"context"}),
                "actor_owner".into(),
            )
            .unwrap();
        let (active, superseded) = store
            .activate_task_artifact_link(&second.id, vec![first.id.clone()])
            .unwrap();
        assert_eq!(active.status, TaskArtifactLinkStatus::Active);
        assert_eq!(superseded[0].status, TaskArtifactLinkStatus::Superseded);
        let links = store.list_task_artifact_links(&task.id, Some(TaskArtifactLinkStatus::Active));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, second.id);
    }

    #[test]
    fn task_fact_idempotency_lifecycle_and_partial_snapshot_fields_round_trip() {
        let store = fresh_store();
        let ch = store
            .create_channel("facts".into(), Some("actor_owner".into()))
            .unwrap();
        let task = create_owned_task(&store, &ch.id, "fact task");
        let (first, created) = store
            .append_task_fact(
                &task.id,
                "mr:1".into(),
                "ci.status".into(),
                TaskFactType::Status,
                serde_json::json!({"pipeline":"p1"}),
                Some("ci:p1".into()),
                TaskFactStatus::Active,
                Vec::new(),
                None,
                "code".into(),
                serde_json::json!({"reviewer":"bot"}),
                None,
                Some("100".into()),
                Some("snap-100".into()),
                Some(Utc::now()),
                vec!["state".into()],
                vec!["duration".into()],
                Some("partial_response".into()),
                Some(TaskSnapshotCompleteness::Partial),
                "actor_ci".into(),
                "ci failed".into(),
                vec!["evt_1".into()],
                None,
                "ci-status.v1".into(),
                serde_json::json!({"state":"failed"}),
            )
            .unwrap();
        assert!(created);
        let (_, duplicated) = store
            .append_task_fact(
                &task.id,
                "mr:1".into(),
                "ci.status".into(),
                TaskFactType::Status,
                serde_json::json!({"pipeline":"p1"}),
                Some("ci:p1".into()),
                TaskFactStatus::Active,
                Vec::new(),
                None,
                "code".into(),
                serde_json::json!({"reviewer":"bot"}),
                None,
                Some("100".into()),
                Some("snap-100".into()),
                None,
                vec!["state".into()],
                vec!["duration".into()],
                Some("partial_response".into()),
                Some(TaskSnapshotCompleteness::Partial),
                "actor_ci".into(),
                "ci failed".into(),
                vec![],
                None,
                "ci-status.v1".into(),
                serde_json::json!({"state":"failed"}),
            )
            .unwrap();
        assert!(!duplicated);

        let (second, _) = store
            .append_task_fact(
                &task.id,
                "mr:1".into(),
                "ci.status".into(),
                TaskFactType::Status,
                serde_json::json!({"pipeline":"p1"}),
                Some("ci:p1:pass".into()),
                TaskFactStatus::Active,
                vec![first.id.clone()],
                None,
                "code".into(),
                serde_json::json!({"reviewer":"bot"}),
                None,
                Some("101".into()),
                Some("snap-101".into()),
                Some(Utc::now()),
                vec!["state".into()],
                vec!["duration".into()],
                None,
                Some(TaskSnapshotCompleteness::Partial),
                "actor_ci".into(),
                "ci passed".into(),
                vec![],
                None,
                "ci-status.v1".into(),
                serde_json::json!({"state":"passed"}),
            )
            .unwrap();
        let first_after = store
            .list_task_facts(&task.id, None, None, None)
            .into_iter()
            .find(|fact| fact.id == first.id)
            .unwrap();
        assert_eq!(first_after.status, TaskFactStatus::Superseded);
        assert_eq!(second.observed_fields, vec!["state"]);
        assert_eq!(
            second.snapshot_completeness,
            Some(TaskSnapshotCompleteness::Partial)
        );
        let (retracted, _) = store
            .append_task_fact(
                &task.id,
                "mr:1".into(),
                "human.decision".into(),
                TaskFactType::Decision,
                serde_json::json!({}),
                Some("decision:1".into()),
                TaskFactStatus::Retracted,
                Vec::new(),
                Some(second.id.clone()),
                "human".into(),
                serde_json::json!({}),
                None,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                Some(TaskSnapshotCompleteness::Unknown),
                "actor_owner".into(),
                "decision retracted".into(),
                vec![],
                None,
                "decision.v1".into(),
                serde_json::json!({}),
            )
            .unwrap();
        let (conflict, _) = store
            .append_task_fact(
                &task.id,
                "mr:1".into(),
                "ci.status".into(),
                TaskFactType::Status,
                serde_json::json!({}),
                Some("ci:conflict".into()),
                TaskFactStatus::Conflict,
                Vec::new(),
                None,
                "code".into(),
                serde_json::json!({}),
                None,
                Some("102".into()),
                Some("snap-102".into()),
                None,
                vec!["state".into()],
                Vec::new(),
                None,
                Some(TaskSnapshotCompleteness::Complete),
                "actor_ci".into(),
                "ci conflict".into(),
                vec![],
                None,
                "ci-status.v1".into(),
                serde_json::json!({"state":"unknown"}),
            )
            .unwrap();
        assert_eq!(retracted.status, TaskFactStatus::Retracted);
        assert_eq!(conflict.status, TaskFactStatus::Conflict);
    }

    #[test]
    fn concurrent_duplicate_fact_append_creates_one_record() {
        let store = fresh_store();
        let ch = store
            .create_channel("fact-race".into(), Some("actor_owner".into()))
            .unwrap();
        let task = create_owned_task(&store, &ch.id, "fact race");
        let workers = 8;
        let barrier = Arc::new(Barrier::new(workers));
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let store = store.clone();
                let task_id = task.id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_task_fact(
                        &task_id,
                        "mr:race".into(),
                        "ci.status".into(),
                        TaskFactType::Status,
                        serde_json::json!({"pipeline":"p1"}),
                        Some("ci:p1".into()),
                        TaskFactStatus::Active,
                        Vec::new(),
                        None,
                        "code".into(),
                        serde_json::json!({"source":"test"}),
                        None,
                        Some("100".into()),
                        Some("snap-100".into()),
                        None,
                        vec!["state".into()],
                        Vec::new(),
                        None,
                        Some(TaskSnapshotCompleteness::Complete),
                        "actor_ci".into(),
                        "ci passed".into(),
                        Vec::new(),
                        None,
                        "ci-status.v1".into(),
                        serde_json::json!({"state":"passed"}),
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|(_, created)| *created).count(), 1);
        assert_eq!(
            store
                .list_task_facts(
                    &task.id,
                    Some("ci.status"),
                    Some(TaskFactStatus::Active),
                    None
                )
                .len(),
            1
        );
    }

    #[test]
    fn replacing_fact_from_another_task_is_rejected() {
        let store = fresh_store();
        let ch = store
            .create_channel("fact-cross-task".into(), Some("actor_owner".into()))
            .unwrap();
        let task_a = create_owned_task(&store, &ch.id, "task a");
        let task_b = create_owned_task(&store, &ch.id, "task b");
        let (fact, _) = store
            .append_task_fact(
                &task_a.id,
                "mr:1".into(),
                "ci.status".into(),
                TaskFactType::Status,
                serde_json::json!({}),
                Some("ci:a".into()),
                TaskFactStatus::Active,
                Vec::new(),
                None,
                String::new(),
                serde_json::json!({}),
                None,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                None,
                "actor_owner".into(),
                String::new(),
                Vec::new(),
                None,
                String::new(),
                serde_json::json!({}),
            )
            .unwrap();
        let err = store
            .append_task_fact(
                &task_b.id,
                "mr:1".into(),
                "ci.status".into(),
                TaskFactType::Status,
                serde_json::json!({}),
                Some("ci:b".into()),
                TaskFactStatus::Active,
                vec![fact.id],
                None,
                String::new(),
                serde_json::json!({}),
                None,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                None,
                "actor_owner".into(),
                String::new(),
                Vec::new(),
                None,
                String::new(),
                serde_json::json!({}),
            )
            .expect_err("cross-task fact replacement rejected");
        assert!(matches!(err, StoreError::InvalidState(_)));
    }

    #[test]
    fn task_projection_change_delivery_and_ack_round_trip() {
        let store = fresh_store();
        let ch = store
            .create_channel("projection".into(), Some("actor_owner".into()))
            .unwrap();
        let task = create_owned_task(&store, &ch.id, "projection task");
        let mut rx = store.subscribe();
        let projection = store
            .put_task_projection(
                &task.id,
                "summary".into(),
                "actor_projection".into(),
                TaskProjectionHealth::Fresh,
                serde_json::json!({"fact_ids":[]}),
                "task-summary.v1".into(),
                serde_json::json!({"summary":"ready"}),
            )
            .unwrap();
        assert_eq!(projection.health, TaskProjectionHealth::Fresh);
        match rx.try_recv().expect("projection emits task.changed") {
            StoreEvent::TaskChanged(changed) => assert_eq!(changed.id, task.id),
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(store.get_task_projection(&task.id, "detail").is_none());
        let stale = store
            .put_task_projection(
                &task.id,
                "detail".into(),
                "actor_projection".into(),
                TaskProjectionHealth::RepairRequired,
                serde_json::json!({}),
                "task-detail.v1".into(),
                serde_json::json!({"reason":"conflict"}),
            )
            .unwrap();
        assert_eq!(stale.health, TaskProjectionHealth::RepairRequired);
        let deliveries = store.list_task_changes("actor_owner", Some(&task.id), false, None, None);
        assert!(deliveries
            .iter()
            .any(|delivery| delivery.change.change_type == TaskChangeType::Projection));
        let change_id = deliveries
            .iter()
            .find(|delivery| delivery.change.change_type == TaskChangeType::Projection)
            .unwrap()
            .change
            .id
            .clone();
        let ack = store
            .ack_task_change(
                &change_id,
                "actor_owner",
                TaskChangeAckDisposition::NoopRecorded,
                Vec::new(),
                "projection observed".into(),
            )
            .unwrap();
        assert_eq!(ack.status, TaskChangeDeliveryStatus::Handled);
        let duplicate = store
            .ack_task_change(
                &change_id,
                "actor_owner",
                TaskChangeAckDisposition::NoopRecorded,
                Vec::new(),
                "again".into(),
            )
            .expect_err("duplicate ack rejected");
        assert!(matches!(duplicate, StoreError::InvalidState(_)));
    }

    #[test]
    fn task_change_ack_requires_reason_or_result_refs() {
        let store = fresh_store();
        let ch = store
            .create_channel("ack".into(), Some("actor_owner".into()))
            .unwrap();
        let task = create_owned_task(&store, &ch.id, "ack task");
        store
            .put_task_projection(
                &task.id,
                "summary".into(),
                "actor_projection".into(),
                TaskProjectionHealth::Fresh,
                serde_json::json!({"fact_ids":[]}),
                "task-summary.v1".into(),
                serde_json::json!({"summary":"ready"}),
            )
            .unwrap();
        let change_id = store
            .list_task_changes("actor_owner", Some(&task.id), false, None, None)
            .into_iter()
            .find(|delivery| delivery.change.change_type == TaskChangeType::Projection)
            .unwrap()
            .change
            .id;
        let err = store
            .ack_task_change(
                &change_id,
                "actor_owner",
                TaskChangeAckDisposition::NoopRecorded,
                Vec::new(),
                String::new(),
            )
            .expect_err("empty ack rejected");
        assert!(matches!(err, StoreError::InvalidState(_)));
    }

    #[test]
    fn action_response_with_task_scope_notifies_conversion_owner() {
        let store = fresh_store();
        let ch = store
            .create_channel("action".into(), Some("actor_owner".into()))
            .unwrap();
        let task = create_owned_task(&store, &ch.id, "action task");
        let request = store
            .append_event(
                "action.request".into(),
                "actor_owner".into(),
                ScopeRef {
                    kind: ScopeKind::Thread,
                    id: task.canonical_thread_id.clone(),
                },
                None,
                serde_json::json!({
                    "requestType": "choose",
                    "title": "Continue?",
                    "taskId": task.id,
                    "targetKey": "repo#main",
                    "decisionKind": "scope-decision",
                    "conversionOwnerActorId": "actor_owner"
                }),
                vec![],
                None,
            )
            .unwrap();
        let response = store
            .append_event(
                "action.response".into(),
                "actor_owner".into(),
                ScopeRef {
                    kind: ScopeKind::Thread,
                    id: task.canonical_thread_id.clone(),
                },
                None,
                serde_json::json!({"optionId":"yes"}),
                vec![Relation {
                    kind: RelationKind::RespondsTo,
                    target: Ref {
                        kind: RefKind::Event,
                        id: request.id.clone(),
                        _meta: None,
                    },
                    _meta: None,
                }],
                None,
            )
            .unwrap();
        let deliveries = store.list_task_changes("actor_owner", Some(&task.id), false, None, None);
        assert!(deliveries.iter().any(|delivery| {
            delivery.change.change_type == TaskChangeType::Action
                && delivery.change.source_ids.contains(&request.id)
                && delivery.change.source_ids.contains(&response.id)
        }));
    }

    #[test]
    fn assignment_contract_preflight_and_write_lease_conflict_are_enforced() {
        let store = fresh_store();
        let ch = store
            .create_channel("lease".into(), Some("actor_owner".into()))
            .unwrap();
        store.grant_channel(&ch.id, "actor_delivery").unwrap();
        store
            .upsert_actor(Actor {
                id: "actor_delivery".into(),
                kind: ActorKind::Agent,
                display_name: "delivery".into(),
                capabilities: Some(serde_json::json!({
                    "capabilities": ["workspace.write", "artifact.publish"],
                    "revision": "rev-a"
                })),
                _meta: None,
            })
            .unwrap();
        let task = create_owned_task(&store, &ch.id, "lease task");
        let contract = serde_json::json!({
            "target": {"target_key": "repo#main", "head": "h1"},
            "effects": {"authorized": ["repo.push"]},
            "workspace": {"resource_key": "worktree:repo", "write_mode": "write"},
            "required_capabilities": ["workspace.write"],
            "versions": {"target_actor_spec_revision": "rev-a"},
            "idempotency_key": "lease-test"
        });
        let missing_required = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Fix,
                "missing inputs".into(),
                Some(serde_json::json!({
                    "context": {"required_facts": ["fact_missing"]},
                    "idempotency_key": "missing-inputs"
                })),
                None,
            )
            .expect_err("required fact guard rejects missing inputs");
        assert!(matches!(missing_required, StoreError::InvalidState(_)));
        let (assignment, _, created) = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Fix,
                "fix".into(),
                Some(contract),
                None,
            )
            .unwrap();
        assert!(created);
        let (same, _, created) = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Fix,
                "fix duplicate".into(),
                Some(serde_json::json!({
                    "target": {"target_key": "repo#main", "head": "h1"},
                    "workspace": {"resource_key": "worktree:repo", "write_mode": "write"},
                    "idempotency_key": "lease-test"
                })),
                None,
            )
            .unwrap();
        assert_eq!(same.id, assignment.id);
        assert!(!created);
        let blocked = store
            .assignment_preflight(
                &assignment.id,
                "repo#main".into(),
                "h1".into(),
                "repo.push".into(),
            )
            .unwrap();
        assert!(!blocked.allowed);
        assert_eq!(blocked.guards["lease"], "missing");

        let (lease, conflicts) = store
            .acquire_workspace_lease(
                &assignment.id,
                "worktree:repo".into(),
                WorkspaceLeaseMode::Write,
                Utc::now() + ChronoDuration::minutes(30),
            )
            .unwrap();
        assert!(conflicts.is_empty());
        assert!(lease.is_some());
        let still_pending = store
            .assignment_preflight(
                &assignment.id,
                "repo#main".into(),
                "h1".into(),
                "repo.push".into(),
            )
            .unwrap();
        assert!(!still_pending.allowed);
        assert_eq!(still_pending.guards["assignment_status"], "pending");
        store
            .update_task_assignment(
                &assignment.id,
                Some(TaskAssignmentStatus::Running),
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
        let allowed = store
            .assignment_preflight(
                &assignment.id,
                "repo#main".into(),
                "h1".into(),
                "repo.push".into(),
            )
            .unwrap();
        assert!(allowed.allowed);

        let (other, _, _) = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Fix,
                "fix again".into(),
                Some(serde_json::json!({
                    "target": {"target_key": "repo#main", "head": "h2"},
                    "workspace": {"resource_key": "worktree:repo", "write_mode": "write"},
                    "required_capabilities": ["workspace.write"],
                    "versions": {"target_actor_spec_revision": "rev-a"},
                    "idempotency_key": "lease-test-2"
                })),
                None,
            )
            .unwrap();
        let (lease, conflicts) = store
            .acquire_workspace_lease(
                &other.id,
                "worktree:repo".into(),
                WorkspaceLeaseMode::Write,
                Utc::now() + ChronoDuration::minutes(30),
            )
            .unwrap();
        assert!(lease.is_none());
        assert_eq!(conflicts.len(), 1);

        let mismatch_create = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Investigate,
                "revision mismatch".into(),
                Some(serde_json::json!({
                    "target": {"target_key": "repo#main", "head": "h1"},
                    "versions": {"target_actor_spec_revision": "rev-b"},
                    "idempotency_key": "revision-mismatch"
                })),
                None,
            )
            .expect_err("revision mismatch rejected at create time");
        assert!(matches!(mismatch_create, StoreError::InvalidState(_)));

        let (mismatch, _, _) = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Investigate,
                "revision drift".into(),
                Some(serde_json::json!({
                    "target": {"target_key": "repo#main", "head": "h1"},
                    "versions": {"target_actor_spec_revision": "rev-a"},
                    "idempotency_key": "revision-drift"
                })),
                None,
            )
            .unwrap();
        store
            .upsert_actor(Actor {
                id: "actor_delivery".into(),
                kind: ActorKind::Agent,
                display_name: "delivery".into(),
                capabilities: Some(serde_json::json!({
                    "capabilities": ["workspace.write", "artifact.publish"],
                    "revision": "rev-c"
                })),
                _meta: None,
            })
            .unwrap();
        let preflight = store
            .assignment_preflight(
                &mismatch.id,
                "repo#main".into(),
                "h1".into(),
                "repo.push".into(),
            )
            .unwrap();
        assert!(!preflight.allowed);
        assert_eq!(preflight.guards["runtime_revision"], "mismatch");

        store
            .update_task(
                &task.id,
                Some(TaskStatus::Done),
                None,
                None,
                None,
                Vec::new(),
            )
            .unwrap();
        let terminal = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Review,
                "review done task".into(),
                None,
                None,
            )
            .expect_err("terminal task rejects review assignment");
        assert!(matches!(terminal, StoreError::InvalidState(_)));
    }

    #[test]
    fn concurrent_assignment_idempotency_returns_one_assignment() {
        let store = fresh_store();
        let ch = store
            .create_channel("assignment-race".into(), Some("actor_owner".into()))
            .unwrap();
        store.grant_channel(&ch.id, "actor_delivery").unwrap();
        let task = create_owned_task(&store, &ch.id, "assignment race");
        let barrier = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                let task_id = task.id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.create_task_assignment(
                        &task_id,
                        "actor_owner".into(),
                        "actor_delivery".into(),
                        TaskAssignmentType::Fix,
                        "fix".into(),
                        Some(serde_json::json!({
                            "idempotency_key": "same-target-head-context"
                        })),
                        Some("same-target-head-context".into()),
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|(_, _, created)| *created).count(), 1);
        let ids: std::collections::HashSet<_> = results
            .iter()
            .map(|(assignment, _, _)| assignment.id.clone())
            .collect();
        assert_eq!(ids.len(), 1);
        assert_eq!(store.list_task_assignments(&task.id).len(), 1);
    }

    #[test]
    fn required_artifact_must_be_task_active_and_validation_fact_active() {
        let store = fresh_store();
        let ch = store
            .create_channel("validation".into(), Some("actor_owner".into()))
            .unwrap();
        store.grant_channel(&ch.id, "actor_delivery").unwrap();
        let task = create_owned_task(&store, &ch.id, "validation task");
        put_test_artifact(&store, "art_required");
        let contract_without_validation = serde_json::json!({
            "context": {
                "required_artifacts": ["art_required"],
                "required_validation_facts": ["fact_missing"]
            },
            "idempotency_key": "validation-missing"
        });
        let err = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Review,
                "review".into(),
                Some(contract_without_validation.clone()),
                None,
            )
            .expect_err("unlinked artifact rejected");
        assert!(matches!(err, StoreError::InvalidState(_)));
        store
            .attach_task_artifact_link(
                &task.id,
                "art_required".into(),
                "effective-context.v1".into(),
                "current".into(),
                None,
                TaskArtifactLinkStatus::Active,
                serde_json::json!({}),
                serde_json::json!({"target_key":"repo#main","purpose":"context"}),
                "actor_owner".into(),
            )
            .unwrap();
        let err = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Review,
                "review".into(),
                Some(contract_without_validation),
                None,
            )
            .expect_err("missing validation fact rejected");
        assert!(matches!(err, StoreError::InvalidState(_)));
        let (fact, _) = store
            .append_task_fact(
                &task.id,
                "art_required".into(),
                "artifact.contract_validated".into(),
                TaskFactType::Status,
                serde_json::json!({}),
                Some("validate:art_required".into()),
                TaskFactStatus::Active,
                Vec::new(),
                None,
                "validator".into(),
                serde_json::json!({}),
                None,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
                None,
                None,
                "validator".into(),
                "artifact valid".into(),
                Vec::new(),
                Some("art_required".into()),
                "contract-validation-result.v1".into(),
                serde_json::json!({"valid": true}),
            )
            .unwrap();
        let (assignment, _, created) = store
            .create_task_assignment(
                &task.id,
                "actor_owner".into(),
                "actor_delivery".into(),
                TaskAssignmentType::Review,
                "review".into(),
                Some(serde_json::json!({
                    "context": {
                        "required_artifacts": ["art_required"],
                        "required_validation_facts": [fact.id]
                    },
                    "idempotency_key": "validation-ok"
                })),
                None,
            )
            .unwrap();
        assert!(created);
        assert_eq!(assignment.to_actor_id, "actor_delivery");
    }

    #[test]
    fn concurrent_write_lease_acquire_allows_single_holder_and_expires_visibly() {
        let store = fresh_store();
        let ch = store
            .create_channel("lease-race".into(), Some("actor_owner".into()))
            .unwrap();
        for actor in ["actor_delivery_a", "actor_delivery_b"] {
            store.grant_channel(&ch.id, actor).unwrap();
        }
        let task = create_owned_task(&store, &ch.id, "lease race");
        let mut assignments = Vec::new();
        for actor in ["actor_delivery_a", "actor_delivery_b"] {
            let (assignment, _, _) = store
                .create_task_assignment(
                    &task.id,
                    "actor_owner".into(),
                    actor.into(),
                    TaskAssignmentType::Fix,
                    "fix".into(),
                    Some(serde_json::json!({
                        "workspace": {"resource_key": "worktree:race", "write_mode": "write"},
                        "idempotency_key": actor
                    })),
                    None,
                )
                .unwrap();
            assignments.push(assignment);
        }
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = assignments
            .iter()
            .map(|assignment| {
                let store = store.clone();
                let assignment_id = assignment.id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.acquire_workspace_lease(
                        &assignment_id,
                        "worktree:race".into(),
                        WorkspaceLeaseMode::Write,
                        Utc::now() + ChronoDuration::milliseconds(50),
                    )
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect();
        assert_eq!(
            results.iter().filter(|(lease, _)| lease.is_some()).count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|(_, conflicts)| !conflicts.is_empty())
                .count(),
            1
        );
        std::thread::sleep(std::time::Duration::from_millis(80));
        let _leases = store.list_workspace_leases(Some("worktree:race"), None, false);
        assert!(store
            .list_workspace_leases(Some("worktree:race"), None, false)
            .iter()
            .any(|lease| lease.status == WorkspaceLeaseStatus::Expired));
        assert!(store
            .list_task_changes("actor_owner", Some(&task.id), false, None, None)
            .iter()
            .any(|delivery| delivery.change.summary.contains("expired")));
    }

    #[test]
    fn search_messages_respects_private_channel_acl() {
        let store = fresh_store();
        let public = store.create_channel("public".into(), None).unwrap();
        let private = store
            .create_channel("private".into(), Some("alice".into()))
            .unwrap();

        let public_scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: public.id.clone(),
        };
        let private_scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: private.id.clone(),
        };
        store
            .append_event(
                "content.add".into(),
                "bob".into(),
                public_scope,
                None,
                serde_json::json!({ "text": "needle public" }),
                Vec::new(),
                None,
            )
            .unwrap();
        store
            .append_event(
                "content.add".into(),
                "alice".into(),
                private_scope.clone(),
                None,
                serde_json::json!({ "text": "needle secret" }),
                Vec::new(),
                None,
            )
            .unwrap();

        let bob_results = store.search_messages("bob", "needle", None, 10);
        assert_eq!(bob_results.len(), 1);
        assert_eq!(bob_results[0].actor_id, "bob");

        let alice_results = store.search_messages("alice", "needle", Some(&private_scope), 10);
        assert_eq!(alice_results.len(), 1);
        assert_eq!(alice_results[0].actor_id, "alice");
    }

    #[test]
    fn delete_channel_succeeds_when_empty_and_replays() {
        let store = fresh_store();
        let ch = store.create_channel("disposable".into(), None).unwrap();
        let (deleted, threads) = store.delete_channel(&ch.id, false).unwrap();
        assert!(deleted);
        assert_eq!(threads, 0);
        assert!(store.get_channel(&ch.id).is_none());

        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store2 = Store::open(journal).unwrap();
        assert!(store2.get_channel(&ch.id).is_none());
    }

    #[test]
    fn delete_channel_with_cascade_removes_threads_and_channel() {
        let store = fresh_store();
        let ch = store.create_channel("doomed".into(), None).unwrap();
        let _t1 = create_thread_under(&store, &ch.id, "actor_owner", "t1");
        let _t2 = create_thread_under(&store, &ch.id, "actor_owner", "t2");

        let (deleted, threads) = store
            .delete_channel(&ch.id, true)
            .expect("cascade should succeed");
        assert!(deleted);
        assert_eq!(threads, 2);
        assert!(store.get_channel(&ch.id).is_none());
        assert!(store.list_threads(Some(&ch.id)).is_empty());

        // Replay: cascade write order must round-trip through the journal.
        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store2 = Store::open(journal).unwrap();
        assert!(store2.get_channel(&ch.id).is_none());
        assert!(store2.list_threads(Some(&ch.id)).is_empty());
    }

    #[test]
    fn delete_channel_with_cascade_on_empty_channel_is_noop_count_zero() {
        let store = fresh_store();
        let ch = store.create_channel("solo".into(), None).unwrap();
        let (deleted, threads) = store.delete_channel(&ch.id, true).unwrap();
        assert!(deleted);
        assert_eq!(threads, 0);
    }

    #[test]
    fn update_thread_renames_and_replays() {
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        let t = create_thread_under(&store, &ch.id, "actor_owner", "old");
        let updated = store.update_thread(&t.id, "fresh".into()).unwrap();
        assert_eq!(updated.title, "fresh");

        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store2 = Store::open(journal).unwrap();
        assert_eq!(store2.get_thread(&t.id).unwrap().title, "fresh");
    }

    #[test]
    fn delete_thread_removes_row_and_scope_index_and_replays() {
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        let t = create_thread_under(&store, &ch.id, "actor_owner", "doomed");
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: t.id.clone(),
        };

        assert!(store.delete_thread(&t.id).unwrap());
        assert!(store.get_thread(&t.id).is_none());
        // events_by_scope is private; re-check via list_threads.
        assert!(store
            .list_threads(Some(&ch.id))
            .iter()
            .all(|x| x.id != t.id));
        // Internal: index should be gone too.
        assert!(!store.inner.read().events_by_scope.contains_key(&scope));

        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store2 = Store::open(journal).unwrap();
        assert!(store2.get_thread(&t.id).is_none());
    }

    #[test]
    fn update_or_delete_missing_channel_returns_not_found() {
        let store = fresh_store();
        let err = store
            .update_channel("chan_missing", "x".into())
            .expect_err("must be NotFound");
        assert!(matches!(err, StoreError::NotFound(_)));
        let err = store
            .delete_channel("chan_missing", false)
            .expect_err("must be NotFound");
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    #[test]
    fn private_channel_seeds_creator_and_gates_others() {
        let store = fresh_store();
        let ch = store
            .create_channel("priv".into(), Some("actor_alice".into()))
            .unwrap();
        assert!(matches!(ch.visibility, ChannelVisibility::Private));
        assert_eq!(ch.members, vec!["actor_alice".to_string()]);
        assert!(store.is_channel_member(&ch.id, "actor_alice"));
        assert!(!store.is_channel_member(&ch.id, "actor_bob"));
    }

    #[test]
    fn public_channel_lets_anyone_in() {
        let store = fresh_store();
        let ch = store.create_channel("pub".into(), None).unwrap();
        assert!(matches!(ch.visibility, ChannelVisibility::Public));
        assert!(ch.members.is_empty());
        // Anyone — even an actor never seen — passes the gate.
        assert!(store.is_channel_member(&ch.id, "actor_random"));
    }

    #[test]
    fn grant_then_revoke_round_trips_through_replay() {
        let store = fresh_store();
        let ch = store
            .create_channel("priv".into(), Some("actor_alice".into()))
            .unwrap();
        store.grant_channel(&ch.id, "actor_bob").unwrap();
        assert!(store.is_channel_member(&ch.id, "actor_bob"));
        // Replay the journal in a fresh store; bob still in.
        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store2 = Store::open(journal).unwrap();
        assert!(store2.is_channel_member(&ch.id, "actor_bob"));

        store.revoke_channel(&ch.id, "actor_bob").unwrap();
        assert!(!store.is_channel_member(&ch.id, "actor_bob"));
        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store3 = Store::open(journal).unwrap();
        assert!(!store3.is_channel_member(&ch.id, "actor_bob"));
    }

    #[test]
    fn append_event_into_private_channel_rejects_non_member() {
        let store = fresh_store();
        let ch = store
            .create_channel("priv".into(), Some("actor_alice".into()))
            .unwrap();
        let t = create_thread_under(&store, &ch.id, "actor_alice", "t");
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: t.id.clone(),
        };
        let err = store
            .append_event(
                "content.add".into(),
                "actor_bob".into(),
                scope.clone(),
                None,
                serde_json::json!({"text": "hi"}),
                vec![],
                None,
            )
            .expect_err("non-member must be denied");
        assert!(matches!(err, StoreError::InvalidState(_)), "got {err:?}");

        // After grant, bob can post.
        store.grant_channel(&ch.id, "actor_bob").unwrap();
        store
            .append_event(
                "content.add".into(),
                "actor_bob".into(),
                scope,
                None,
                serde_json::json!({"text": "hi"}),
                vec![],
                None,
            )
            .expect("member may now append");
    }

    #[test]
    fn update_or_delete_missing_thread_returns_not_found() {
        let store = fresh_store();
        let err = store
            .update_thread("thread_missing", "x".into())
            .expect_err("must be NotFound");
        assert!(matches!(err, StoreError::NotFound(_)));
        let err = store
            .delete_thread("thread_missing")
            .expect_err("must be NotFound");
        assert!(matches!(err, StoreError::NotFound(_)));
    }

    /// Helper for RespondsTo tests below: append `kind` from `actor` into
    /// `scope` with the given relations. Returns the created event id.
    fn append_with_relations(
        store: &Arc<Store>,
        kind: &str,
        actor: &str,
        scope: ScopeRef,
        relations: Vec<Relation>,
    ) -> String {
        store
            .append_event(
                kind.into(),
                actor.into(),
                scope,
                None,
                serde_json::json!({"text": "hi"}),
                relations,
                None,
            )
            .expect("append event")
            .id
    }

    fn responds_to(event_id: &str) -> Relation {
        Relation {
            kind: RelationKind::RespondsTo,
            target: Ref {
                kind: RefKind::Event,
                id: event_id.into(),
                _meta: None,
            },
            _meta: None,
        }
    }

    #[test]
    fn responds_to_writes_pending_delivery_for_original_actor() {
        // svc_am_bridge appends a question, agent_qa replies with RespondsTo
        // -> question event. The reply event must produce a pending delivery
        // row for svc_am_bridge so a restarted service host can replay it via
        // the future delivery/list API.
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        store.grant_channel(&ch.id, "svc_am_bridge").unwrap();
        store.grant_channel(&ch.id, "agent_qa").unwrap();
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: ch.id.clone(),
        };

        let trigger_id = append_with_relations(
            &store,
            "content.add",
            "svc_am_bridge",
            scope.clone(),
            vec![],
        );
        let reply_id = append_with_relations(
            &store,
            "content.add",
            "agent_qa",
            scope,
            vec![responds_to(&trigger_id)],
        );

        let key = (reply_id.clone(), "svc_am_bridge".to_string());
        let delivery = store
            .inner
            .read()
            .deliveries
            .get(&key)
            .cloned()
            .expect("reverse delivery row exists");
        assert!(matches!(delivery.state, DeliveryState::Pending));
        assert_eq!(delivery.event_id, reply_id);
        assert_eq!(delivery.actor_id, "svc_am_bridge");
    }

    #[test]
    fn explicit_handoff_reply_does_not_reverse_deliver_to_original_actor() {
        // Reply-as-handoff events may carry both HandsOffTo and RespondsTo.
        // The explicit handoff target is the only actor that should wake up;
        // otherwise one user action can start two actor turns.
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        store.grant_channel(&ch.id, "actor_router").unwrap();
        store.grant_channel(&ch.id, "actor_delivery").unwrap();
        store.grant_channel(&ch.id, "actor_examiner").unwrap();
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: ch.id.clone(),
        };

        let original_id = append_with_relations(
            &store,
            "content.add",
            "actor_delivery",
            scope.clone(),
            vec![],
        );
        let handoff_id = append_with_relations(
            &store,
            "content.add",
            "actor_router",
            scope,
            vec![
                responds_to(&original_id),
                Relation {
                    kind: RelationKind::HandsOffTo,
                    target: Ref {
                        kind: RefKind::Actor,
                        id: "actor_examiner".into(),
                        _meta: None,
                    },
                    _meta: None,
                },
            ],
        );

        let deliveries = store.inner.read().deliveries.clone();
        assert!(
            deliveries.contains_key(&(handoff_id.clone(), "actor_examiner".to_string())),
            "explicit target must receive the delivery",
        );
        assert!(
            !deliveries.contains_key(&(handoff_id, "actor_delivery".to_string())),
            "responds_to target must not also receive the delivery",
        );
    }

    #[test]
    fn responds_to_skips_self_response() {
        // An actor responding to its own prior event should not generate a
        // self-deliver row — it would just be noise the speaker has to ack.
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        store.grant_channel(&ch.id, "agent_qa").unwrap();
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: ch.id.clone(),
        };

        let first_id =
            append_with_relations(&store, "content.add", "agent_qa", scope.clone(), vec![]);
        let second_id = append_with_relations(
            &store,
            "content.add",
            "agent_qa",
            scope,
            vec![responds_to(&first_id)],
        );

        let key = (second_id, "agent_qa".to_string());
        assert!(
            !store.inner.read().deliveries.contains_key(&key),
            "self-response must not write a delivery row",
        );
    }

    #[test]
    fn action_response_to_own_action_request_keeps_reverse_delivery() {
        // GUI misconfiguration can make the human workspace use the same actor
        // id as the agent runtime. For permission prompts that still needs to
        // route back to the long-lived daemon worker inbox.
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        store.grant_channel(&ch.id, "agent_qa").unwrap();
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: ch.id.clone(),
        };

        let request_id =
            append_with_relations(&store, "action.request", "agent_qa", scope.clone(), vec![]);
        let response_id = append_with_relations(
            &store,
            "action.response",
            "agent_qa",
            scope,
            vec![responds_to(&request_id)],
        );

        let key = (response_id, "agent_qa".to_string());
        assert!(
            store.inner.read().deliveries.contains_key(&key),
            "action.response to action.request must reverse-deliver even for same actor id",
        );
    }

    #[test]
    fn responds_to_replays_through_journal() {
        // Reverse-delivery rows must round-trip through the journal so that
        // restarting the server preserves the pending inbox for offline
        // service actors.
        let store = fresh_store();
        let ch = store.create_channel("c".into(), None).unwrap();
        store.grant_channel(&ch.id, "svc_am_bridge").unwrap();
        store.grant_channel(&ch.id, "agent_qa").unwrap();
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: ch.id.clone(),
        };

        let trigger_id = append_with_relations(
            &store,
            "content.add",
            "svc_am_bridge",
            scope.clone(),
            vec![],
        );
        let reply_id = append_with_relations(
            &store,
            "content.add",
            "agent_qa",
            scope,
            vec![responds_to(&trigger_id)],
        );

        let journal = Journal::open(store.journal.path().to_path_buf()).unwrap();
        let store2 = Store::open(journal).unwrap();
        let key = (reply_id, "svc_am_bridge".to_string());
        assert!(
            store2.inner.read().deliveries.contains_key(&key),
            "reverse delivery row must replay from journal",
        );
    }
}
