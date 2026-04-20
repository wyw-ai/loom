use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::Utc;
use parking_lot::RwLock;
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
    ConversationCreated(Conversation),
    ArtifactPublished(Artifact),
    ReceiptRecorded(Receipt),
    DeliveryUpdated(Delivery),
    HandoffCreated(Event),
}

impl StoreEvent {
    pub fn scope(&self) -> Option<ScopeRef> {
        match self {
            StoreEvent::EventCreated(e) => Some(e.scope.clone()),
            StoreEvent::TurnOpened(t) | StoreEvent::TurnClosed(t) => Some(t.scope.clone()),
            StoreEvent::ConversationCreated(c) => Some(ScopeRef {
                kind: ScopeKind::Space,
                id: c.space_id.clone(),
            }),
            StoreEvent::ArtifactPublished(_) => None,
            StoreEvent::ReceiptRecorded(_) => None,
            StoreEvent::DeliveryUpdated(_) => None,
            StoreEvent::HandoffCreated(e) => Some(e.scope.clone()),
        }
    }
}

#[derive(Default)]
struct Inner {
    actors: HashMap<String, Actor>,
    spaces: HashMap<String, Space>,
    conversations: HashMap<String, Conversation>,
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

    // -------- Spaces --------

    pub fn create_space(&self, title: String) -> StoreResult<Space> {
        let space = Space {
            id: format!("space_{}", short_id()),
            title,
            _meta: None,
        };
        self.journal.append(&Mutation::SpaceCreate(space.clone()))?;
        self.inner
            .write()
            .spaces
            .insert(space.id.clone(), space.clone());
        Ok(space)
    }

    pub fn list_spaces(&self) -> Vec<Space> {
        self.inner.read().spaces.values().cloned().collect()
    }

    pub fn get_space(&self, id: &str) -> Option<Space> {
        self.inner.read().spaces.get(id).cloned()
    }

    // -------- Conversations --------

    pub fn create_conversation(
        &self,
        space_id: String,
        title: String,
        root_event_id: Option<String>,
    ) -> StoreResult<Conversation> {
        if self.get_space(&space_id).is_none() {
            return Err(StoreError::NotFound(format!("space {space_id}")));
        }
        let conv = Conversation {
            id: format!("conv_{}", short_id()),
            space_id,
            title,
            root_event_id,
            _meta: None,
        };
        self.journal
            .append(&Mutation::ConversationCreate(conv.clone()))?;
        self.inner
            .write()
            .conversations
            .insert(conv.id.clone(), conv.clone());
        self.emit(StoreEvent::ConversationCreated(conv.clone()));
        Ok(conv)
    }

    pub fn list_conversations(&self, space_id: Option<&str>) -> Vec<Conversation> {
        let inner = self.inner.read();
        inner
            .conversations
            .values()
            .filter(|c| match space_id {
                Some(id) => c.space_id == id,
                None => true,
            })
            .cloned()
            .collect()
    }

    pub fn get_conversation(&self, id: &str) -> Option<Conversation> {
        self.inner.read().conversations.get(id).cloned()
    }

    pub fn set_conversation_root(
        &self,
        conversation_id: String,
        root_event_id: String,
    ) -> StoreResult<()> {
        self.journal.append(&Mutation::ConversationRootSet {
            conversation_id: conversation_id.clone(),
            root_event_id: root_event_id.clone(),
        })?;
        let mut inner = self.inner.write();
        if let Some(c) = inner.conversations.get_mut(&conversation_id) {
            c.root_event_id = Some(root_event_id);
            Ok(())
        } else {
            Err(StoreError::NotFound(format!(
                "conversation {conversation_id}"
            )))
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
            ScopeKind::Space => {
                if self.get_space(&scope.id).is_none() {
                    return Err(StoreError::NotFound(format!("space {}", scope.id)));
                }
            }
            ScopeKind::Conversation => {
                if self.get_conversation(&scope.id).is_none() {
                    return Err(StoreError::NotFound(format!("conversation {}", scope.id)));
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

        // For conversations whose root_event_id is unset, set it on the first
        // appended event if it has no replies_to.
        if let ScopeKind::Conversation = event.scope.kind {
            let needs_root = matches!(
                self.get_conversation(&event.scope.id),
                Some(c) if c.root_event_id.is_none()
            );
            let is_top_level = !event
                .relations
                .iter()
                .any(|r| matches!(r.kind, RelationKind::RepliesTo));
            if needs_root && is_top_level {
                let _ = self.set_conversation_root(event.scope.id.clone(), event.id.clone());
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
        Mutation::SpaceCreate(s) => {
            inner.spaces.insert(s.id.clone(), s);
        }
        Mutation::ConversationCreate(c) => {
            inner.conversations.insert(c.id.clone(), c);
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
        Mutation::ConversationRootSet {
            conversation_id,
            root_event_id,
        } => {
            if let Some(c) = inner.conversations.get_mut(&conversation_id) {
                c.root_event_id = Some(root_event_id);
            }
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
