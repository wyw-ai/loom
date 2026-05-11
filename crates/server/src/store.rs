use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{Datelike, Duration as ChronoDuration, Utc, Weekday};
use parking_lot::RwLock;
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
    ChannelCreated(Channel),
    ArtifactPublished(Artifact),
    ReceiptRecorded(Receipt),
    DeliveryUpdated(Delivery),
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
            StoreEvent::ArtifactPublished(_) => None,
            StoreEvent::ReceiptRecorded(_) => None,
            StoreEvent::DeliveryUpdated(_) => None,
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
    turns: HashMap<String, Turn>,
    /// scope ref -> ordered events
    events_by_scope: HashMap<ScopeRef, Vec<String>>,
    events: HashMap<String, Event>,
    /// (turn_id) -> next seq
    turn_seq: HashMap<String, u64>,
    /// implicit turn used when an event/append arrives with no turn_id, keyed by (actor_id, scope)
    memberships: HashMap<(String, ScopeRef), Membership>,
    deliveries: HashMap<(String, String), Delivery>,
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
    broadcaster: broadcast::Sender<StoreEvent>,
}

impl Store {
    pub fn open(journal: Arc<Journal>) -> StoreResult<Arc<Self>> {
        let (tx, _) = broadcast::channel(1024);
        let store = Arc::new(Self {
            journal: journal.clone(),
            inner: RwLock::new(Inner::default()),
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
        let removed = self.inner.write().channels.remove(id).is_some();
        Ok((removed, deleted_threads))
    }

    // -------- Threads --------

    pub fn create_thread(
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
        Ok(t.clone())
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
        Ok(removed)
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
        let mut reverse_targets: Vec<String> = Vec::new();
        {
            let inner = self.inner.read();
            for r in &event.relations {
                if !matches!(r.kind, RelationKind::RespondsTo) || r.target.kind != RefKind::Event {
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

        self.emit(StoreEvent::EventCreated(event.clone()));

        if implicit_turn {
            let _ = self.close_turn(&assigned_turn_id, TurnStatus::Closed);
        }

        Ok(event)
    }

    pub fn get_event(&self, id: &str) -> Option<Event> {
        self.inner.read().events.get(id).cloned()
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
            inner
                .memberships
                .retain(|(member_actor_id, _), _| member_actor_id != &actor_id);
            inner
                .deliveries
                .retain(|(_, target_actor_id), _| target_actor_id != &actor_id);
            inner
                .receipts
                .retain(|(_, receipt_actor_id, _), _| receipt_actor_id != &actor_id);
        }
        Mutation::ChannelCreate(c) => {
            inner.channels.insert(c.id.clone(), c);
        }
        Mutation::ThreadCreate(t) => {
            inner.threads.insert(t.id.clone(), t);
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
        }
        Mutation::ThreadUpdate { thread_id, title } => {
            if let Some(t) = inner.threads.get_mut(&thread_id) {
                t.title = title;
            }
        }
        Mutation::ThreadDelete { thread_id } => {
            inner.threads.remove(&thread_id);
            let scope = ScopeRef {
                kind: ScopeKind::Thread,
                id: thread_id,
            };
            inner.events_by_scope.remove(&scope);
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
        // route back to the long-lived `joi agent serve` inbox.
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
