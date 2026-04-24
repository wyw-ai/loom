use std::sync::Arc;

use chrono::Utc;
use proto::methods::*;
use proto::types::*;
use proto::{ErrorCode, ErrorObject};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::runtime::wakeup;
use crate::state::AppState;
use crate::store::StoreError;

pub type HandlerResult = Result<Value, ErrorObject>;

fn map_store_err(err: StoreError) -> ErrorObject {
    match err {
        StoreError::NotFound(m) => ErrorObject::new(ErrorCode::APP_NOT_FOUND, m),
        StoreError::Conflict(m) => ErrorObject::new(ErrorCode::APP_CONFLICT, m),
        StoreError::InvalidState(m) => ErrorObject::new(ErrorCode::APP_INVALID_STATE, m),
        StoreError::Io(e) => ErrorObject::new(ErrorCode::APP_RUNTIME_ERROR, e.to_string()),
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, ErrorObject> {
    let v = params.unwrap_or(Value::Null);
    serde_json::from_value(v)
        .map_err(|e| ErrorObject::new(ErrorCode::INVALID_PARAMS, e.to_string()))
}

fn ok<T: serde::Serialize>(value: T) -> HandlerResult {
    serde_json::to_value(value)
        .map_err(|e| ErrorObject::new(ErrorCode::INTERNAL_ERROR, e.to_string()))
}

pub async fn dispatch(
    state: &AppState,
    connection_id: &str,
    method: &str,
    params: Option<Value>,
) -> HandlerResult {
    match method {
        method::INITIALIZE => initialize(params),
        method::CONNECTION_OPEN => connection_open(state, connection_id, params),
        method::CONNECTION_CLOSE => connection_close(state, params),
        method::SCOPE_SUBSCRIBE => scope_subscribe(state, connection_id, params),
        method::SCOPE_UNSUBSCRIBE => scope_unsubscribe(state, connection_id, params),
        method::SCOPE_READ => scope_read(state, connection_id, params),
        method::CHANNEL_CREATE => channel_create(state, connection_id, params),
        method::CHANNEL_LIST => channel_list(state, connection_id),
        method::CHANNEL_UPDATE => channel_update(state, params),
        method::CHANNEL_DELETE => channel_delete(state, params),
        method::CHANNEL_INVITE => channel_invite(state, connection_id, params),
        method::CHANNEL_REVOKE => channel_revoke(state, connection_id, params),
        method::CHANNEL_MEMBERS => channel_members(state, connection_id, params),
        method::THREAD_CREATE => thread_create(state, params),
        method::THREAD_LIST => thread_list(state, params),
        method::THREAD_UPDATE => thread_update(state, params),
        method::THREAD_DELETE => thread_delete(state, params),
        method::TURN_OPEN => turn_open(state, params),
        method::TURN_CLOSE => turn_close(state, connection_id, params).await,
        method::TURN_TRACE_READ => turn_trace_read(state, connection_id, params),
        method::TURN_TRACE_APPEND => turn_trace_append(state, connection_id, params),
        method::EVENT_APPEND => event_append(state, params).await,
        method::ARTIFACT_PUBLISH => artifact_publish(state, params),
        method::ARTIFACT_GET => artifact_get(state, params),
        method::ARTIFACT_READ => artifact_read(state, params),
        method::RECEIPT_RECORD => receipt_record(state, params),
        method::ACTOR_LIST => actor_list(state),
        method::ACTOR_UPSERT => actor_upsert(state, params),
        method::AGENT_LIST => agent_list(state),
        method::AGENT_REGISTER => agent_register(state, params),
        method::AGENT_UNREGISTER => agent_unregister(state, params),
        method::AGENT_START => agent_start(state, params).await,
        method::AGENT_STOP => agent_stop(state, params).await,
        method::AGENT_LOG => agent_log(state, params),
        method::AGENT_LIST_MARKETPLACE => agent_list_marketplace(),
        method::AGENT_INSTALL => agent_install(state, params),
        other => Err(ErrorObject::new(
            ErrorCode::METHOD_NOT_FOUND,
            format!("unknown method `{}`", other),
        )),
    }
}

// ---- initialize ----

fn initialize(params: Option<Value>) -> HandlerResult {
    let _: InitializeParams = parse_params(params.clone()).unwrap_or_default();
    let res = InitializeResult {
        protocol_version: proto::PROTOCOL_VERSION.into(),
        server_info: ServerInfo {
            name: proto::SERVER_NAME.into(),
            title: proto::SERVER_TITLE.into(),
            version: proto::SERVER_VERSION.into(),
        },
        server_capabilities: json!({
            "scopes": ["channel", "thread"],
            "extensions": ["agent"],
        }),
    };
    ok(res)
}

// ---- connection ----

fn connection_open(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ConnectionOpenParams = parse_params(params)?;
    let actor_id = p.actor_id.clone();
    let claim_kind = p.actor_kind.unwrap_or(ActorKind::Human);
    let actor = match state.store.get_actor(&actor_id) {
        Some(existing) => existing,
        None => {
            let new_actor = Actor {
                id: actor_id.clone(),
                kind: claim_kind,
                display_name: p.display_name.unwrap_or_else(|| actor_id.clone()),
                capabilities: None,
                _meta: None,
            };
            state.store.upsert_actor(new_actor).map_err(map_store_err)?
        }
    };
    state
        .subscriptions
        .bind_actor(connection_id, actor_id.clone(), claim_kind);
    let endpoint_id = format!(
        "ep_{}",
        p.endpoint
            .as_ref()
            .map(|e| e.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string()[..12].into())
    );
    let connection = Connection_ {
        id: connection_id.into(),
        actor_id,
        endpoint_id,
        opened_at: Utc::now(),
        _meta: None,
    };
    ok(ConnectionOpenResult {
        connection: connection.into(),
        actor,
    })
}

// Helper to mirror proto::Connection (avoid name clash with subscribe::Connection)
struct Connection_ {
    id: String,
    actor_id: String,
    endpoint_id: String,
    opened_at: chrono::DateTime<chrono::Utc>,
    _meta: Option<Meta>,
}

impl From<Connection_> for proto::types::Connection {
    fn from(c: Connection_) -> Self {
        Self {
            id: c.id,
            actor_id: c.actor_id,
            endpoint_id: c.endpoint_id,
            opened_at: c.opened_at,
            _meta: c._meta,
        }
    }
}

fn connection_close(_state: &AppState, params: Option<Value>) -> HandlerResult {
    let _p: ConnectionCloseParams = parse_params(params)?;
    ok(ConnectionCloseResult { closed: true })
}

// ---- scope ----

fn scope_subscribe(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ScopeOnlyParams = parse_params(params)?;
    let actor_id = caller_actor(state, connection_id)?;
    // ACL gate: refuse subscriptions to private channels the caller isn't
    // a member of, so a non-member can't even passively watch.
    state
        .store
        .check_scope_access(&p.scope, &actor_id)
        .map_err(map_store_err)?;
    if !state
        .subscriptions
        .subscribe(connection_id, p.scope.clone())
    {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "connection not registered",
        ));
    }
    ok(ScopeSubscribeResult {
        mode: "live".into(),
        created_at: Utc::now(),
        actor_id,
        scope: p.scope,
    })
}

fn scope_unsubscribe(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ScopeOnlyParams = parse_params(params)?;
    let removed = state.subscriptions.unsubscribe(connection_id, &p.scope);
    ok(ScopeUnsubscribeResult {
        unsubscribed: removed,
    })
}

fn scope_read(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ScopeReadParams = parse_params(params)?;
    let actor_id = caller_actor(state, connection_id)?;
    state
        .store
        .check_scope_access(&p.scope, &actor_id)
        .map_err(map_store_err)?;
    let (events, has_more) =
        state
            .store
            .read_scope(&p.scope, p.limit, p.before_event_id.as_deref());
    ok(ScopeReadResult {
        events,
        page_info: PageInfo {
            has_more,
            next_cursor: None,
            _meta: None,
        },
    })
}

/// Look up the actor id bound to this connection. All ACL gates rely on
/// the connection's bound actor (set by `connection/open`) and treat an
/// unbound connection as "no actor identity to authorize" — a 4xx-style
/// app error rather than a server panic.
fn caller_actor(state: &AppState, connection_id: &str) -> Result<String, ErrorObject> {
    state
        .subscriptions
        .actor_for_connection(connection_id)
        .ok_or_else(|| {
            ErrorObject::new(
                ErrorCode::APP_INVALID_STATE,
                "connection has no bound actor; call connection/open first",
            )
        })
}

// ---- channel ----

fn channel_create(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ChannelCreateParams = parse_params(params)?;
    // Prefer an explicit `actorId` from the params (so a tool can create
    // a private channel on behalf of the operator); fall back to the
    // connection's bound actor. Only when both are absent (legacy v0
    // callers) do we fall through to a Public channel.
    let creator = match p.actor_id {
        Some(id) => Some(id),
        None => state.subscriptions.actor_for_connection(connection_id),
    };
    let channel = state
        .store
        .create_channel(p.title, creator)
        .map_err(map_store_err)?;
    ok(ChannelCreateResult { channel })
}

fn channel_list(state: &AppState, connection_id: &str) -> HandlerResult {
    let caller = state.subscriptions.actor_for_connection(connection_id);
    let channels = state
        .store
        .list_channels()
        .into_iter()
        .filter(|c| match c.visibility {
            // Public channels are always listable.
            ChannelVisibility::Public => true,
            // Private channels: only members see them. Unbound callers
            // (no actor) see no private channels — better to omit than
            // to leak titles to a stranger holding a websocket.
            ChannelVisibility::Private => match caller.as_deref() {
                Some(actor) => c.members.iter().any(|m| m == actor),
                None => false,
            },
        })
        .collect();
    ok(ChannelListResult { channels })
}

fn channel_invite(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ChannelInviteParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    // Only existing members can invite. Public channels make this
    // vacuously true (everyone is implicitly a member).
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot invite to channel {}", p.channel_id),
        ));
    }
    let channel = state
        .store
        .grant_channel(&p.channel_id, &p.actor_id)
        .map_err(map_store_err)?;
    ok(ChannelInviteResult { channel })
}

fn channel_revoke(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ChannelRevokeParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    // Same access rule as invite: must be an existing member.
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot revoke from channel {}", p.channel_id),
        ));
    }
    // Guardrail: refuse to remove the channel's first/creator member when
    // there are other members. Without this, a clueless invitee could
    // orphan everyone else from the channel they were welcomed into.
    if let Some(ch) = state.store.get_channel(&p.channel_id) {
        if matches!(ch.visibility, ChannelVisibility::Private)
            && ch.members.first().map(|s| s.as_str()) == Some(p.actor_id.as_str())
            && ch.members.len() > 1
        {
            return Err(ErrorObject::new(
                ErrorCode::APP_INVALID_STATE,
                "cannot revoke the channel creator while other members exist",
            ));
        }
    }
    let channel = state
        .store
        .revoke_channel(&p.channel_id, &p.actor_id)
        .map_err(map_store_err)?;
    ok(ChannelRevokeResult { channel })
}

fn channel_members(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ChannelMembersParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let ch = state
        .store
        .get_channel(&p.channel_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "channel"))?;
    // Reading the member list is itself member-only on private channels —
    // otherwise anyone could enumerate who's in a private channel.
    if matches!(ch.visibility, ChannelVisibility::Private)
        && !ch.members.iter().any(|m| m == &caller)
    {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "actor is not a member of this channel",
        ));
    }
    let members = ch
        .members
        .iter()
        .filter_map(|id| state.store.get_actor(id))
        .collect();
    ok(ChannelMembersResult { members })
}

fn channel_update(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ChannelUpdateParams = parse_params(params)?;
    let channel = state
        .store
        .update_channel(&p.channel_id, p.title)
        .map_err(map_store_err)?;
    ok(ChannelUpdateResult { channel })
}

fn channel_delete(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ChannelDeleteParams = parse_params(params)?;
    let deleted = state
        .store
        .delete_channel(&p.channel_id)
        .map_err(map_store_err)?;
    ok(ChannelDeleteResult { deleted })
}

// ---- thread ----

fn thread_create(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ThreadCreateParams = parse_params(params)?;
    let thread = state
        .store
        .create_thread(p.channel_id, p.title, p.root_event_id)
        .map_err(map_store_err)?;
    ok(ThreadCreateResult { thread })
}

fn thread_list(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ThreadListParams = parse_params(params).unwrap_or(ThreadListParams { channel_id: None });
    let threads = state.store.list_threads(p.channel_id.as_deref());
    ok(ThreadListResult { threads })
}

fn thread_update(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ThreadUpdateParams = parse_params(params)?;
    let thread = state
        .store
        .update_thread(&p.thread_id, p.title)
        .map_err(map_store_err)?;
    ok(ThreadUpdateResult { thread })
}

fn thread_delete(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ThreadDeleteParams = parse_params(params)?;
    let deleted = state
        .store
        .delete_thread(&p.thread_id)
        .map_err(map_store_err)?;
    ok(ThreadDeleteResult { deleted })
}

// ---- turn ----

fn turn_open(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: TurnOpenParams = parse_params(params)?;
    let turn = state
        .store
        .open_turn(p.actor_id, p.scope, p.trigger_event_id)
        .map_err(map_store_err)?;
    ok(TurnOpenResult { turn })
}

async fn turn_close(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TurnCloseParams = parse_params(params)?;

    // Non-cancel paths: keep the v0 behavior — just write to store. Closing a
    // turn this way is the agent's own internal lifecycle event (the embedded
    // wakeup translator does this directly via store), or an external `joi
    // agent serve` reporting completion. No adapter side-effects.
    if !matches!(p.status, TurnStatus::Cancelled) {
        let turn = state
            .store
            .close_turn(&p.turn_id, p.status)
            .map_err(map_store_err)?;
        return ok(TurnCloseResult { turn });
    }

    // ---- cancel path ----
    let caller = state
        .subscriptions
        .actor_for_connection(connection_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_INVALID_STATE, "connection has no actor"))?;

    let turn = state
        .store
        .get_turn(&p.turn_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "turn"))?;

    // ACL: any member of the channel hosting the turn's scope may cancel.
    let channel_id = crate::ws::channel_id_for_scope(state, &turn.scope)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "channel for turn scope"))?;
    if !state.store.is_channel_member(&channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot cancel turn in channel {channel_id}"),
        ));
    }

    // Best-effort: nudge the adapter to stop. The eventual `Finished
    // {success:false, summary:"cancelled"}` echo is no-oped below by the
    // active-turn clearance — the journal write here is what other
    // subscribers see.
    if let Some(adapter) = state.runtime.adapter_for(&turn.actor_id) {
        if let Err(e) = adapter.cancel(turn.scope.clone()).await {
            tracing::warn!(actor = %turn.actor_id, %e, "adapter cancel failed");
        }
    }

    // Flush any partial text the agent had already produced into a final
    // `content.add` carrying `_meta.cancelled: true`. Preserves the work so
    // the user can read what got typed before they stopped it.
    if let Some(text) = state.runtime.take_text_buffer(&turn.actor_id, &p.turn_id) {
        let payload = json!({
            "contentType": "text/markdown",
            "text": text,
            "_meta": { "cancelled": true },
        });
        if let Err(e) = state.store.append_event(
            "content.add".into(),
            turn.actor_id.clone(),
            turn.scope.clone(),
            Some(p.turn_id.clone()),
            payload,
            vec![],
            None,
        ) {
            tracing::warn!(turn = %p.turn_id, %e, "failed to flush cancelled text buffer");
        }
    }

    // Journal the cancel as a `turn.close` event so channel members render a
    // system divider. `_meta.cancelledBy` lets clients show who pressed stop.
    let close_payload = json!({
        "status": "cancelled",
        "stopReason": "user_cancelled",
        "_meta": { "cancelledBy": caller },
    });
    if let Err(e) = state.store.append_event(
        "turn.close".into(),
        turn.actor_id.clone(),
        turn.scope.clone(),
        Some(p.turn_id.clone()),
        close_payload,
        vec![],
        None,
    ) {
        tracing::warn!(turn = %p.turn_id, %e, "failed to write turn.close cancellation event");
    }

    let closed = state
        .store
        .close_turn(&p.turn_id, TurnStatus::Cancelled)
        .map_err(map_store_err)?;

    // Clear runtime state so (a) the late ACP `Finished` echo is a no-op
    // (its `let Some(tid) = active_turn ...` guard fails) and (b) the next
    // FIFO trigger for this scope gets dispatched. Mirrors the wakeup path
    // at wakeup.rs:426.
    if let Some(next) = state
        .runtime
        .clear_active_turn(&turn.actor_id, &turn.scope.id)
    {
        let mgr = state.runtime.clone();
        let actor = turn.actor_id.clone();
        tokio::spawn(async move {
            if let Err(e) = wakeup::dispatch_trigger(mgr, actor.clone(), next).await {
                tracing::warn!(actor = %actor, %e, "post-cancel queued trigger dispatch failed");
            }
        });
    }

    ok(TurnCloseResult { turn: closed })
}

fn turn_trace_read(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TurnTraceReadParams = parse_params(params)?;
    let turn = state
        .store
        .get_turn(&p.turn_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "turn"))?;
    // Owner-only: the connection must be bound to the actor that owns the
    // turn. Trace frames are private execution detail of that actor; other
    // actors must not see them.
    let caller_actor = state
        .subscriptions
        .actor_for_connection(connection_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_INVALID_STATE, "connection has no actor"))?;
    if caller_actor != turn.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "trace is private to the turn owner",
        ));
    }
    let (frames, has_more) = state
        .store
        .read_turn_trace(&p.turn_id, p.limit, p.before_seq)
        .map_err(map_store_err)?;
    ok(TurnTraceReadResult {
        frames,
        page_info: PageInfo {
            has_more,
            next_cursor: None,
            _meta: None,
        },
    })
}

fn turn_trace_append(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TurnTraceAppendParams = parse_params(params)?;
    let turn = state
        .store
        .get_turn(&p.turn_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "turn"))?;
    let caller_actor = state
        .subscriptions
        .actor_for_connection(connection_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_INVALID_STATE, "connection has no actor"))?;
    // Same owner-only constraint as turn_trace_read: only the actor that owns
    // the turn (the agent for which it was opened) may write its own trace.
    if caller_actor != turn.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "trace is private to the turn owner",
        ));
    }
    let frame = state
        .store
        .append_trace_frame(&p.turn_id, p.kind, p.payload)
        .map_err(map_store_err)?;
    ok(TurnTraceAppendResult { frame })
}

// ---- event/append ----

async fn event_append(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: EventAppendParams = parse_params(params)?;
    let input = p.event;
    let event = state
        .store
        .append_event(
            input.kind.clone(),
            input.actor_id,
            input.scope,
            input.turn_id,
            input.payload,
            input.relations,
            input._meta,
        )
        .map_err(map_store_err)?;

    // If this is an action.response, forward it to the originating ACP child.
    if event.kind == "action.response" {
        let runtime = state.runtime.clone();
        let store = state.store.clone();
        let ev = event.clone();
        tokio::spawn(async move {
            wakeup::forward_action_response(&runtime, &store, &ev).await;
        });
    }
    ok(EventAppendResult { event })
}

// ---- artifact ----

fn artifact_publish(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ArtifactPublishParams = parse_params(params)?;
    if let Some(scope) = p.scope.as_ref() {
        state
            .store
            .check_scope_access(scope, &p.created_by)
            .map_err(map_store_err)?;
    }
    let artifact = state
        .artifacts
        .publish(&state.store, p)
        .map_err(map_store_err)?;
    ok(ArtifactPublishResult { artifact })
}

fn artifact_get(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ArtifactGetParams = parse_params(params)?;
    let artifact = if let Some(id) = p.artifact_id {
        state.store.get_artifact(&id)
    } else if let Some(uri) = p.artifact_uri {
        state.store.get_artifact_by_uri(&uri)
    } else {
        return Err(ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            "artifactId or artifactUri required",
        ));
    };
    let artifact =
        artifact.ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "artifact"))?;
    ok(ArtifactGetResult { artifact })
}

fn artifact_read(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ArtifactReadParams = parse_params(params)?;
    let artifact = state
        .store
        .get_artifact(&p.artifact_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "artifact"))?;
    let res = state
        .artifacts
        .read(&artifact, p.max_bytes)
        .map_err(map_store_err)?;
    ok(res)
}

// ---- receipt ----

fn receipt_record(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ReceiptRecordParams = parse_params(params)?;
    let r = state
        .store
        .record_receipt(p.event_id, p.actor_id, p.kind)
        .map_err(map_store_err)?;
    ok(ReceiptRecordResult { receipt: r })
}

// ---- actor / agent ----

fn actor_list(state: &AppState) -> HandlerResult {
    ok(ActorListResult {
        actors: state.store.list_actors(),
    })
}

/// Pre-register or update an actor row. `connection/open` already does an
/// implicit upsert, but it stamps `kind = Human` if the caller forgets to
/// pass `actorKind`. This dedicated RPC lets `joi agent serve` (and any
/// other operator) declare an agent's full `Actor` (id, kind, display,
/// capabilities) before the agent ever opens its own connection — which
/// is what makes the "invite this agent into the channel, agent connects
/// later" flow possible.
fn actor_upsert(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ActorUpsertParams = parse_params(params)?;
    let actor = state.store.upsert_actor(p.actor).map_err(map_store_err)?;
    ok(ActorUpsertResult { actor })
}

fn agent_list(state: &AppState) -> HandlerResult {
    ok(AgentListResult {
        agents: state.runtime.list(),
    })
}

fn agent_register(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: AgentRegisterParams = parse_params(params)?;
    let info = state
        .runtime
        .register(p.spec)
        .map_err(|e| ErrorObject::new(ErrorCode::APP_RUNTIME_ERROR, e.to_string()))?;
    ok(AgentRegisterResult { agent: info })
}

fn agent_unregister(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: AgentByIdParams = parse_params(params)?;
    state
        .runtime
        .unregister(&p.actor_id)
        .map_err(|e| ErrorObject::new(ErrorCode::APP_RUNTIME_ERROR, e.to_string()))?;
    ok(AgentOkResult { ok: true })
}

async fn agent_start(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: AgentByIdParams = parse_params(params)?;
    let runtime = state.runtime.clone();
    runtime
        .ensure_started(&p.actor_id)
        .await
        .map_err(|e| ErrorObject::new(ErrorCode::APP_RUNTIME_ERROR, e.to_string()))?;
    let info = runtime
        .get_info(&p.actor_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "agent"))?;
    ok(AgentSimpleResult { agent: info })
}

async fn agent_stop(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: AgentByIdParams = parse_params(params)?;
    state
        .runtime
        .stop(&p.actor_id)
        .await
        .map_err(|e| ErrorObject::new(ErrorCode::APP_RUNTIME_ERROR, e.to_string()))?;
    ok(AgentOkResult { ok: true })
}

fn agent_log(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: AgentLogParams = parse_params(params)?;
    let lines = state
        .runtime
        .log(&p.actor_id, p.tail)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "agent"))?;
    ok(AgentLogResult { lines })
}

// ---- agent/listMarketplace + agent/install ----

fn agent_list_marketplace() -> HandlerResult {
    ok(AgentMarketplaceListResult {
        entries: proto::marketplace::list_all(),
    })
}

fn agent_install(state: &AppState, params: Option<Value>) -> HandlerResult {
    use proto::marketplace::{lookup, resolve, Preference};

    let p: AgentInstallParams = parse_params(params)?;
    let entry = lookup(&p.marketplace_id).ok_or_else(|| {
        ErrorObject::new(
            ErrorCode::APP_NOT_FOUND,
            format!("marketplace entry `{}` not found", p.marketplace_id),
        )
    })?;
    let prefer = match p.prefer.as_deref().unwrap_or("auto") {
        "npx" => Preference::Npx,
        "uvx" => Preference::Uvx,
        "binary" => Preference::Binary,
        _ => Preference::Auto,
    };
    let resolved = resolve(&entry, prefer, |bin| path_lookup_via_env(bin))
        .map_err(|e| ErrorObject::new(ErrorCode::APP_INVALID_STATE, e.to_string()))?;

    let local_id = p
        .local_actor_id
        .clone()
        .unwrap_or_else(|| format!("actor_{}", entry.id.replace('-', "_")));
    let display = p.display_name.clone().unwrap_or_else(|| entry.name.clone());

    let spec = AgentSpec {
        actor: Actor {
            id: local_id.clone(),
            kind: ActorKind::Agent,
            display_name: display,
            capabilities: None,
            _meta: None,
        },
        transport: AgentTransport {
            kind: "acp_stdio".into(),
            command: resolved.command.clone(),
            args: resolved.args.clone(),
            env: resolved.env.clone(),
            cwd: "{agent.workspace}".into(),
            auth_method: None,
            session: None,
            output_format: None,
            prompt_via: proto::methods::PromptVia::default(),
        },
        autostart: false,
        bundle: None,
        // Marketplace install gets persona + memory on by default: identity
        // files scaffold from the marketplace description, memory prompt /
        // MCP delivery are enabled so `memory.query` is reachable from the
        // first turn. Old hand-written specs that skip these fields get
        // `None` via serde default — behavior preserved.
        identity: Some(proto::methods::IdentitySpec::default()),
        memory: Some(proto::methods::MemorySpec {
            delivery: proto::methods::MemoryDeliverySpec {
                prompt: true,
                mcp: true,
            },
            ..Default::default()
        }),
    };

    let info = state
        .runtime
        .register(spec)
        .map_err(|e| ErrorObject::new(ErrorCode::APP_RUNTIME_ERROR, e.to_string()))?;
    ok(AgentInstallResult {
        agent: info,
        source: resolved.source,
    })
}

fn path_lookup_via_env(bin: &str) -> bool {
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return true;
        }
        // Windows: also try with .exe / .cmd
        if cfg!(windows) {
            for ext in ["exe", "cmd", "bat"] {
                let mut c = candidate.clone();
                c.set_extension(ext);
                if c.is_file() {
                    return true;
                }
            }
        }
    }
    false
}

#[allow(dead_code)]
pub fn _ensure_arc<T>(x: Arc<T>) -> Arc<T> {
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::ArtifactStore;
    use crate::journal::Journal;
    use crate::runtime::RuntimeManager;
    use crate::state::AppState;
    use crate::store::Store;
    use crate::subscribe::Subscriptions;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "joi-handler-tests-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos()
        ));
        path
    }

    fn test_state(name: &str) -> AppState {
        let root = temp_path(name);
        std::fs::create_dir_all(&root).expect("create root");
        let journal = Journal::open(root.join("journal.jsonl")).expect("open journal");
        let store = Store::open(journal).expect("open store");
        let subscriptions = Subscriptions::new();
        let artifacts = Arc::new(
            ArtifactStore::new(root.join("artifacts"), root.join("workspaces"))
                .expect("open artifacts"),
        );
        let runtime = RuntimeManager::new(
            root.join("runtime"),
            root.join("agents"),
            store.clone(),
            "ws://test/rpc".into(),
        )
        .expect("create runtime");
        AppState {
            store,
            subscriptions,
            runtime,
            artifacts,
        }
    }

    #[test]
    fn artifact_publish_rejects_inaccessible_scope() {
        let state = test_state("artifact-publish-acl");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");

        let err = artifact_publish(
            &state,
            Some(json!({
                "createdBy": "actor_guest",
                "scope": { "kind": "channel", "id": channel.id },
                "ingress": {
                    "kind": "inline_text",
                    "name": "note.md",
                    "mediaType": "text/markdown",
                    "text": "hello"
                }
            })),
        )
        .expect_err("artifact publish should fail");

        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);
    }

    #[test]
    fn artifact_publish_rejects_missing_scope() {
        let state = test_state("artifact-publish-missing-scope");

        let err = artifact_publish(
            &state,
            Some(json!({
                "createdBy": "actor_owner",
                "scope": { "kind": "thread", "id": "thread_missing" },
                "ingress": {
                    "kind": "inline_text",
                    "name": "note.md",
                    "mediaType": "text/markdown",
                    "text": "hello"
                }
            })),
        )
        .expect_err("artifact publish should fail");

        assert_eq!(err.code, ErrorCode::APP_NOT_FOUND);
    }
}
