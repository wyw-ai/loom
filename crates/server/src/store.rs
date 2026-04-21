use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::Utc;
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
    ArtifactPublished(Artifact),
    ReceiptRecorded(Receipt),
    DeliveryUpdated(Delivery),
    HandoffCreated(Event),
    /// Turn-private trace frame. Carried on the same broadcast channel as
    /// scope events purely so the websocket layer can route it; the fanout
    /// must NOT broadcast it to scope subscribers — see `ws::fanout`.
    TraceAppended(TraceFrame),
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
            StoreEvent::HandoffCreated(e) => Some(e.scope.clone()),
            // Trace frames are owner-private; ws fanout routes them by
            // turn owner, never by scope.
            StoreEvent::TraceAppended(_) => None,
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

    pub fn get_actor(&self, id: &str) -> Option<Actor> {
        self.inner.read().actors.get(id).cloned()
    }

    pub fn list_actors(&self) -> Vec<Actor> {
        self.inner.read().actors.values().cloned().collect()
    }

    // -------- Channels --------

    pub fn create_channel(&self, title: String) -> StoreResult<Channel> {
        let channel = Channel {
            id: format!("chan_{}", short_id()),
            title,
            _meta: None,
        };
        self.journal
            .append(&Mutation::ChannelCreate(channel.clone()))?;
        self.inner
            .write()
            .channels
            .insert(channel.id.clone(), channel.clone());
        Ok(channel)
    }

    pub fn list_channels(&self) -> Vec<Channel> {
        self.inner.read().channels.values().cloned().collect()
    }

    pub fn get_channel(&self, id: &str) -> Option<Channel> {
        self.inner.read().channels.get(id).cloned()
    }

    // -------- Threads --------

    pub fn create_thread(
        &self,
        channel_id: String,
        title: String,
        root_event_id: Option<String>,
    ) -> StoreResult<Thread> {
        if self.get_channel(&channel_id).is_none() {
            return Err(StoreError::NotFound(format!("channel {channel_id}")));
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

    pub fn set_thread_root(
        &self,
        thread_id: String,
        root_event_id: String,
    ) -> StoreResult<()> {
        self.journal.append(&Mutation::ThreadRootSet {
            thread_id: thread_id.clone(),
            root_event_id: root_event_id.clone(),
        })?;
        let mut inner = self.inner.write();
        if let Some(t) = inner.threads.get_mut(&thread_id) {
            t.root_event_id = Some(root_event_id);
            Ok(())
        } else {
            Err(StoreError::NotFound(format!("thread {thread_id}")))
        }
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
        if self.inner.read().turns.get(turn_id).is_none() {
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
        if self.inner.read().turns.get(turn_id).is_none() {
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
            if matches!(r.kind, RelationKind::Targets | RelationKind::HandsOffTo)
                && r.target.kind == RefKind::Actor
            {
                let _ = self.touch_membership(r.target.id.clone(), scope.clone(), None);
            }
        }

        // Deliveries: explicit directed receivers only.
        for r in &event.relations {
            if matches!(r.kind, RelationKind::Targets | RelationKind::HandsOffTo)
                && r.target.kind == RefKind::Actor
            {
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

        // For threads whose root_event_id is unset, set it on the first
        // appended event if it has no replies_to.
        if let ScopeKind::Thread = event.scope.kind {
            let needs_root = matches!(
                self.get_thread(&event.scope.id),
                Some(t) if t.root_event_id.is_none()
            );
            let is_top_level = !event
                .relations
                .iter()
                .any(|r| matches!(r.kind, RelationKind::RepliesTo));
            if needs_root && is_top_level {
                let _ = self.set_thread_root(event.scope.id.clone(), event.id.clone());
            }
        }

        self.emit(StoreEvent::EventCreated(event.clone()));
        if event.kind == "handoff.offer" {
            self.emit(StoreEvent::HandoffCreated(event.clone()));
        }

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
        Mutation::ArtifactCreate(a) => {
            inner.artifacts.insert(a.id.clone(), a);
        }
        Mutation::ThreadRootSet {
            thread_id,
            root_event_id,
        } => {
            if let Some(t) = inner.threads.get_mut(&thread_id) {
                t.root_event_id = Some(root_event_id);
            }
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
    }
}

fn short_id() -> String {
    let id = Uuid::new_v4().simple().to_string();
    id[..12].to_string()
}

// silence unused-import lint while keeping BTreeMap available for future expansion
#[allow(dead_code)]
fn _meta_keep() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::new()
}
