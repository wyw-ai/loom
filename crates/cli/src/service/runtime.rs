// Public §6.3 surface; first consumer is the S2 am plugin. Silence
// dead-code warnings until S2 wires the first call site.
#![allow(dead_code)]

//! `ServiceRuntime` is the §6.3 substrate plugins call into. Wraps a
//! single WS connection bound to one service actor and offers the
//! protocol primitives plugins need: actor upsert, channel-member
//! ensure, message send, content append, dedupe, cursor, state-dir,
//! and the §9.5/§9.2 reply drainer.
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
    method, ActorUpsertParams, ActorUpsertResult, ArtifactIngress, ArtifactPublishParams,
    ArtifactPublishResult, InboxListParams, InboxListResult, InlineTextIngress, MessageSendResult,
    TaskFactAppendResult, ThreadCreateParams, ThreadCreateResult, ThreadListParams,
    ThreadListResult,
};
use proto::types::{Actor, DeliveryState, Message, Meta, Relation, ScopeKind, ScopeRef, Thread};
use serde_json::json;

use crate::client::Client;

use super::state::{self, DedupeStore};

pub struct ServiceRuntime {
    service_id: String,
    actor_id: String,
    instance_id: Option<String>,
    client: Arc<Client>,
    service_state_dir: PathBuf,
    state_dir: PathBuf,
    dedupe: DedupeStore,
}

fn service_payload_body(kind: &str, payload: &serde_json::Value) -> String {
    if let Some(text) = payload.get("text").and_then(serde_json::Value::as_str) {
        return text.to_string();
    }
    let rendered = serde_json::to_string_pretty(payload)
        .unwrap_or_else(|_| serde_json::to_string(payload).unwrap_or_default());
    if rendered.is_empty() {
        kind.to_string()
    } else {
        format!("{kind}\n\n```json\n{rendered}\n```")
    }
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
            instance_id: None,
            client,
            service_state_dir: state_dir.clone(),
            state_dir,
            dedupe,
        }))
    }

    /// Variant of [`Self::start`] that scopes state to a single instance
    /// (`<data_root>/services/<service_id>/instances/<instance_id>/`).
    /// Used by `lifecycle = thread_bound` services so multiple instances
    /// of the same spec can coexist with disjoint cursor/dedupe state.
    pub fn start_instance(
        service_id: String,
        actor_id: String,
        instance_id: String,
        client: Arc<Client>,
        data_root: &Path,
    ) -> Result<Arc<Self>> {
        let state_dir = state::ensure_instance_state_dir(data_root, &service_id, &instance_id)?;
        let service_state_dir = state::state_dir(data_root, &service_id);
        let dedupe = DedupeStore::open(&state_dir)?;
        Ok(Arc::new(Self {
            service_id,
            actor_id,
            instance_id: Some(instance_id),
            client,
            service_state_dir,
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

    /// `Some(thread_id)` for thread-bound instances, `None` for the
    /// channel-level singleton. Plugins that need to interpolate the
    /// bound scope (e.g., scheduler's `{thread.id}` placeholder) read
    /// this.
    pub fn instance_id(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// State shared by every instance of this service. For a singleton this
    /// is the same directory as [`Self::state_dir`]; for a thread-bound
    /// runtime it is the parent `<data_root>/services/<service_id>/` directory.
    pub fn service_state_dir(&self) -> &Path {
        &self.service_state_dir
    }

    /// §8.4 dedupe primitive. Returns `Ok(true)` if `key` is new (caller
    /// must process the source), `Ok(false)` if the key was previously
    /// recorded (caller must skip). MUST be invoked **before** the
    /// matching append/send so a process crash between
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
    /// server even before the first message references the actor.
    pub async fn actor_upsert(&self, actor: Actor) -> Result<Actor> {
        let res: ActorUpsertResult = self
            .client
            .call(method::ACTOR_UPSERT, ActorUpsertParams { actor })
            .await
            .context("actor/upsert")?;
        Ok(res.actor)
    }

    /// Send a `content.add` message into `scope` as `text/markdown`,
    /// stamped with the runtime's `actor_id`. Returns the new message id.
    pub async fn append_content(
        &self,
        scope: ScopeRef,
        text: impl Into<String>,
        relations: Vec<Relation>,
        meta: Option<Meta>,
    ) -> Result<String> {
        let target = self.message_target_for_scope(&scope).await?;
        let mut metadata = meta.unwrap_or_default();
        metadata.insert("kind".into(), json!("content.add"));
        metadata.insert("contentType".into(), json!("text/markdown"));
        if !relations.is_empty() {
            metadata.insert("relations".into(), serde_json::to_value(relations)?);
        }
        let res: MessageSendResult = self
            .client
            .call(
                method::MESSAGE_SEND,
                json!({
                    "target": target,
                    "body": text.into(),
                    "intent": "chat",
                    "deliveryPolicy": "notify_only",
                    "metadata": metadata,
                }),
            )
            .await
            .context("message.send content")?;
        Ok(res.message.id)
    }

    /// Send a directed Loom message that wakes `target_actor`.
    pub async fn send_directed_message(
        &self,
        target_actor: &str,
        scope: ScopeRef,
        text: impl Into<String>,
        meta: Option<Meta>,
    ) -> Result<String> {
        let target = self.message_target_for_scope(&scope).await?;
        let res: MessageSendResult = self
            .client
            .call(
                method::MESSAGE_SEND,
                json!({
                    "target": target,
                    "body": text.into(),
                    "audience": [{
                        "kind": "actor",
                        "id": target_actor,
                    }],
                    "intent": "request_action",
                    "deliveryPolicy": "wake_agent",
                    "metadata": meta.unwrap_or_default(),
                }),
            )
            .await
            .with_context(|| format!("message.send directed to {target_actor}"))?;
        Ok(res.message.id)
    }

    /// Publish an artifact whose body is rendered verbatim from `text`
    /// under `name`. `media_type` defaults to `application/json` to match
    /// the §6 cross-agent contract artifacts (caller may override).
    /// Returns `(artifact_id, artifact_uri)`.
    pub async fn publish_artifact(
        &self,
        scope: ScopeRef,
        name: impl Into<String>,
        media_type: Option<String>,
        text: impl Into<String>,
    ) -> Result<(String, String)> {
        let params = ArtifactPublishParams {
            ingress: ArtifactIngress::InlineText(InlineTextIngress {
                name: name.into(),
                media_type: media_type.unwrap_or_else(|| "application/json".into()),
                text: text.into(),
            }),
            created_by: self.actor_id.clone(),
            scope: Some(scope),
        };
        let res: ArtifactPublishResult = self
            .client
            .call(method::ARTIFACT_PUBLISH, params)
            .await
            .context("artifact/publish")?;
        Ok((res.artifact.id, res.artifact.uri))
    }

    /// Send an arbitrary-kind status message whose payload is the given JSON
    /// value, with an optional artifact attachment.
    pub async fn append_status(
        &self,
        scope: ScopeRef,
        kind: impl Into<String>,
        payload: serde_json::Value,
        artifact_id: Option<&str>,
        meta: Option<Meta>,
    ) -> Result<String> {
        let target = self.message_target_for_scope(&scope).await?;
        let kind = kind.into();
        let body = service_payload_body(&kind, &payload);
        let mut metadata = meta.unwrap_or_default();
        metadata.insert("kind".into(), json!(kind));
        metadata.insert("payload".into(), payload);
        let attachments = artifact_id
            .map(|id| vec![id.to_string()])
            .unwrap_or_default();
        let res: MessageSendResult = self
            .client
            .call(
                method::MESSAGE_SEND,
                json!({
                    "target": target,
                    "body": body,
                    "intent": "notify",
                    "deliveryPolicy": "notify_only",
                    "attachments": attachments,
                    "metadata": metadata,
                }),
            )
            .await
            .context("message.send status.update")?;
        Ok(res.message.id)
    }

    /// Record service output as a durable task fact. Scheduler jobs use
    /// this when their JSON payload carries both `schema` and `taskId`.
    pub async fn append_task_fact(
        &self,
        task_id: &str,
        kind: &str,
        payload: serde_json::Value,
        artifact_id: Option<&str>,
        summary: impl Into<String>,
        source_cursor: Option<&str>,
    ) -> Result<String> {
        let res: TaskFactAppendResult = self
            .client
            .call(
                method::TASK_FACT_APPEND,
                json!({
                    "taskId": task_id,
                    "targetKey": payload.get("mrId")
                        .or_else(|| payload.get("workitemId"))
                        .and_then(|v| {
                            v.as_str()
                                .map(ToString::to_string)
                                .or_else(|| v.as_i64().map(|n| n.to_string()))
                                .or_else(|| v.as_u64().map(|n| n.to_string()))
                        })
                        .unwrap_or_default(),
                    "kind": kind,
                    "factType": "status",
                    "status": "active",
                    "producerId": &self.actor_id,
                    "summary": summary.into(),
                    "artifactId": artifact_id,
                    "payloadSchema": kind,
                    "payload": payload,
                    "sourceCursor": source_cursor,
                }),
            )
            .await
            .context("task/fact.append")?;
        Ok(res.fact.id)
    }

    /// Publish a `service.self_complete` message into `scope` carrying the
    /// originating service id and a free-form `reason`. Used by service
    /// plugins (e.g., scheduler running a thread-bound bundle) when the
    /// underlying source signals "this instance is done — auto_stop_on
    /// owners should tear me down". The message is informational; the
    /// actual stop decision belongs to whoever observes `auto_stop_on`.
    pub async fn publish_self_complete(
        &self,
        scope: ScopeRef,
        reason: impl Into<String>,
    ) -> Result<String> {
        let payload = json!({
            "service_id": self.service_id,
            "instance_id": self.instance_id,
            "reason": reason.into(),
        });
        let target = self.message_target_for_scope(&scope).await?;
        let mut metadata = Meta::default();
        metadata.insert("kind".into(), json!("service.self_complete"));
        metadata.insert("payload".into(), payload.clone());
        let res: MessageSendResult = self
            .client
            .call(
                method::MESSAGE_SEND,
                json!({
                    "target": target,
                    "body": service_payload_body("service.self_complete", &payload),
                    "intent": "notify",
                    "deliveryPolicy": "notify_only",
                    "metadata": metadata,
                }),
            )
            .await
            .context("message.send service.self_complete")?;
        Ok(res.message.id)
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

    /// Create a fresh thread under `channel_id`, rooted at a channel message.
    /// Returns the new thread row. The "or-get" half of §6.3's
    /// `create_or_get_thread` lives in plugin-specific thread-map state
    /// (e.g., `service::am::scope`) — the runtime exposes only the
    /// stateless server-side primitive.
    pub async fn create_thread(
        &self,
        channel_id: &str,
        root_message_id: &str,
        title: &str,
    ) -> Result<Thread> {
        let res: ThreadCreateResult = self
            .client
            .call(
                method::THREAD_CREATE,
                ThreadCreateParams {
                    channel_id: channel_id.into(),
                    title: title.into(),
                    root_message_id: root_message_id.into(),
                },
            )
            .await
            .with_context(|| format!("thread/create in {channel_id}"))?;
        Ok(res.thread)
    }

    pub async fn message_target_for_scope(&self, scope: &ScopeRef) -> Result<String> {
        match scope.kind {
            ScopeKind::Channel => Ok(format!("#{}", scope.id)),
            ScopeKind::Thread => {
                let res: ThreadListResult = self
                    .client
                    .call(
                        method::THREAD_LIST,
                        ThreadListParams {
                            channel_id: None,
                            archived: false,
                        },
                    )
                    .await
                    .context("thread/list")?;
                let thread = res
                    .threads
                    .into_iter()
                    .find(|thread| thread.id == scope.id)
                    .ok_or_else(|| anyhow::anyhow!("thread {} not found", scope.id))?;
                Ok(format!("#{}:{}", thread.channel_id, thread.root_message_id))
            }
        }
    }

    /// Append a channel-root message as this service actor and return its id.
    pub async fn append_channel_message(
        &self,
        channel_id: &str,
        text: impl Into<String>,
    ) -> Result<String> {
        let res: MessageSendResult = self
            .client
            .call(
                method::MESSAGE_SEND,
                json!({
                    "target": format!("#{channel_id}"),
                    "body": text.into(),
                    "intent": "chat",
                    "deliveryPolicy": "notify_only",
                }),
            )
            .await
            .with_context(|| format!("message.send in {channel_id}"))?;
        Ok(res.message.id)
    }

    pub async fn await_message_replies(
        &self,
        parent_message_id: &str,
        timeout: Duration,
    ) -> Result<Vec<Message>> {
        let deadline = Instant::now() + timeout;
        let poll_interval = Duration::from_millis(500);
        loop {
            let mut found = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let params = InboxListParams {
                    actor_id: self.actor_id.clone(),
                    state: Some(DeliveryState::Pending),
                    limit: Some(50),
                    cursor: cursor.clone(),
                };
                let res: InboxListResult = self
                    .client
                    .call(method::INBOX_LIST, params)
                    .await
                    .context("inbox.list")?;
                for entry in res.deliveries {
                    let Some(message) = entry.message else {
                        continue;
                    };
                    if message.parent_message_id.as_deref() == Some(parent_message_id) {
                        found.push(message);
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
