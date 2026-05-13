use std::sync::Arc;

use chrono::Utc;
use proto::methods::*;
use proto::types::*;
use proto::{ErrorCode, ErrorObject};
use serde_json::{json, Value};
use uuid::Uuid;

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

#[allow(dead_code)]
fn map_runtime_err(err: impl std::fmt::Display) -> ErrorObject {
    ErrorObject::new(ErrorCode::APP_RUNTIME_ERROR, err.to_string())
}

/// Log a scope-skills projection failure that follows a successful store
/// mutation. We deliberately do NOT surface this as an RPC error: the
/// authoritative state (channel/thread/membership) is already persisted, so
/// returning an error would tempt callers to retry and create duplicates.
/// The projection is reconciled lazily on next read / restart, or by an
/// out-of-band repair job.
fn warn_projection_failure(operation: &str, err: impl std::fmt::Display) {
    tracing::warn!(
        operation,
        %err,
        "scope_skills projection failed after successful store mutation; \
         leaving store mutation in place and relying on lazy reconciliation"
    );
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
        method::CONNECTION_LIST => connection_list(state, params),
        method::SCOPE_SUBSCRIBE => scope_subscribe(state, connection_id, params),
        method::SCOPE_UNSUBSCRIBE => scope_unsubscribe(state, connection_id, params),
        method::SCOPE_READ => scope_read(state, connection_id, params),
        method::CHANNEL_CREATE => channel_create(state, connection_id, params),
        method::CHANNEL_LIST => channel_list(state, connection_id),
        method::CHANNEL_UPDATE => channel_update(state, params),
        method::CHANNEL_DELETE => channel_delete(state, connection_id, params),
        method::CHANNEL_INVITE => channel_invite(state, connection_id, params),
        method::CHANNEL_REVOKE => channel_revoke(state, connection_id, params),
        method::CHANNEL_MEMBERS => channel_members(state, connection_id, params),
        method::THREAD_CREATE => thread_create(state, connection_id, params),
        method::THREAD_LIST => thread_list(state, connection_id, params),
        method::THREAD_UPDATE => thread_update(state, connection_id, params),
        method::THREAD_ARCHIVE => thread_archive(state, connection_id, params),
        method::THREAD_DELETE => thread_delete(state, connection_id, params),
        method::TASK_CREATE => task_create(state, connection_id, params),
        method::TASK_GET => task_get(state, connection_id, params),
        method::TASK_LIST => task_list(state, connection_id, params),
        method::TASK_UPDATE => task_update(state, connection_id, params),
        method::TASK_ASSIGNMENT_CREATE => {
            task_assignment_create(state, connection_id, params).await
        }
        method::TASK_ASSIGNMENT_UPDATE => task_assignment_update(state, connection_id, params),
        method::TURN_OPEN => turn_open(state, params),
        method::TURN_CLOSE => turn_close(state, connection_id, params).await,
        method::TURN_TRACE_READ => turn_trace_read(state, connection_id, params),
        method::TURN_TRACE_APPEND => turn_trace_append(state, connection_id, params),
        method::EVENT_APPEND => event_append(state, params).await,
        method::MESSAGE_SEARCH => message_search(state, connection_id, params),
        method::ARTIFACT_PUBLISH => artifact_publish(state, params),
        method::ARTIFACT_GET => artifact_get(state, params),
        method::ARTIFACT_READ => artifact_read(state, params),
        method::RECEIPT_RECORD => receipt_record(state, params),
        method::REMINDER_SCHEDULE => reminder_schedule(state, params),
        method::REMINDER_LIST => reminder_list(state, params),
        method::REMINDER_CANCEL => reminder_cancel(state, params),
        method::REMINDER_SNOOZE => reminder_snooze(state, params),
        method::REMINDER_UPDATE => reminder_update(state, params),
        method::DELIVERY_LIST => delivery_list(state, connection_id, params),
        method::MACHINE_COMMAND => machine_command(state, connection_id, params).await,
        method::MACHINE_COMMAND_CREATE => {
            machine_command_create(state, connection_id, params).await
        }
        method::MACHINE_COMMAND_GET => machine_command_get(state, connection_id, params),
        method::MACHINE_COMMAND_LIST => machine_command_list(state, connection_id, params),
        method::MACHINE_COMMAND_ACK => machine_command_ack(state, connection_id, params),
        method::MACHINE_COMMAND_RESULT => machine_command_result(state, connection_id, params),
        method::MACHINE_COMMAND_CANCEL => machine_command_cancel(state, connection_id, params),
        method::ACTOR_LIST => actor_list(state),
        method::ACTOR_UPSERT => actor_upsert(state, params),
        method::ACTOR_DELETE => actor_delete(state, params),
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
            "extensions": ["agent", "task"],
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
        .bind_actor(connection_id, actor_id.clone(), claim_kind, p.claim_inbox);
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

fn connection_list(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ConnectionListParams = if params.is_some() {
        parse_params(params)?
    } else {
        ConnectionListParams::default()
    };
    ok(ConnectionListResult {
        actor_ids: state.subscriptions.connected_actor_ids(&p.actor_ids),
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
    if let Some(actor_id) = channel.members.first() {
        if let Err(e) =
            state
                .scope_skills
                .sync_actor_channel_membership(&state.store, &channel.id, actor_id)
        {
            warn_projection_failure("channel_create.sync_actor_channel_membership", e);
        }
    }
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
    if let Err(e) =
        state
            .scope_skills
            .sync_actor_channel_membership(&state.store, &channel.id, &p.actor_id)
    {
        warn_projection_failure("channel_invite.sync_actor_channel_membership", e);
    }
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
    if let Err(e) =
        state
            .scope_skills
            .remove_actor_channel_membership(&state.store, &channel.id, &p.actor_id)
    {
        warn_projection_failure("channel_revoke.remove_actor_channel_membership", e);
    }
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

fn channel_delete(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ChannelDeleteParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot delete channel {}", p.channel_id),
        ));
    }
    let thread_ids = state
        .store
        .list_threads(Some(&p.channel_id))
        .into_iter()
        .map(|thread| thread.id)
        .collect::<Vec<_>>();
    let (deleted, deleted_threads) = state
        .store
        .delete_channel(&p.channel_id, p.cascade)
        .map_err(map_store_err)?;
    if deleted {
        if let Err(e) = state
            .scope_skills
            .remove_channel_scope(&p.channel_id, &thread_ids)
        {
            warn_projection_failure("channel_delete.remove_channel_scope", e);
        }
    }
    ok(ChannelDeleteResult {
        deleted,
        deleted_threads,
    })
}

// ---- thread ----

fn thread_create(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ThreadCreateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    // Channel-membership gate: a non-member cannot spawn a thread inside a
    // private channel, even though the underlying store would happily create
    // one. Mirrors the channel_invite pattern; public channels short-circuit
    // via is_channel_member.
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot create thread in channel {}",
                p.channel_id
            ),
        ));
    }
    let thread = state
        .store
        .create_thread(p.channel_id, p.title, p.root_event_id)
        .map_err(map_store_err)?;
    if let Err(e) = state
        .scope_skills
        .sync_thread_channel_memberships(&state.store, &thread)
    {
        warn_projection_failure("thread_create.sync_thread_channel_memberships", e);
    }
    ok(ThreadCreateResult { thread })
}

fn thread_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ThreadListParams = parse_params(params).unwrap_or(ThreadListParams {
        channel_id: None,
        archived: false,
    });
    let caller = state.subscriptions.actor_for_connection(connection_id);
    // Silently drop threads in channels the caller can't see — same shape
    // as channel_list. Unbound callers (no actor) only see public-channel
    // threads. This means an unauthorized client never learns thread titles
    // or ids, just an empty list.
    let threads = state
        .store
        .list_threads_filtered(p.channel_id.as_deref(), p.archived)
        .into_iter()
        .filter(|t| match caller.as_deref() {
            Some(actor) => state.store.is_channel_member(&t.channel_id, actor),
            None => state
                .store
                .get_channel(&t.channel_id)
                .map(|c| matches!(c.visibility, ChannelVisibility::Public))
                .unwrap_or(false),
        })
        .collect();
    ok(ThreadListResult { threads })
}

fn thread_update(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ThreadUpdateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let channel_id = state
        .store
        .get_thread(&p.thread_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "thread"))?
        .channel_id;
    if !state.store.is_channel_member(&channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot update thread {} in channel {channel_id}",
                p.thread_id
            ),
        ));
    }
    let thread = state
        .store
        .update_thread(&p.thread_id, p.title)
        .map_err(map_store_err)?;
    ok(ThreadUpdateResult { thread })
}

fn thread_archive(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ThreadArchiveParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let channel_id = state
        .store
        .get_thread(&p.thread_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "thread"))?
        .channel_id;
    if !state.store.is_channel_member(&channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot archive thread {} in channel {channel_id}",
                p.thread_id
            ),
        ));
    }
    let thread = state
        .store
        .archive_thread(&p.thread_id, p.archived)
        .map_err(map_store_err)?;
    ok(ThreadArchiveResult { thread })
}

fn thread_delete(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ThreadDeleteParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let channel_id = state
        .store
        .get_thread(&p.thread_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "thread"))?
        .channel_id;
    if !state.store.is_channel_member(&channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot delete thread {} in channel {channel_id}",
                p.thread_id
            ),
        ));
    }
    let deleted = state
        .store
        .delete_thread(&p.thread_id)
        .map_err(map_store_err)?;
    if deleted {
        if let Err(e) = state.scope_skills.remove_thread_scope(&p.thread_id) {
            warn_projection_failure("thread_delete.remove_thread_scope", e);
        }
    }
    ok(ThreadDeleteResult { deleted })
}

// ---- task ----

fn ensure_task_access(state: &AppState, task: &Task, actor_id: &str) -> Result<(), ErrorObject> {
    if state.store.is_channel_member(&task.channel_id, actor_id) {
        Ok(())
    } else {
        Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {actor_id} cannot access task {} in channel {}",
                task.id, task.channel_id
            ),
        ))
    }
}

fn task_create(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskCreateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let source = state
        .store
        .get_event(&p.source_event_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "source event"))?;
    state
        .store
        .check_scope_access(&source.scope, &caller)
        .map_err(map_store_err)?;
    let requester = p.requester_actor_id.unwrap_or(caller);
    let task = state
        .store
        .create_task(
            p.source_event_id,
            p.title,
            p.description,
            requester,
            p.owner_actor_id,
            p.status,
        )
        .map_err(map_store_err)?;
    ok(TaskCreateResult { task })
}

fn task_get(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskGetParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let assignments = state.store.list_task_assignments(&task.id);
    ok(TaskGetResult { task, assignments })
}

fn task_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskListParams = parse_params(params).unwrap_or_default();
    let caller = caller_actor(state, connection_id)?;
    if let Some(channel_id) = p.channel_id.as_ref() {
        if !state.store.is_channel_member(channel_id, &caller) {
            return Err(ErrorObject::new(
                ErrorCode::APP_INVALID_STATE,
                format!("actor {caller} cannot list tasks in channel {channel_id}"),
            ));
        }
    }
    let tasks = state
        .store
        .list_tasks(
            p.channel_id.as_deref(),
            p.source_event_id.as_deref(),
            p.owner_actor_id.as_deref(),
            &p.statuses,
        )
        .into_iter()
        .filter(|task| state.store.is_channel_member(&task.channel_id, &caller))
        .collect();
    ok(TaskListResult { tasks })
}

fn task_update(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskUpdateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let task = state
        .store
        .update_task(
            &p.task_id,
            p.status,
            p.owner_actor_id,
            p.result_summary,
            p.artifact_ids,
            p.append_artifact_ids,
        )
        .map_err(map_store_err)?;
    ok(TaskUpdateResult { task })
}

async fn task_assignment_create(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskAssignmentCreateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let from = p.from_actor_id.unwrap_or(caller);
    let (assignment, task) = state
        .store
        .create_task_assignment(
            &p.task_id,
            from.clone(),
            p.to_actor_id.clone(),
            p.assignment_type,
            p.instruction.clone(),
        )
        .map_err(map_store_err)?;

    let event = state
        .store
        .append_event(
            "content.add".into(),
            from,
            ScopeRef {
                kind: ScopeKind::Thread,
                id: task.canonical_thread_id.clone(),
            },
            None,
            json!({
                "contentType": "text/markdown",
                "text": task_assignment_message(&task, &assignment),
                "_meta": {
                    "taskId": task.id,
                    "taskNumber": task.number,
                    "assignmentId": assignment.id,
                    "assignmentType": assignment.assignment_type,
                    "expectedOutput": "Update the assignment result and reply in this task thread."
                }
            }),
            vec![
                Relation {
                    kind: RelationKind::HandsOffTo,
                    target: Ref {
                        kind: RefKind::Actor,
                        id: assignment.to_actor_id.clone(),
                        _meta: None,
                    },
                    _meta: None,
                },
                Relation {
                    kind: RelationKind::RelatesToTask,
                    target: Ref {
                        kind: RefKind::Task,
                        id: task.id.clone(),
                        _meta: None,
                    },
                    _meta: None,
                },
                Relation {
                    kind: RelationKind::RespondsTo,
                    target: Ref {
                        kind: RefKind::Event,
                        id: task.source_event_id.clone(),
                        _meta: None,
                    },
                    _meta: None,
                },
            ],
            None,
        )
        .map_err(map_store_err)?;
    ok(TaskAssignmentCreateResult {
        assignment,
        event,
        task,
    })
}

fn task_assignment_update(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskAssignmentUpdateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let requested_status = p.status;
    let assignment = state
        .store
        .get_assignment(&p.assignment_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "assignment"))?;
    let task = state
        .store
        .get_task(&assignment.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let (assignment, task) = state
        .store
        .update_task_assignment(
            &p.assignment_id,
            requested_status,
            p.result_event_id,
            p.result_summary,
        )
        .map_err(map_store_err)?;
    if requested_status.is_some_and(is_terminal_assignment_status) {
        emit_assignment_return_handoff(state, &caller, &task, &assignment);
    }
    ok(TaskAssignmentUpdateResult { assignment, task })
}

fn is_terminal_assignment_status(status: TaskAssignmentStatus) -> bool {
    matches!(
        status,
        TaskAssignmentStatus::Completed
            | TaskAssignmentStatus::Failed
            | TaskAssignmentStatus::Canceled
    )
}

fn emit_assignment_return_handoff(
    state: &AppState,
    from_actor_id: &str,
    task: &Task,
    assignment: &TaskAssignment,
) {
    let target_actor_id = assignment.from_actor_id.as_str();
    let status = serde_json::to_value(assignment.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{:?}", assignment.status));
    let mut text = format!(
        "Assignment `{assignment_id}` ({assignment_type:?}) is {status}.\n\nHanding task #{number} back to @{target_actor_id}. Continue from the assignment result: revise, create a follow-up assignment, ask the requester, or update the task status.",
        assignment_id = assignment.id,
        assignment_type = assignment.assignment_type,
        number = task.number,
    );
    if !assignment.result_summary.trim().is_empty() {
        text.push_str("\n\nResult summary:\n");
        text.push_str(assignment.result_summary.trim());
    }

    if let Err(err) = state.store.append_event(
        "content.add".into(),
        from_actor_id.into(),
        ScopeRef {
            kind: ScopeKind::Thread,
            id: task.canonical_thread_id.clone(),
        },
        None,
        json!({
            "contentType": "text/markdown",
            "text": text,
            "_meta": {
                "taskId": task.id.clone(),
                "taskNumber": task.number,
                "assignmentId": assignment.id.clone(),
                "assignmentStatus": status,
                "expectedOutput": "Continue the task in this thread; either revise, create follow-up assignments, ask the requester, or update the task status."
            }
        }),
        vec![
            Relation {
                kind: RelationKind::HandsOffTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: target_actor_id.into(),
                    _meta: None,
                },
                _meta: None,
            },
            Relation {
                kind: RelationKind::RelatesToTask,
                target: Ref {
                    kind: RefKind::Task,
                    id: task.id.clone(),
                    _meta: None,
                },
                _meta: None,
            },
        ],
        None,
    ) {
        tracing::warn!(
            task = %task.id,
            assignment = %assignment.id,
            target = %target_actor_id,
            %err,
            "failed to append assignment return handoff"
        );
    }
}

fn task_assignment_message(task: &Task, assignment: &TaskAssignment) -> String {
    format!(
        "Task #{number}: {title}\n\nAssignment `{assignment_id}` ({assignment_type:?}) for @{to_actor}.\n\n{instruction}\n\nReturn your result in this thread and update the assignment when complete. Completion hands the task back to the actor who assigned it for the next step.",
        number = task.number,
        title = task.title,
        assignment_id = assignment.id,
        assignment_type = assignment.assignment_type,
        to_actor = assignment.to_actor_id,
        instruction = assignment.instruction.trim(),
    )
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

    // Non-cancel paths: just write to store. Closing a turn this way is the
    // actor's own lifecycle report, typically from an external `joi agent
    // serve` worker. No adapter side-effects live in the server.
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

    // Journal the cancel as a `turn.close` event so channel members render a
    // system divider. The event is handed to the agent actor so external
    // `joi daemon` can cancel the actual adapter process; server itself
    // stays transport-agnostic.
    let close_payload = json!({
        "status": "cancelled",
        "stopReason": "user_cancelled",
        "_meta": { "cancelledBy": caller },
    });
    if let Err(e) = state.store.append_event(
        "turn.close".into(),
        caller.clone(),
        turn.scope.clone(),
        Some(p.turn_id.clone()),
        close_payload,
        vec![Relation {
            kind: RelationKind::HandsOffTo,
            target: Ref {
                kind: RefKind::Actor,
                id: turn.actor_id.clone(),
                _meta: None,
            },
            _meta: None,
        }],
        None,
    ) {
        tracing::warn!(turn = %p.turn_id, %e, "failed to write turn.close cancellation event");
    }

    let closed = state
        .store
        .close_turn(&p.turn_id, TurnStatus::Cancelled)
        .map_err(map_store_err)?;

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

    ok(EventAppendResult { event })
}

fn message_search(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: MessageSearchParams = parse_params(params)?;
    let actor_id = caller_actor(state, connection_id)?;
    if p.query.trim().is_empty() {
        return Err(ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            "query is empty",
        ));
    }
    if let Some(scope) = p.scope.as_ref() {
        state
            .store
            .check_scope_access(scope, &actor_id)
            .map_err(map_store_err)?;
    }
    ok(MessageSearchResult {
        events: state
            .store
            .search_messages(&actor_id, &p.query, p.scope.as_ref(), p.limit),
    })
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
        .read(&artifact, p.offset, p.max_bytes)
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

// ---- reminder ----

fn reminder_schedule(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ReminderScheduleParams = parse_params(params)?;
    let fire_at = match (p.fire_at, p.delay_seconds) {
        (Some(fire_at), _) => fire_at,
        (None, Some(delay)) if delay > 0 => Utc::now() + chrono::Duration::seconds(delay),
        (None, Some(_)) => {
            return Err(ErrorObject::new(
                ErrorCode::INVALID_PARAMS,
                "delaySeconds must be positive",
            ))
        }
        (None, None) => {
            return Err(ErrorObject::new(
                ErrorCode::INVALID_PARAMS,
                "fireAt or delaySeconds required",
            ))
        }
    };
    let reminder = state
        .store
        .schedule_reminder(p.actor_id, p.title, p.scope, p.msg_id, fire_at, p.repeat)
        .map_err(map_store_err)?;
    ok(ReminderScheduleResult { reminder })
}

fn reminder_list(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ReminderListParams = parse_params(params)?;
    ok(ReminderListResult {
        reminders: state.store.list_reminders(&p.actor_id, &p.statuses, p.all),
    })
}

fn reminder_cancel(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ReminderIdParams = parse_params(params)?;
    let reminder = state
        .store
        .cancel_reminder(&p.actor_id, &p.id)
        .map_err(map_store_err)?;
    ok(ReminderCancelResult { reminder })
}

fn reminder_snooze(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ReminderSnoozeParams = parse_params(params)?;
    let reminder = state
        .store
        .snooze_reminder(&p.actor_id, &p.id, p.by_seconds)
        .map_err(map_store_err)?;
    ok(ReminderSnoozeResult { reminder })
}

fn reminder_update(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ReminderUpdateParams = parse_params(params)?;
    let fire_at = match (p.fire_at, p.delay_seconds) {
        (Some(fire_at), _) => Some(fire_at),
        (None, Some(delay)) if delay > 0 => Some(Utc::now() + chrono::Duration::seconds(delay)),
        (None, Some(_)) => {
            return Err(ErrorObject::new(
                ErrorCode::INVALID_PARAMS,
                "delaySeconds must be positive",
            ))
        }
        (None, None) => None,
    };
    let reminder = state
        .store
        .update_reminder(&p.actor_id, &p.id, p.title, fire_at, p.repeat)
        .map_err(map_store_err)?;
    ok(ReminderUpdateResult { reminder })
}

// ---- delivery/list (§9.2 durable actor inbox) ----
//
// Cursor format is `<rfc3339_utc>|<event_id>`. The `|` separator avoids
// collision with the `:` characters inside an RFC3339 timestamp; event ids
// are uuid v4 (no `|`) so the split is unambiguous. Cursor is opaque to
// callers — the format is an implementation detail of this handler and
// `Store::list_deliveries`.

fn encode_delivery_cursor(d: &Delivery) -> String {
    format!("{}|{}", d.updated_at.to_rfc3339(), d.event_id)
}

fn decode_delivery_cursor(raw: &str) -> Result<(Timestamp, String), ErrorObject> {
    let (ts_str, eid) = raw.split_once('|').ok_or_else(|| {
        ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            format!("delivery/list: malformed cursor `{raw}` (expected `<ts>|<event_id>`)"),
        )
    })?;
    let ts = chrono::DateTime::parse_from_rfc3339(ts_str)
        .map_err(|e| {
            ErrorObject::new(
                ErrorCode::INVALID_PARAMS,
                format!("delivery/list: bad cursor timestamp `{ts_str}`: {e}"),
            )
        })?
        .with_timezone(&chrono::Utc);
    Ok((ts, eid.to_string()))
}

fn delivery_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: DeliveryListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if caller != p.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "delivery/list: caller actor `{caller}` cannot read inbox of `{}`",
                p.actor_id
            ),
        ));
    }
    let limit = p.limit.unwrap_or(50).clamp(1, 200) as usize;
    let after = p
        .cursor
        .as_deref()
        .map(decode_delivery_cursor)
        .transpose()?;
    // Fetch one extra row to detect whether a follow-up page exists.
    let mut rows = state
        .store
        .list_deliveries(&p.actor_id, p.state, limit + 1, after);
    let has_more = rows.len() > limit;
    if has_more {
        rows.truncate(limit);
    }
    let next_cursor = if has_more {
        rows.last().map(encode_delivery_cursor)
    } else {
        None
    };
    let entries: Vec<DeliveryListEntry> = rows
        .into_iter()
        .map(|d| {
            let event = state.store.get_event(&d.event_id);
            DeliveryListEntry { delivery: d, event }
        })
        .collect();
    ok(DeliveryListResult {
        deliveries: entries,
        next_cursor,
    })
}

async fn machine_command(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: MachineCommandParams = parse_params(params)?;
    let operation = p
        .command
        .get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|op| !op.is_empty())
        .ok_or_else(|| {
            ErrorObject::new(ErrorCode::INVALID_PARAMS, "machine command op is required")
        })?
        .to_string();
    let command_id = format!("mcmd_{}", Uuid::new_v4().simple());
    let create = MachineCommandCreateParams {
        machine_id: p.machine_id,
        machine_actor_id: p.machine_actor_id,
        workspace_id: p.workspace_id,
        operation,
        payload: p.command,
        if_inventory_revision: p.if_inventory_revision,
        command_id: Some(command_id.clone()),
        timeout_ms: p.timeout_ms,
    };
    let timeout_ms = create.timeout_ms.unwrap_or(30_000).clamp(1_000, 120_000);
    let rx = state.machine_commands.register(command_id.clone());
    let (command, _) = match create_machine_command(state, connection_id, create) {
        Ok(value) => value,
        Err(err) => {
            state.machine_commands.cancel(&command_id);
            return Err(err);
        }
    };
    let delivered = notify_machine_command(state, command)?;
    if !delivered {
        state.machine_commands.cancel(&command_id);
        // The command remains queued for daemon pull/reconnect; the wait wrapper
        // reports that synchronous delivery was not possible.
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "machine daemon is not connected; command is queued",
        ));
    }

    let result = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), rx).await
    {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            return Err(ErrorObject::new(
                ErrorCode::APP_RUNTIME_ERROR,
                format!("machine command result channel closed: {command_id}"),
            ));
        }
        Err(_) => {
            state.machine_commands.cancel(&command_id);
            return Err(ErrorObject::new(
                ErrorCode::APP_RUNTIME_ERROR,
                format!("machine command timed out: {command_id}"),
            ));
        }
    };
    let result_machine_id = result.machine_id.clone();
    let response = MachineCommandResponse {
        command_id: result.command_id,
        machine_id: result.machine_id,
        machine_actor_id: result
            .machine_actor_id
            .unwrap_or_else(|| machine_connection_actor_id(&result_machine_id)),
        ok: result.ok,
        output: result.output,
        error: result.error,
    };
    ok(response)
}

async fn machine_command_create(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: MachineCommandCreateParams = parse_params(params)?;
    let (command, _) = create_machine_command(state, connection_id, p)?;
    let command_id = command.command_id.clone();
    let delivered = notify_machine_command(state, command.clone())?;
    let command = state
        .store
        .get_machine_command(&command_id)
        .unwrap_or(command);
    ok(MachineCommandCreateResult { command, delivered })
}

fn machine_command_get(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: MachineCommandGetParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let command = state
        .store
        .get_machine_command(&p.command_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "machine command not found"))?;
    authorize_machine_command_reader(&caller, &command)?;
    ok(MachineCommandGetResult { command })
}

fn machine_command_list(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: MachineCommandListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let caller_is_machine = is_machine_actor(state, &caller);
    let machine_actor_id = if caller_is_machine {
        Some(caller.as_str())
    } else {
        p.machine_actor_id.as_deref()
    };
    let requested_by = if caller_is_machine {
        p.requested_by.as_deref()
    } else {
        Some(caller.as_str())
    };
    let commands = state.store.list_machine_commands(
        p.machine_id.as_deref(),
        machine_actor_id,
        &p.statuses,
        requested_by,
        p.limit.unwrap_or(100).clamp(1, 500),
    );
    ok(MachineCommandListResult { commands })
}

fn machine_command_ack(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: MachineCommandAckParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let mut command = state
        .store
        .get_machine_command(&p.command_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "machine command not found"))?;
    let expected_actor = p
        .machine_actor_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(&command.machine_actor_id);
    if caller != expected_actor || expected_actor != command.machine_actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor `{caller}` cannot ack command `{}`",
                command.command_id
            ),
        ));
    }
    if let Some(machine_id) = p.machine_id.as_deref() {
        if machine_id != command.machine_id {
            return Err(ErrorObject::new(
                ErrorCode::INVALID_PARAMS,
                "ack machineId does not match command",
            ));
        }
    }
    validate_machine_actor(
        state,
        &command.machine_id,
        &command.machine_actor_id,
        None,
        command.workspace_id.as_deref(),
        None,
        false,
    )?;
    if !command.status.is_terminal() {
        command.status = MachineCommandStatus::Running;
        command.attempts = command.attempts.saturating_add(1);
        command.updated_at = Utc::now();
        command = state
            .store
            .upsert_machine_command(command)
            .map_err(map_store_err)?;
    }
    ok(MachineCommandAckResult { command })
}

fn machine_command_result(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let result: MachineCommandResultParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let expected_actor = result
        .machine_actor_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| machine_connection_actor_id(&result.machine_id));
    if caller != expected_actor {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "machine/command.result: caller actor `{caller}` cannot complete command for `{expected_actor}`"
            ),
        ));
    }
    let mut command = state
        .store
        .get_machine_command(&result.command_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "unknown machine command"))?;
    if command.machine_id != result.machine_id || command.machine_actor_id != expected_actor {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "machine command result target mismatch",
        ));
    }
    validate_machine_actor(
        state,
        &result.machine_id,
        &expected_actor,
        None,
        command.workspace_id.as_deref(),
        None,
        false,
    )?;
    if command.status.is_terminal() {
        return ok(json!({ "accepted": true, "command": command }));
    }
    let now = Utc::now();
    command.status = result.status.unwrap_or(if result.ok {
        MachineCommandStatus::Succeeded
    } else {
        MachineCommandStatus::Failed
    });
    if !command.status.is_terminal() {
        command.status = if result.ok {
            MachineCommandStatus::Succeeded
        } else {
            MachineCommandStatus::Failed
        };
    }
    command.output = Some(result.output.clone());
    command.error = result.structured_error.clone().or_else(|| {
        result.error.as_ref().map(|message| MachineCommandError {
            code: "machine_command_failed".into(),
            message: message.clone(),
            retryable: false,
            details: None,
        })
    });
    command.finished_at = Some(now);
    command.updated_at = now;
    let command = state
        .store
        .upsert_machine_command(command)
        .map_err(map_store_err)?;
    state.machine_commands.complete(result.clone());
    ok(json!({ "accepted": true, "command": command }))
}

fn machine_command_cancel(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: MachineCommandCancelParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let mut command = state
        .store
        .get_machine_command(&p.command_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "machine command not found"))?;
    authorize_machine_command_reader(&caller, &command)?;
    if !command.status.is_terminal() {
        command.status = MachineCommandStatus::Cancelled;
        command.error = p.reason.map(|message| MachineCommandError {
            code: "cancelled".into(),
            message,
            retryable: false,
            details: None,
        });
        let now = Utc::now();
        command.finished_at = Some(now);
        command.updated_at = now;
        command = state
            .store
            .upsert_machine_command(command)
            .map_err(map_store_err)?;
    }
    state.machine_commands.cancel(&p.command_id);
    ok(MachineCommandCancelResult { command })
}

fn machine_connection_actor_id(machine_id: &str) -> String {
    format!("actor_service_{}", machine_id)
}

fn create_machine_command(
    state: &AppState,
    connection_id: &str,
    p: MachineCommandCreateParams,
) -> Result<(MachineCommand, bool), ErrorObject> {
    let machine_id = p.machine_id.trim();
    if machine_id.is_empty() {
        return Err(ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            "machine id is required",
        ));
    }
    let operation = p.operation.trim();
    if operation.is_empty() {
        return Err(ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            "machine command operation is required",
        ));
    }
    let caller = caller_actor(state, connection_id)?;
    let machine_actor_id = p
        .machine_actor_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| machine_connection_actor_id(machine_id));
    validate_machine_actor(
        state,
        machine_id,
        &machine_actor_id,
        Some(&caller),
        p.workspace_id.as_deref(),
        p.if_inventory_revision,
        is_mutating_machine_operation(operation),
    )?;
    let now = Utc::now();
    let command = MachineCommand {
        command_id: p
            .command_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("mcmd_{}", Uuid::new_v4().simple())),
        machine_id: machine_id.to_string(),
        machine_actor_id,
        requested_by: caller,
        workspace_id: p.workspace_id,
        operation: operation.to_string(),
        payload: p.payload,
        if_inventory_revision: p.if_inventory_revision,
        status: MachineCommandStatus::Queued,
        deadline_at: p
            .timeout_ms
            .map(|ms| now + chrono::Duration::milliseconds(ms.clamp(1_000, 120_000) as i64)),
        created_at: now,
        updated_at: now,
        attempts: 0,
        output: None,
        error: None,
        finished_at: None,
        _meta: None,
    };
    if state
        .store
        .get_machine_command(&command.command_id)
        .is_some()
    {
        return Err(ErrorObject::new(
            ErrorCode::APP_CONFLICT,
            format!("machine command already exists: {}", command.command_id),
        ));
    }
    let command = state
        .store
        .upsert_machine_command(command)
        .map_err(map_store_err)?;
    Ok((command, false))
}

fn notify_machine_command(
    state: &AppState,
    mut command: MachineCommand,
) -> Result<bool, ErrorObject> {
    let delivered = state.subscriptions.send_to_actor(
        &command.machine_actor_id,
        method::MACHINE_COMMAND_NOTIFY,
        machine_command_notification_payload(&command),
    );
    if delivered && command.status == MachineCommandStatus::Queued {
        command.status = MachineCommandStatus::Delivered;
        command.updated_at = Utc::now();
        state
            .store
            .upsert_machine_command(command)
            .map_err(map_store_err)?;
    }
    Ok(delivered)
}

fn machine_command_notification_payload(command: &MachineCommand) -> Value {
    json!({
        "commandId": command.command_id,
        "machineId": command.machine_id,
        "machineActorId": command.machine_actor_id,
        "requestedBy": command.requested_by,
        "workspaceId": command.workspace_id,
        "operation": command.operation,
        "payload": command.payload,
        "command": command.payload,
        "ifInventoryRevision": command.if_inventory_revision,
    })
}

fn authorize_machine_command_reader(
    caller: &str,
    command: &MachineCommand,
) -> Result<(), ErrorObject> {
    if caller == command.requested_by || caller == command.machine_actor_id {
        Ok(())
    } else {
        Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor `{caller}` cannot access command `{}`",
                command.command_id
            ),
        ))
    }
}

fn is_machine_actor(state: &AppState, actor_id: &str) -> bool {
    state
        .store
        .get_actor(actor_id)
        .and_then(|actor| actor._meta)
        .and_then(|meta| {
            meta.get("role")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .as_deref()
        == Some("machine")
}

fn is_mutating_machine_operation(operation: &str) -> bool {
    matches!(
        operation,
        "agent.create" | "agent.update" | "agent.remove" | "agent.profile.write"
    )
}

fn validate_machine_actor(
    state: &AppState,
    machine_id: &str,
    machine_actor_id: &str,
    requester_actor_id: Option<&str>,
    workspace_id: Option<&str>,
    if_inventory_revision: Option<u64>,
    require_revision: bool,
) -> Result<(), ErrorObject> {
    let actor = state.store.get_actor(machine_actor_id).ok_or_else(|| {
        ErrorObject::new(
            ErrorCode::APP_NOT_FOUND,
            format!("machine actor not found: {machine_actor_id}"),
        )
    })?;
    if actor.kind != ActorKind::Service {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor `{machine_actor_id}` is not a service actor"),
        ));
    }
    let meta = actor._meta.ok_or_else(|| {
        ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("machine actor `{machine_actor_id}` has no metadata"),
        )
    })?;
    let role = meta.get("role").and_then(Value::as_str);
    let source = meta.get("source").and_then(Value::as_str);
    let inventory_version = meta.get("inventoryVersion").and_then(Value::as_u64);
    let meta_machine_id = meta.get("machineId").and_then(Value::as_str);
    if role != Some("machine")
        || source != Some("daemon")
        || inventory_version != Some(2)
        || meta_machine_id != Some(machine_id)
    {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor `{machine_actor_id}` is not a daemon inventory v2 machine for `{machine_id}`"),
        ));
    }
    let has_command_capability = meta
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability.as_str() == Some("machine.command"))
        });
    if !has_command_capability {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("machine `{machine_id}` does not advertise machine.command capability"),
        ));
    }
    if let Some(expected_workspace_id) = meta
        .get("workspaceId")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
    {
        if workspace_id != Some(expected_workspace_id) {
            return Err(ErrorObject::new(
                ErrorCode::APP_INVALID_STATE,
                format!(
                    "machine `{machine_id}` belongs to workspace `{expected_workspace_id}`, not `{}`",
                    workspace_id.unwrap_or("<missing>")
                ),
            ));
        }
    }
    if let Some(expected_revision) = if_inventory_revision {
        let current_revision = meta.get("revision").and_then(Value::as_u64).unwrap_or(0);
        if expected_revision != current_revision {
            return Err(ErrorObject::new(
                ErrorCode::APP_CONFLICT,
                format!(
                    "machine inventory revision mismatch: expected {expected_revision}, current {current_revision}"
                ),
            ));
        }
    } else if require_revision {
        return Err(ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            "mutating machine command requires ifInventoryRevision",
        ));
    }
    if let Some(requester_actor_id) = requester_actor_id {
        if let Some(owner_actor_id) = meta
            .get("ownerActorId")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
        {
            if owner_actor_id != requester_actor_id {
                return Err(ErrorObject::new(
                    ErrorCode::APP_INVALID_STATE,
                    format!(
                        "actor `{requester_actor_id}` cannot command machine `{machine_id}` owned by `{owner_actor_id}`"
                    ),
                ));
            }
        }
    }
    Ok(())
}

// ---- actor / agent ----

fn actor_list(state: &AppState) -> HandlerResult {
    ok(ActorListResult {
        actors: state.store.list_actors(),
    })
}

/// Pre-register or update an actor row. `connection/open` already does an
/// implicit upsert, but it stamps `kind = Human` if the caller forgets to
/// pass `actorKind`. This dedicated RPC lets `joi daemon` (and any
/// other operator) declare an agent's full `Actor` (id, kind, display,
/// capabilities) before the agent ever opens its own connection — which
/// is what makes the "invite this agent into the channel, agent connects
/// later" flow possible.
fn actor_upsert(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ActorUpsertParams = parse_params(params)?;
    let actor = state.store.upsert_actor(p.actor).map_err(map_store_err)?;
    ok(ActorUpsertResult { actor })
}

fn actor_delete(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ActorDeleteParams = parse_params(params)?;
    let deleted = state
        .store
        .delete_actor(&p.actor_id)
        .map_err(map_store_err)?;
    ok(ActorDeleteResult { deleted })
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
    use crate::machine_commands::MachineCommandWaiters;
    use crate::scope_skills::ScopeSkills;
    use crate::store::Store;
    use crate::subscribe::{Connection, Subscriptions};
    use std::path::PathBuf;
    use tokio::sync::mpsc;

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

    fn fresh_state(name: &str) -> AppState {
        let root = temp_path(name);
        std::fs::create_dir_all(&root).expect("create root");
        let journal = Journal::open(root.join("journal.jsonl")).expect("open journal");
        let store = Store::open(journal).expect("open store");
        let subscriptions = Subscriptions::new();
        let artifacts = Arc::new(
            ArtifactStore::new(root.join("artifacts"), root.join("workspaces"))
                .expect("artifact store"),
        );
        let scope_skills = Arc::new(
            ScopeSkills::new(root.join("workspaces"), root.join("agents")).expect("scope skills"),
        );
        AppState {
            store,
            subscriptions,
            artifacts,
            scope_skills,
            machine_commands: MachineCommandWaiters::new(),
        }
    }

    async fn open_conn(state: &AppState, connection_id: &str, actor_id: &str) {
        let (tx, _rx) = mpsc::unbounded_channel();
        state.subscriptions.add_connection(Connection {
            id: connection_id.into(),
            actor_id: None,
            tx,
        });
        dispatch(
            state,
            connection_id,
            method::CONNECTION_OPEN,
            Some(json!({ "actorId": actor_id })),
        )
        .await
        .expect("connection/open");
    }

    async fn open_service_conn(
        state: &AppState,
        connection_id: &str,
        actor_id: &str,
    ) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        state.subscriptions.add_connection(Connection {
            id: connection_id.into(),
            actor_id: None,
            tx,
        });
        dispatch(
            state,
            connection_id,
            method::CONNECTION_OPEN,
            Some(json!({
                "actorId": actor_id,
                "actorKind": "service",
            })),
        )
        .await
        .expect("connection/open service");
        rx
    }

    fn append_channel_root(
        state: &AppState,
        channel_id: &str,
        actor_id: &str,
        text: &str,
    ) -> String {
        state
            .store
            .append_event(
                "content.add".into(),
                actor_id.into(),
                ScopeRef {
                    kind: ScopeKind::Channel,
                    id: channel_id.into(),
                },
                None,
                json!({ "contentType": "text/markdown", "text": text }),
                vec![],
                None,
            )
            .expect("append root event")
            .id
    }

    fn create_thread_under(
        state: &AppState,
        channel_id: &str,
        actor_id: &str,
        title: &str,
    ) -> proto::types::Thread {
        let root_event_id = append_channel_root(state, channel_id, actor_id, title);
        state
            .store
            .create_thread(channel_id.into(), title.into(), root_event_id)
            .expect("create thread")
    }

    #[test]
    fn artifact_publish_rejects_inaccessible_scope() {
        let state = fresh_state("artifact-publish-acl");
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
        let state = fresh_state("artifact-publish-missing-scope");

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

    #[tokio::test]
    async fn task_assignment_create_handoffs_in_canonical_thread() {
        let state = fresh_state("task-assignment-create");
        open_conn(&state, "conn_owner", "actor_owner").await;
        let channel = state
            .store
            .create_channel("tasks".into(), Some("actor_owner".into()))
            .expect("create channel");
        state
            .store
            .grant_channel(&channel.id, "actor_reviewer")
            .expect("grant reviewer");
        let source_event_id =
            append_channel_root(&state, &channel.id, "actor_owner", "write story");

        let created = dispatch(
            &state,
            "conn_owner",
            method::TASK_CREATE,
            Some(json!({
                "sourceEventId": source_event_id,
                "title": "write story",
                "ownerActorId": "actor_owner"
            })),
        )
        .await
        .expect("task/create");
        let created: TaskCreateResult = serde_json::from_value(created).unwrap();

        let assigned = dispatch(
            &state,
            "conn_owner",
            method::TASK_ASSIGNMENT_CREATE,
            Some(json!({
                "taskId": created.task.id,
                "toActorId": "actor_reviewer",
                "type": "review",
                "instruction": "review the story"
            })),
        )
        .await
        .expect("task/assignment.create");
        let assigned: TaskAssignmentCreateResult = serde_json::from_value(assigned).unwrap();

        assert_eq!(assigned.event.scope.kind, ScopeKind::Thread);
        assert_eq!(assigned.event.scope.id, created.task.canonical_thread_id);
        assert!(assigned.event.relations.iter().any(|relation| {
            matches!(relation.kind, RelationKind::HandsOffTo)
                && relation.target.kind == RefKind::Actor
                && relation.target.id == "actor_reviewer"
        }));
        assert!(assigned.event.relations.iter().any(|relation| {
            matches!(relation.kind, RelationKind::RelatesToTask)
                && relation.target.kind == RefKind::Task
                && relation.target.id == created.task.id
        }));
        let meta = assigned
            .event
            .payload
            .get("_meta")
            .and_then(|value| value.as_object())
            .expect("task meta");
        assert_eq!(
            meta.get("assignmentId").and_then(|value| value.as_str()),
            Some(assigned.assignment.id.as_str())
        );
    }

    #[tokio::test]
    async fn task_assignment_update_hands_terminal_result_back_to_assigner() {
        let state = fresh_state("task-assignment-return");
        open_conn(&state, "conn_owner", "actor_owner").await;
        open_conn(&state, "conn_delegate", "actor_delegate").await;
        open_conn(&state, "conn_reviewer", "actor_reviewer").await;
        let channel = state
            .store
            .create_channel("tasks".into(), Some("actor_owner".into()))
            .expect("create channel");
        state
            .store
            .grant_channel(&channel.id, "actor_delegate")
            .expect("grant delegate");
        state
            .store
            .grant_channel(&channel.id, "actor_reviewer")
            .expect("grant reviewer");
        let source_event_id =
            append_channel_root(&state, &channel.id, "actor_owner", "write story");

        let created = dispatch(
            &state,
            "conn_owner",
            method::TASK_CREATE,
            Some(json!({
                "sourceEventId": source_event_id,
                "title": "write story",
                "ownerActorId": "actor_owner"
            })),
        )
        .await
        .expect("task/create");
        let created: TaskCreateResult = serde_json::from_value(created).unwrap();

        let assigned = dispatch(
            &state,
            "conn_delegate",
            method::TASK_ASSIGNMENT_CREATE,
            Some(json!({
                "taskId": created.task.id,
                "toActorId": "actor_reviewer",
                "type": "review",
                "instruction": "review the story"
            })),
        )
        .await
        .expect("task/assignment.create");
        let assigned: TaskAssignmentCreateResult = serde_json::from_value(assigned).unwrap();
        let result_event = state
            .store
            .append_event(
                "content.add".into(),
                "actor_reviewer".into(),
                ScopeRef {
                    kind: ScopeKind::Thread,
                    id: created.task.canonical_thread_id.clone(),
                },
                None,
                json!({ "contentType": "text/markdown", "text": "looks good" }),
                vec![],
                None,
            )
            .expect("result event");

        let updated = dispatch(
            &state,
            "conn_reviewer",
            method::TASK_ASSIGNMENT_UPDATE,
            Some(json!({
                "assignmentId": assigned.assignment.id,
                "status": "completed",
                "resultEventId": result_event.id,
                "resultSummary": "looks good"
            })),
        )
        .await
        .expect("task/assignment.update");
        let updated: TaskAssignmentUpdateResult = serde_json::from_value(updated).unwrap();
        assert_eq!(updated.assignment.status, TaskAssignmentStatus::Completed);

        let (events, _) = state.store.read_scope(
            &ScopeRef {
                kind: ScopeKind::Thread,
                id: created.task.canonical_thread_id.clone(),
            },
            20,
            None,
        );
        let callback = events
            .iter()
            .rev()
            .find(|event| {
                event.actor_id == "actor_reviewer"
                    && event.relations.iter().any(|relation| {
                        matches!(relation.kind, RelationKind::HandsOffTo)
                            && relation.target.kind == RefKind::Actor
                            && relation.target.id == "actor_delegate"
                    })
                    && event
                        .payload
                        .get("text")
                        .and_then(|value| value.as_str())
                        .is_some_and(|text| text.contains("Handing task #"))
            })
            .expect("assignment return handoff event");
        assert!(callback.relations.iter().any(|relation| {
            matches!(relation.kind, RelationKind::RelatesToTask)
                && relation.target.kind == RefKind::Task
                && relation.target.id == created.task.id
        }));
        let meta = callback
            .payload
            .get("_meta")
            .and_then(|value| value.as_object())
            .expect("callback meta");
        assert_eq!(
            meta.get("assignmentStatus")
                .and_then(|value| value.as_str()),
            Some("completed")
        );
        assert_eq!(
            meta.get("assignmentId").and_then(|value| value.as_str()),
            Some(updated.assignment.id.as_str())
        );
        let delegate_deliveries =
            state
                .store
                .list_deliveries("actor_delegate", Some(DeliveryState::Pending), 20, None);
        assert!(delegate_deliveries
            .iter()
            .any(|delivery| delivery.event_id == callback.id));
        let owner_deliveries =
            state
                .store
                .list_deliveries("actor_owner", Some(DeliveryState::Pending), 20, None);
        assert!(!owner_deliveries
            .iter()
            .any(|delivery| delivery.event_id == callback.id));
    }

    #[test]
    fn artifact_read_supports_offset_chunks_and_preview_meta() {
        let state = fresh_state("artifact-read-offset");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");

        let published = artifact_publish(
            &state,
            Some(json!({
                "createdBy": "actor_owner",
                "scope": { "kind": "channel", "id": channel.id },
                "ingress": {
                    "kind": "file_bytes",
                    "name": "note.md",
                    "mediaType": "application/octet-stream",
                    "bytes": [48, 49, 50, 51, 52, 53]
                }
            })),
        )
        .expect("artifact publish");
        let published: ArtifactPublishResult =
            serde_json::from_value(published).expect("publish result");
        assert_eq!(published.artifact.media_type, "text/markdown");
        let meta = published.artifact._meta.as_ref().expect("artifact meta");
        assert_eq!(meta.get("attachmentKind"), Some(&json!("text")));
        assert_eq!(meta.get("previewable"), Some(&json!(true)));

        let first = artifact_read(
            &state,
            Some(json!({
                "artifactId": published.artifact.id,
                "offset": 2,
                "maxBytes": 3
            })),
        )
        .expect("artifact read");
        let first: ArtifactReadResult = serde_json::from_value(first).expect("read result");
        assert_eq!(first.offset, 2);
        assert_eq!(first.bytes, b"234".to_vec());
        assert!(first.truncated);
        assert_eq!(first.next_offset, Some(5));

        let second = artifact_read(
            &state,
            Some(json!({
                "artifactId": first.artifact_id,
                "offset": first.next_offset,
                "maxBytes": 3
            })),
        )
        .expect("artifact read tail");
        let second: ArtifactReadResult = serde_json::from_value(second).expect("read tail");
        assert_eq!(second.bytes, b"5".to_vec());
        assert!(!second.truncated);
        assert_eq!(second.next_offset, None);
    }

    #[test]
    fn artifact_publish_detects_image_attachment_meta_and_reads_chunks() {
        let state = fresh_state("artifact-image-attachment");
        let channel = state
            .store
            .create_channel("images".into(), Some("actor_owner".into()))
            .expect("create channel");

        let published = artifact_publish(
            &state,
            Some(json!({
                "createdBy": "actor_owner",
                "scope": { "kind": "channel", "id": channel.id },
                "ingress": {
                    "kind": "file_bytes",
                    "name": "avatar.bin",
                    "mediaType": "application/octet-stream",
                    "bytes": [137, 80, 78, 71, 13, 10, 26, 10, 0, 1, 2, 3, 4, 5]
                }
            })),
        )
        .expect("artifact publish");
        let published: ArtifactPublishResult =
            serde_json::from_value(published).expect("publish result");
        assert_eq!(published.artifact.media_type, "image/png");
        let meta = published.artifact._meta.as_ref().expect("artifact meta");
        assert_eq!(meta.get("attachmentKind"), Some(&json!("image")));
        assert_eq!(meta.get("previewable"), Some(&json!(true)));

        let chunk = artifact_read(
            &state,
            Some(json!({
                "artifactId": published.artifact.id,
                "offset": 8,
                "maxBytes": 4
            })),
        )
        .expect("artifact read image chunk");
        let chunk: ArtifactReadResult = serde_json::from_value(chunk).expect("read result");
        assert_eq!(chunk.offset, 8);
        assert_eq!(chunk.bytes, vec![0, 1, 2, 3]);
        assert!(chunk.truncated);
        assert_eq!(chunk.next_offset, Some(12));

        let tail = artifact_read(
            &state,
            Some(json!({
                "artifactId": chunk.artifact_id,
                "offset": chunk.next_offset,
                "maxBytes": 4
            })),
        )
        .expect("artifact read image tail");
        let tail: ArtifactReadResult = serde_json::from_value(tail).expect("tail result");
        assert_eq!(tail.bytes, vec![4, 5]);
        assert!(!tail.truncated);
        assert_eq!(tail.next_offset, None);
    }

    #[tokio::test]
    async fn attachment_exchange_between_human_and_agent_round_trips() {
        let state = fresh_state("attachment-exchange");
        open_conn(&state, "conn_human", "actor_human").await;
        open_conn(&state, "conn_agent", "actor_agent").await;

        dispatch(
            &state,
            "conn_human",
            method::ACTOR_UPSERT,
            Some(json!({
                "actor": {
                    "id": "actor_agent",
                    "kind": "agent",
                    "displayName": "Attachment Agent"
                }
            })),
        )
        .await
        .expect("actor/upsert");

        let channel = dispatch(
            &state,
            "conn_human",
            method::CHANNEL_CREATE,
            Some(json!({
                "title": "attachment exchange",
                "actorId": "actor_human"
            })),
        )
        .await
        .expect("channel/create");
        let channel: ChannelCreateResult = serde_json::from_value(channel).expect("channel result");

        dispatch(
            &state,
            "conn_human",
            method::CHANNEL_INVITE,
            Some(json!({
                "channelId": channel.channel.id,
                "actorId": "actor_agent"
            })),
        )
        .await
        .expect("channel/invite");

        let root = dispatch(
            &state,
            "conn_human",
            method::EVENT_APPEND,
            Some(json!({
                "event": {
                    "type": "content.add",
                    "actorId": "actor_human",
                    "scope": { "kind": "channel", "id": channel.channel.id },
                    "payload": {
                        "contentType": "text/markdown",
                        "text": "root for attachment exchange"
                    }
                }
            })),
        )
        .await
        .expect("root event");
        let root: EventAppendResult = serde_json::from_value(root).expect("root result");

        let thread = dispatch(
            &state,
            "conn_human",
            method::THREAD_CREATE,
            Some(json!({
                "channelId": channel.channel.id,
                "rootEventId": root.event.id,
                "title": "attachment thread"
            })),
        )
        .await
        .expect("thread/create");
        let thread: ThreadCreateResult = serde_json::from_value(thread).expect("thread result");
        let thread_scope = json!({ "kind": "thread", "id": thread.thread.id });
        let human_payload = b"human upload payload line 1\nline 2\n".to_vec();

        let human_artifact = artifact_publish(
            &state,
            Some(json!({
                "createdBy": "actor_human",
                "scope": thread_scope.clone(),
                "ingress": {
                    "kind": "file_bytes",
                    "name": "human.txt",
                    "mediaType": "text/plain",
                    "bytes": human_payload.clone()
                }
            })),
        )
        .expect("human artifact publish");
        let human_artifact: ArtifactPublishResult =
            serde_json::from_value(human_artifact).expect("human artifact result");

        let human_event = dispatch(
            &state,
            "conn_human",
            method::EVENT_APPEND,
            Some(json!({
                "event": {
                    "type": "content.add",
                    "actorId": "actor_human",
                    "scope": thread_scope.clone(),
                    "payload": {
                        "contentType": "text/markdown",
                        "text": "human attached file"
                    },
                    "relations": [
                        {
                            "kind": "attaches_artifact",
                            "target": { "kind": "artifact", "id": human_artifact.artifact.id }
                        },
                        {
                            "kind": "hands_off_to",
                            "target": { "kind": "actor", "id": "actor_agent" }
                        }
                    ]
                }
            })),
        )
        .await
        .expect("human event append");
        let human_event: EventAppendResult =
            serde_json::from_value(human_event).expect("human event result");
        assert!(human_event.event.relations.iter().any(|relation| {
            relation.kind == RelationKind::AttachesArtifact
                && relation.target.id == human_artifact.artifact.id
        }));
        assert!(human_event.event.relations.iter().any(|relation| {
            relation.kind == RelationKind::HandsOffTo && relation.target.id == "actor_agent"
        }));

        let offset_read = artifact_read(
            &state,
            Some(json!({
                "artifactId": human_artifact.artifact.id,
                "offset": 6,
                "maxBytes": 6
            })),
        )
        .expect("agent offset read");
        let offset_read: ArtifactReadResult =
            serde_json::from_value(offset_read).expect("offset read result");
        assert_eq!(offset_read.offset, 6);
        assert_eq!(offset_read.content, "upload");
        assert!(offset_read.truncated);
        assert_eq!(offset_read.next_offset, Some(12));

        let mut downloaded = Vec::new();
        let mut offset = 0_u64;
        loop {
            let chunk = artifact_read(
                &state,
                Some(json!({
                    "artifactId": human_artifact.artifact.id,
                    "offset": offset,
                    "maxBytes": 5
                })),
            )
            .expect("chunked artifact read");
            let chunk: ArtifactReadResult = serde_json::from_value(chunk).expect("chunk result");
            downloaded.extend_from_slice(&chunk.bytes);
            if !chunk.truncated {
                break;
            }
            offset = chunk.next_offset.expect("next offset");
        }
        assert_eq!(downloaded, human_payload);

        let agent_payload = b"agent file from actor_agent\n".to_vec();
        let agent_artifact = artifact_publish(
            &state,
            Some(json!({
                "createdBy": "actor_agent",
                "scope": thread_scope.clone(),
                "ingress": {
                    "kind": "file_bytes",
                    "name": "agent.txt",
                    "mediaType": "text/plain",
                    "bytes": agent_payload
                }
            })),
        )
        .expect("agent artifact publish");
        let agent_artifact: ArtifactPublishResult =
            serde_json::from_value(agent_artifact).expect("agent artifact result");

        let agent_event = dispatch(
            &state,
            "conn_agent",
            method::EVENT_APPEND,
            Some(json!({
                "event": {
                    "type": "content.add",
                    "actorId": "actor_agent",
                    "scope": thread_scope.clone(),
                    "payload": {
                        "contentType": "text/markdown",
                        "text": "agent sent file"
                    },
                    "relations": [
                        {
                            "kind": "attaches_artifact",
                            "target": { "kind": "artifact", "id": agent_artifact.artifact.id }
                        }
                    ]
                }
            })),
        )
        .await
        .expect("agent event append");
        let agent_event: EventAppendResult =
            serde_json::from_value(agent_event).expect("agent event result");
        assert!(agent_event.event.relations.iter().any(|relation| {
            relation.kind == RelationKind::AttachesArtifact
                && relation.target.id == agent_artifact.artifact.id
        }));

        let history = dispatch(
            &state,
            "conn_human",
            method::SCOPE_READ,
            Some(json!({
                "scope": thread_scope,
                "limit": 100
            })),
        )
        .await
        .expect("scope/read");
        let history: ScopeReadResult = serde_json::from_value(history).expect("history result");
        assert!(history
            .events
            .iter()
            .any(|event| event.id == human_event.event.id));
        assert!(history
            .events
            .iter()
            .any(|event| event.id == agent_event.event.id));

        let agent_read = artifact_read(
            &state,
            Some(json!({
                "artifactId": agent_artifact.artifact.id,
                "maxBytes": 200
            })),
        )
        .expect("human reads agent artifact");
        let agent_read: ArtifactReadResult =
            serde_json::from_value(agent_read).expect("agent artifact read result");
        assert!(agent_read.content.contains("agent file from actor_agent"));
    }

    #[tokio::test]
    async fn channel_delete_refuses_non_member() {
        let state = fresh_state("channel-delete-non-member");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        create_thread_under(&state, &channel.id, "actor_owner", "child");
        open_conn(&state, "conn_intruder", "actor_intruder").await;

        let err = dispatch(
            &state,
            "conn_intruder",
            method::CHANNEL_DELETE,
            Some(json!({ "channelId": channel.id, "cascade": true })),
        )
        .await
        .expect_err("delete should be denied");

        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);
        assert!(state.store.get_channel(&channel.id).is_some());
        assert_eq!(state.store.list_threads(Some(&channel.id)).len(), 1);
    }

    #[tokio::test]
    async fn channel_delete_allows_member_cascade() {
        let state = fresh_state("channel-delete-member");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        create_thread_under(&state, &channel.id, "actor_owner", "child");
        open_conn(&state, "conn_owner", "actor_owner").await;

        let value = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_DELETE,
            Some(json!({ "channelId": channel.id, "cascade": true })),
        )
        .await
        .expect("delete should succeed");
        let result: ChannelDeleteResult = serde_json::from_value(value).expect("delete result");

        assert!(result.deleted);
        assert_eq!(result.deleted_threads, 1);
        assert!(state.store.get_channel(&channel.id).is_none());
        assert!(state.store.list_threads(Some(&channel.id)).is_empty());
    }

    #[tokio::test]
    async fn thread_create_refuses_non_member_in_private_channel() {
        let state = fresh_state("auto");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        let root_event_id = append_channel_root(&state, &channel.id, "actor_owner", "sneaky");
        open_conn(&state, "conn_intruder", "actor_intruder").await;

        let err = dispatch(
            &state,
            "conn_intruder",
            method::THREAD_CREATE,
            Some(
                json!({ "channelId": channel.id, "rootEventId": root_event_id, "title": "sneaky" }),
            ),
        )
        .await
        .expect_err("thread/create should be denied");

        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);
        assert!(state.store.list_threads(Some(&channel.id)).is_empty());
    }

    #[tokio::test]
    async fn thread_create_allows_member() {
        let state = fresh_state("auto");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        let root_event_id = append_channel_root(&state, &channel.id, "actor_owner", "ok");
        open_conn(&state, "conn_owner", "actor_owner").await;

        let value = dispatch(
            &state,
            "conn_owner",
            method::THREAD_CREATE,
            Some(json!({ "channelId": channel.id, "rootEventId": root_event_id, "title": "ok" })),
        )
        .await
        .expect("thread/create should succeed");
        let result: ThreadCreateResult = serde_json::from_value(value).expect("create result");

        assert_eq!(result.thread.title, "ok");
        assert_eq!(state.store.list_threads(Some(&channel.id)).len(), 1);
    }

    #[tokio::test]
    async fn thread_list_filters_to_visible_channels() {
        let state = fresh_state("auto");
        // alice is in private_a; intruder is not. private_b is invisible to both.
        let private_a = state
            .store
            .create_channel("a".into(), Some("actor_alice".into()))
            .expect("create a");
        let _t_a = create_thread_under(&state, &private_a.id, "actor_alice", "in-a");
        let private_b = state
            .store
            .create_channel("b".into(), Some("actor_bob".into()))
            .expect("create b");
        let _t_b = create_thread_under(&state, &private_b.id, "actor_bob", "in-b");

        open_conn(&state, "conn_alice", "actor_alice").await;
        let value = dispatch(&state, "conn_alice", method::THREAD_LIST, None)
            .await
            .expect("thread/list");
        let result: ThreadListResult = serde_json::from_value(value).expect("list result");
        let titles: Vec<&str> = result.threads.iter().map(|t| t.title.as_str()).collect();
        assert!(
            titles.contains(&"in-a") && !titles.contains(&"in-b"),
            "alice must see in-a but not in-b; got {titles:?}",
        );
    }

    #[tokio::test]
    async fn thread_delete_refuses_non_member() {
        let state = fresh_state("auto");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        let thread = create_thread_under(&state, &channel.id, "actor_owner", "doomed");
        open_conn(&state, "conn_intruder", "actor_intruder").await;

        let err = dispatch(
            &state,
            "conn_intruder",
            method::THREAD_DELETE,
            Some(json!({ "threadId": thread.id })),
        )
        .await
        .expect_err("thread/delete should be denied");

        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);
        assert!(state.store.get_thread(&thread.id).is_some());
    }

    #[tokio::test]
    async fn thread_update_refuses_non_member() {
        let state = fresh_state("auto");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        let thread = create_thread_under(&state, &channel.id, "actor_owner", "orig");
        open_conn(&state, "conn_intruder", "actor_intruder").await;

        let err = dispatch(
            &state,
            "conn_intruder",
            method::THREAD_UPDATE,
            Some(json!({ "threadId": thread.id, "title": "renamed" })),
        )
        .await
        .expect_err("thread/update should be denied");

        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);
        assert_eq!(state.store.get_thread(&thread.id).unwrap().title, "orig");
    }

    // ---- delivery/list (§9.2 durable inbox) ----

    fn handoff_to(actor_id: &str) -> Relation {
        Relation {
            kind: RelationKind::HandsOffTo,
            target: Ref {
                kind: RefKind::Actor,
                id: actor_id.into(),
                _meta: None,
            },
            _meta: None,
        }
    }

    fn append_handoff(state: &AppState, channel_id: &str, from: &str, to: &str) -> String {
        state
            .store
            .append_event(
                "content.add".into(),
                from.into(),
                ScopeRef {
                    kind: ScopeKind::Channel,
                    id: channel_id.into(),
                },
                None,
                json!({"text": "hi"}),
                vec![handoff_to(to)],
                None,
            )
            .expect("append handoff")
            .id
    }

    #[tokio::test]
    async fn delivery_list_returns_pending_for_caller_inbox() {
        // Baseline: an event with hands_off_to writes a Pending delivery row
        // for the target. The bound caller can list it back, and the result
        // includes the inline event payload (no follow-up scope/read needed).
        let state = fresh_state("auto");
        let ch = state.store.create_channel("c".into(), None).expect("ch");
        state
            .store
            .grant_channel(&ch.id, "svc_writer")
            .expect("g w");
        state
            .store
            .grant_channel(&ch.id, "actor_target")
            .expect("g t");
        let event_id = append_handoff(&state, &ch.id, "svc_writer", "actor_target");
        open_conn(&state, "conn_t", "actor_target").await;

        let value = dispatch(
            &state,
            "conn_t",
            method::DELIVERY_LIST,
            Some(json!({ "actorId": "actor_target", "state": "pending" })),
        )
        .await
        .expect("delivery/list ok");
        let res: DeliveryListResult = serde_json::from_value(value).expect("decode");

        assert_eq!(res.deliveries.len(), 1);
        let entry = &res.deliveries[0];
        assert_eq!(entry.delivery.event_id, event_id);
        assert_eq!(entry.delivery.actor_id, "actor_target");
        assert!(matches!(entry.delivery.state, DeliveryState::Pending));
        let event = entry.event.as_ref().expect("inline event payload");
        assert_eq!(event.id, event_id);
        assert!(res.next_cursor.is_none(), "single page");
    }

    #[tokio::test]
    async fn delivery_list_filters_by_state_after_receipt() {
        // record_receipt advances Pending -> Delivered. After that the
        // pending filter must drop the row, and the delivered filter picks
        // it up. Proves state is a real index, not a noop.
        let state = fresh_state("auto");
        let ch = state.store.create_channel("c".into(), None).expect("ch");
        state.store.grant_channel(&ch.id, "svc_writer").expect("gw");
        state
            .store
            .grant_channel(&ch.id, "actor_target")
            .expect("gt");
        let event_id = append_handoff(&state, &ch.id, "svc_writer", "actor_target");
        state
            .store
            .record_receipt(
                event_id.clone(),
                "actor_target".into(),
                ReceiptKind::Completed,
            )
            .expect("ack");
        open_conn(&state, "conn_t", "actor_target").await;

        let pending: DeliveryListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::DELIVERY_LIST,
                Some(json!({ "actorId": "actor_target", "state": "pending" })),
            )
            .await
            .expect("pending ok"),
        )
        .expect("decode");
        assert!(
            pending.deliveries.is_empty(),
            "pending filter must drop acked row"
        );

        let delivered: DeliveryListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::DELIVERY_LIST,
                Some(json!({ "actorId": "actor_target", "state": "delivered" })),
            )
            .await
            .expect("delivered ok"),
        )
        .expect("decode");
        assert_eq!(delivered.deliveries.len(), 1);
        assert_eq!(delivered.deliveries[0].delivery.event_id, event_id);
    }

    #[tokio::test]
    async fn delivery_list_paginates_with_cursor() {
        // Three deliveries, limit=2. First page returns 2 + cursor; second
        // page returns the remaining 1 with no cursor. Together they cover
        // every event id exactly once — order-independent so the test is
        // resilient to clock-resolution ties (sort tiebreak is uuid event_id).
        let state = fresh_state("auto");
        let ch = state.store.create_channel("c".into(), None).expect("ch");
        state.store.grant_channel(&ch.id, "svc_writer").expect("gw");
        state
            .store
            .grant_channel(&ch.id, "actor_target")
            .expect("gt");
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(append_handoff(&state, &ch.id, "svc_writer", "actor_target"));
        }
        open_conn(&state, "conn_t", "actor_target").await;

        let page1: DeliveryListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::DELIVERY_LIST,
                Some(json!({ "actorId": "actor_target", "limit": 2 })),
            )
            .await
            .expect("p1"),
        )
        .expect("decode");
        assert_eq!(page1.deliveries.len(), 2);
        let cursor = page1.next_cursor.expect("more rows -> cursor present");

        let page2: DeliveryListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::DELIVERY_LIST,
                Some(json!({ "actorId": "actor_target", "limit": 2, "cursor": cursor })),
            )
            .await
            .expect("p2"),
        )
        .expect("decode");
        assert_eq!(page2.deliveries.len(), 1);
        assert!(page2.next_cursor.is_none(), "drained -> no cursor");

        let mut seen: Vec<String> = page1
            .deliveries
            .iter()
            .chain(page2.deliveries.iter())
            .map(|e| e.delivery.event_id.clone())
            .collect();
        seen.sort();
        let mut want = ids;
        want.sort();
        assert_eq!(seen, want, "pages must cover every id exactly once");
    }

    #[tokio::test]
    async fn delivery_list_refuses_other_actors_inbox() {
        // Reading another actor's inbox would let any connected client snoop
        // every directed event in the system. The handler refuses with the
        // same error class as "no bound actor".
        let state = fresh_state("auto");
        let ch = state.store.create_channel("c".into(), None).expect("ch");
        state.store.grant_channel(&ch.id, "svc_writer").expect("gw");
        state
            .store
            .grant_channel(&ch.id, "actor_target")
            .expect("gt");
        append_handoff(&state, &ch.id, "svc_writer", "actor_target");
        open_conn(&state, "conn_intruder", "actor_intruder").await;

        let err = dispatch(
            &state,
            "conn_intruder",
            method::DELIVERY_LIST,
            Some(json!({ "actorId": "actor_target" })),
        )
        .await
        .expect_err("must refuse cross-actor read");
        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);
        assert!(
            err.message.contains("inbox"),
            "error message should mention inbox: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn delivery_list_requires_bound_actor() {
        // A connection that never called connection/open has no actor
        // identity; delivery/list cannot pick a default and must refuse.
        let state = fresh_state("auto");
        let (tx, _rx) = mpsc::unbounded_channel();
        state.subscriptions.add_connection(Connection {
            id: "conn_anon".into(),
            actor_id: None,
            tx,
        });

        let err = dispatch(
            &state,
            "conn_anon",
            method::DELIVERY_LIST,
            Some(json!({ "actorId": "actor_target" })),
        )
        .await
        .expect_err("must refuse anonymous caller");
        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);
    }

    #[tokio::test]
    async fn machine_command_create_persists_for_daemon_pull() {
        let state = fresh_state("machine-command-durable");
        open_conn(&state, "conn_human", "actor_human").await;
        dispatch(
            &state,
            "conn_human",
            method::ACTOR_UPSERT,
            Some(json!({
                "actor": {
                    "id": "actor_service_machine_remote",
                    "kind": "service",
                    "displayName": "Remote Machine",
                    "_meta": {
                        "role": "machine",
                        "source": "daemon",
                        "inventoryVersion": 2,
                        "machineId": "machine_remote",
                        "workspaceId": "workspace_a",
                        "ownerActorId": "actor_human",
                        "capabilities": ["inventory.read", "machine.command"],
                        "revision": 9
                    }
                }
            })),
        )
        .await
        .expect("actor/upsert machine");

        let value = dispatch(
            &state,
            "conn_human",
            method::MACHINE_COMMAND_CREATE,
            Some(json!({
                "machineId": "machine_remote",
                "machineActorId": "actor_service_machine_remote",
                "workspaceId": "workspace_a",
                "operation": "agent.remove",
                "payload": { "op": "agent.remove", "actorId": "actor_agent" },
                "ifInventoryRevision": 9
            })),
        )
        .await
        .expect("machine/command.create");
        let created: MachineCommandCreateResult =
            serde_json::from_value(value).expect("create result");
        assert!(!created.delivered);
        assert_eq!(created.command.status, MachineCommandStatus::Queued);

        open_service_conn(&state, "conn_machine", "actor_service_machine_remote").await;
        dispatch(
            &state,
            "conn_machine",
            method::ACTOR_UPSERT,
            Some(json!({
                "actor": {
                    "id": "actor_service_machine_remote",
                    "kind": "service",
                    "displayName": "Remote Machine",
                    "_meta": {
                        "role": "machine",
                        "source": "daemon",
                        "inventoryVersion": 2,
                        "machineId": "machine_remote",
                        "workspaceId": "workspace_a",
                        "ownerActorId": "actor_human",
                        "capabilities": ["inventory.read", "machine.command"],
                        "revision": 9
                    }
                }
            })),
        )
        .await
        .expect("actor/upsert machine again");

        let value = dispatch(
            &state,
            "conn_machine",
            method::MACHINE_COMMAND_LIST,
            Some(json!({
                "machineId": "machine_remote",
                "statuses": ["queued"]
            })),
        )
        .await
        .expect("machine/command.list");
        let listed: MachineCommandListResult = serde_json::from_value(value).expect("list result");
        assert_eq!(listed.commands.len(), 1);
        assert_eq!(listed.commands[0].command_id, created.command.command_id);

        let value = dispatch(
            &state,
            "conn_machine",
            method::MACHINE_COMMAND_ACK,
            Some(json!({
                "commandId": created.command.command_id,
                "machineId": "machine_remote",
                "machineActorId": "actor_service_machine_remote"
            })),
        )
        .await
        .expect("machine/command.ack");
        let acked: MachineCommandAckResult = serde_json::from_value(value).expect("ack result");
        assert_eq!(acked.command.status, MachineCommandStatus::Running);

        dispatch(
            &state,
            "conn_machine",
            method::MACHINE_COMMAND_RESULT,
            Some(json!({
                "commandId": created.command.command_id,
                "machineId": "machine_remote",
                "machineActorId": "actor_service_machine_remote",
                "ok": true,
                "output": { "done": true }
            })),
        )
        .await
        .expect("machine/command.result");

        let value = dispatch(
            &state,
            "conn_human",
            method::MACHINE_COMMAND_GET,
            Some(json!({ "commandId": created.command.command_id })),
        )
        .await
        .expect("machine/command.get");
        let fetched: MachineCommandGetResult = serde_json::from_value(value).expect("get result");
        assert_eq!(fetched.command.status, MachineCommandStatus::Succeeded);
        assert_eq!(
            fetched
                .command
                .output
                .and_then(|value| value.get("done").and_then(Value::as_bool).map(bool::from)),
            Some(true)
        );
    }

    #[tokio::test]
    async fn machine_command_routes_to_service_and_waits_for_result() {
        let state = fresh_state("machine-command-routes");
        open_conn(&state, "conn_human", "actor_human").await;
        let mut service_rx =
            open_service_conn(&state, "conn_machine", "actor_service_machine_remote").await;
        dispatch(
            &state,
            "conn_machine",
            method::ACTOR_UPSERT,
            Some(json!({
                "actor": {
                    "id": "actor_service_machine_remote",
                    "kind": "service",
                    "displayName": "Remote Machine",
                    "_meta": {
                        "role": "machine",
                        "source": "daemon",
                        "inventoryVersion": 2,
                        "machineId": "machine_remote",
                        "capabilities": ["machine.command"],
                        "revision": 7
                    }
                }
            })),
        )
        .await
        .expect("actor/upsert machine");

        let state_for_command = state.clone();
        let command = tokio::spawn(async move {
            dispatch(
                &state_for_command,
                "conn_human",
                method::MACHINE_COMMAND,
                Some(json!({
                    "machineId": "machine_remote",
                    "ifInventoryRevision": 7,
                    "command": { "op": "agent.remove", "actorId": "actor_agent" },
                    "timeoutMs": 5_000
                })),
            )
            .await
        });

        let frame = service_rx
            .recv()
            .await
            .expect("machine command notification");
        let notification: proto::Notification =
            serde_json::from_str(&frame).expect("notification frame");
        assert_eq!(notification.method, method::MACHINE_COMMAND_NOTIFY);
        let params = notification.params.expect("notification params");
        let command_id = params
            .get("commandId")
            .and_then(Value::as_str)
            .expect("commandId");
        assert_eq!(
            params.get("requestedBy").and_then(Value::as_str),
            Some("actor_human")
        );

        dispatch(
            &state,
            "conn_machine",
            method::MACHINE_COMMAND_RESULT,
            Some(json!({
                "commandId": command_id,
                "machineId": "machine_remote",
                "machineActorId": "actor_service_machine_remote",
                "ok": true,
                "output": { "done": true }
            })),
        )
        .await
        .expect("machine/command.result");

        let value = command
            .await
            .expect("join")
            .expect("machine command result");
        let result: MachineCommandResponse = serde_json::from_value(value).expect("decode result");
        assert!(result.ok);
        assert_eq!(
            result.output.get("done").and_then(Value::as_bool),
            Some(true)
        );
    }
}
