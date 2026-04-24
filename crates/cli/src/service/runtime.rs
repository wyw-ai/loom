// Public §6.3 surface; first consumer is the S2 am plugin. Silence
// dead-code warnings until S2 wires the first call site.
#![allow(dead_code)]

//! `ServiceRuntime` is the §6.3 substrate plugins call into. Wraps a
//! single WS connection bound to one service actor and offers the
//! protocol primitives plugins need: actor upsert, channel-member
//! ensure, content append + handoff sugar, dedupe, cursor, state-dir,
//! and the §9.5/§9.2 await-responds-to drainer.
//!
//! Single-actor by construction: the host opens one connection per
//! `ServiceSpec` and creates one `ServiceRuntime` over it, so the
//! `actor_id` field can be stamped onto every event without per-call
//! plumbing. Plugins that need to act as multiple actors (rare, would
//! need rethinking — see §6.1) ask the host for additional runtimes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use proto::methods::{
    method, ActorUpsertParams, ActorUpsertResult, DeliveryListParams, DeliveryListResult,
    EventAppendInput, EventAppendParams, EventAppendResult, ThreadCreateParams, ThreadCreateResult,
};
use proto::types::{
    Actor, DeliveryState, Event, Meta, Ref, RefKind, Relation, RelationKind, ScopeRef, Thread,
};
use serde_json::json;

use crate::client::Client;

use super::state::{self, DedupeStore};

pub struct ServiceRuntime {
    service_id: String,
    actor_id: String,
    client: Arc<Client>,
    state_dir: PathBuf,
    dedupe: DedupeStore,
}

impl ServiceRuntime {
    /// Spin up a runtime bound to `actor_id`, with state dir rooted under
    /// `data_root`. The caller is responsible for opening the WS
    /// connection and binding it to `actor_id` (via `connection/open`)
    /// before reaching here — this is the host's job, not the runtime's.
    pub fn start(
        service_id: String,
        actor_id: String,
        client: Arc<Client>,
        data_root: &Path,
    ) -> Result<Arc<Self>> {
        let state_dir = state::ensure_state_dir(data_root, &service_id)?;
        let dedupe = DedupeStore::open(&state_dir)?;
        Ok(Arc::new(Self {
            service_id,
            actor_id,
            client,
            state_dir,
            dedupe,
        }))
    }

    pub fn service_id(&self) -> &str {
        &self.service_id
    }

    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// §8.4 dedupe primitive. Returns `Ok(true)` if `key` is new (caller
    /// must process the source), `Ok(false)` if the key was previously
    /// recorded (caller must skip). MUST be invoked **before** the
    /// matching `append_content`/`handoff` so a process crash between
    /// dedup and append does not double-emit on restart.
    pub fn dedupe_once(&self, key: &str) -> Result<bool> {
        self.dedupe.record(key)
    }

    pub fn cursor_load(&self, name: &str) -> Result<Option<String>> {
        state::cursor_load(&self.state_dir, name)
    }

    pub fn cursor_save(&self, name: &str, value: &str) -> Result<()> {
        state::cursor_save(&self.state_dir, name, value)
    }

    /// Idempotent actor row upsert. The host calls this once at startup
    /// so the service actor's display name and capabilities reach the
    /// server even before the first event references the actor.
    pub async fn actor_upsert(&self, actor: Actor) -> Result<Actor> {
        let res: ActorUpsertResult = self
            .client
            .call(method::ACTOR_UPSERT, ActorUpsertParams { actor })
            .await
            .context("actor/upsert")?;
        Ok(res.actor)
    }

    /// Append a `content.add` event into `scope` as `text/markdown`,
    /// stamped with the runtime's `actor_id`. Returns the new event id.
    pub async fn append_content(
        &self,
        scope: ScopeRef,
        text: impl Into<String>,
        relations: Vec<Relation>,
        meta: Option<Meta>,
    ) -> Result<String> {
        let event = EventAppendInput {
            kind: "content.add".into(),
            actor_id: self.actor_id.clone(),
            scope,
            turn_id: None,
            payload: json!({ "contentType": "text/markdown", "text": text.into() }),
            relations,
            _meta: meta,
        };
        let res: EventAppendResult = self
            .client
            .call(method::EVENT_APPEND, EventAppendParams { event })
            .await
            .context("event/append")?;
        Ok(res.event.id)
    }

    /// Sugar over [`Self::append_content`] that adds a single
    /// `hands_off_to -> actor:<target>` relation. The most common shape
    /// of plugin output (see §7.3 for am, §8.3 for scheduler).
    pub async fn handoff(
        &self,
        target_actor: &str,
        scope: ScopeRef,
        text: impl Into<String>,
        meta: Option<Meta>,
    ) -> Result<String> {
        let relations = vec![Relation {
            kind: RelationKind::HandsOffTo,
            target: Ref {
                kind: RefKind::Actor,
                id: target_actor.into(),
                _meta: None,
            },
            _meta: None,
        }];
        self.append_content(scope, text, relations, meta).await
    }

    /// Make sure this runtime's actor is a member of `channel_id`.
    /// Idempotent on the server (re-inviting an existing member is a
    /// no-op). Returns Err only on transport failure or when the caller
    /// lacks permission to invite into the channel.
    pub async fn ensure_channel_member(&self, channel_id: &str) -> Result<()> {
        self.invite_member(channel_id, &self.actor_id).await
    }

    /// Invite an arbitrary `actor_id` into `channel_id`. Generalization
    /// of [`Self::ensure_channel_member`] so plugins can pull a target
    /// agent into the same channel as the service actor (e.g., AM's
    /// `auto_invite` mode for the configured `targetAgent`).
    pub async fn invite_member(&self, channel_id: &str, actor_id: &str) -> Result<()> {
        self.client
            .call_raw(
                method::CHANNEL_INVITE,
                Some(json!({
                    "channelId": channel_id,
                    "actorId": actor_id,
                })),
            )
            .await
            .with_context(|| format!("channel/invite {actor_id} into {channel_id}"))?;
        Ok(())
    }

    /// Create a fresh thread under `channel_id` titled `title` (no root
    /// event). Returns the new thread row. The "or-get" half of §6.3's
    /// `create_or_get_thread` lives in plugin-specific thread-map state
    /// (e.g., `service::am::scope`) — the runtime exposes only the
    /// stateless server-side primitive.
    pub async fn create_thread(&self, channel_id: &str, title: &str) -> Result<Thread> {
        let res: ThreadCreateResult = self
            .client
            .call(
                method::THREAD_CREATE,
                ThreadCreateParams {
                    channel_id: channel_id.into(),
                    title: title.into(),
                    root_event_id: None,
                },
            )
            .await
            .with_context(|| format!("thread/create in {channel_id}"))?;
        Ok(res.thread)
    }

    /// Drain pending deliveries off this actor's inbox and return events
    /// whose relations carry `responds_to -> trigger_event_id`. Polls
    /// `delivery/list` (§9.2) every 500ms until either a match arrives or
    /// `timeout` elapses.
    ///
    /// **S1 limitation**: pure polling, no live `stream/update`
    /// fan-in. The §9.5 push path exists on the server but requires a
    /// notification consumer the host doesn't yet wire; S2 plugin work
    /// will replace this loop with the hybrid drain-then-watch design.
    /// For S1 callers (none yet — the first one is the S2 am rewrite)
    /// the polling latency is acceptable.
    ///
    /// **Receipts**: this method does not record receipts on returned
    /// events. The caller decides when an event is safely processed and
    /// calls `receipt/record` itself. Without that, the same event would
    /// reappear on the next poll — that's the desired safety, not a bug.
    pub async fn await_responds_to(
        &self,
        trigger_event_id: &str,
        timeout: Duration,
    ) -> Result<Vec<Event>> {
        let deadline = Instant::now() + timeout;
        let poll_interval = Duration::from_millis(500);
        loop {
            let mut found = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let params = DeliveryListParams {
                    actor_id: self.actor_id.clone(),
                    state: Some(DeliveryState::Pending),
                    limit: Some(50),
                    cursor: cursor.clone(),
                };
                let res: DeliveryListResult = self
                    .client
                    .call(method::DELIVERY_LIST, params)
                    .await
                    .context("delivery/list")?;
                for entry in res.deliveries {
                    let Some(event) = entry.event else { continue };
                    let matches = event.relations.iter().any(|r| {
                        matches!(r.kind, RelationKind::RespondsTo)
                            && r.target.kind == RefKind::Event
                            && r.target.id == trigger_event_id
                    });
                    if matches {
                        found.push(event);
                    }
                }
                cursor = res.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            if !found.is_empty() {
                return Ok(found);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(Vec::new());
            }
            let sleep = (deadline - now).min(poll_interval);
            tokio::time::sleep(sleep).await;
        }
    }
}
