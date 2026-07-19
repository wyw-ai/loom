use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use proto::methods::*;
use proto::types::*;
use proto::{ErrorCode, ErrorObject};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::state::AppState;
use crate::store::{Store, StoreError};

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
        method::CHANNEL_CREATE => channel_create(state, connection_id, params),
        method::CHANNEL_LIST => channel_list(state, connection_id),
        method::CHANNEL_UPDATE => channel_update(state, params),
        method::CHANNEL_DELETE => channel_delete(state, connection_id, params),
        method::CHANNEL_INVITE => channel_invite(state, connection_id, params),
        method::CHANNEL_REVOKE => channel_revoke(state, connection_id, params),
        method::CHANNEL_MEMBERS => channel_members(state, connection_id, params),
        method::CHANNEL_MEMBER_CONFIG_GET => {
            channel_member_config_get(state, connection_id, params)
        }
        method::CHANNEL_MEMBER_CONFIG_LIST => {
            channel_member_config_list(state, connection_id, params)
        }
        method::CHANNEL_MEMBER_CONFIG_SET => {
            channel_member_config_set(state, connection_id, params)
        }
        method::CHANNEL_MEMBER_CONFIG_CLEAR => {
            channel_member_config_clear(state, connection_id, params)
        }
        method::CHANNEL_SET_INSTRUCTION => channel_set_instruction(state, connection_id, params),
        method::CHANNEL_GET_INSTRUCTION => channel_get_instruction(state, connection_id, params),
        method::CHANNEL_CLEAR_INSTRUCTION => {
            channel_clear_instruction(state, connection_id, params)
        }
        method::THREAD_CREATE => thread_create(state, connection_id, params),
        method::THREAD_LIST => thread_list(state, connection_id, params),
        method::THREAD_UPDATE => thread_update(state, connection_id, params),
        method::THREAD_ARCHIVE => thread_archive(state, connection_id, params),
        method::THREAD_DELETE => thread_delete(state, connection_id, params),
        method::THREAD_FOLLOW => thread_follow(state, connection_id, params),
        method::THREAD_UNFOLLOW => thread_unfollow(state, connection_id, params),
        method::THREAD_SET_INSTRUCTION => thread_set_instruction(state, connection_id, params),
        method::THREAD_GET_INSTRUCTION => thread_get_instruction(state, connection_id, params),
        method::THREAD_CLEAR_INSTRUCTION => thread_clear_instruction(state, connection_id, params),
        method::TASK_CREATE => task_create(state, connection_id, params),
        method::TASK_GET => task_get(state, connection_id, params),
        method::TASK_LIST => task_list(state, connection_id, params),
        method::TASK_UPDATE => task_update(state, connection_id, params),
        method::TASK_CLAIM => task_claim(state, connection_id, params),
        method::TASK_ASSIGN => task_assign(state, connection_id, params),
        method::TASK_COMPLETE => task_complete(state, connection_id, params),
        method::TASK_REOPEN => task_reopen(state, connection_id, params),
        method::TASK_CANCEL => task_cancel(state, connection_id, params),
        method::TASK_REF_ATTACH => task_ref_attach(state, connection_id, params),
        method::TASK_REF_FIND => task_ref_find(state, connection_id, params),
        method::TASK_REF_LIST => task_ref_list(state, connection_id, params),
        method::TASK_ARTIFACT_ATTACH => task_artifact_attach(state, connection_id, params),
        method::TASK_ARTIFACT_ACTIVATE => task_artifact_activate(state, connection_id, params),
        method::TASK_ARTIFACT_LIST => task_artifact_list(state, connection_id, params),
        method::TASK_FACT_APPEND => task_fact_append(state, connection_id, params),
        method::TASK_FACT_LIST => task_fact_list(state, connection_id, params),
        method::TASK_PROJECTION_PUT => task_projection_put(state, connection_id, params),
        method::TASK_PROJECTION_GET => task_projection_get(state, connection_id, params),
        method::TASK_PROJECTION_LIST => task_projection_list(state, connection_id, params),
        method::TASK_ASSIGNMENT_CREATE => {
            task_assignment_create(state, connection_id, params).await
        }
        method::TASK_ASSIGNMENT_UPDATE => task_assignment_update(state, connection_id, params),
        method::TASK_ASSIGNMENT_CONTEXT => task_assignment_context(state, connection_id, params),
        method::TASK_ASSIGNMENT_PREFLIGHT => {
            task_assignment_preflight(state, connection_id, params)
        }
        method::TASK_CHANGE_LIST => task_change_list(state, connection_id, params),
        method::TASK_CHANGE_ACK => task_change_ack(state, connection_id, params),
        method::TASK_WORKSPACE_LEASE_ACQUIRE => {
            workspace_lease_acquire(state, connection_id, params)
        }
        method::TASK_WORKSPACE_LEASE_RELEASE => {
            workspace_lease_release(state, connection_id, params)
        }
        method::TASK_WORKSPACE_LEASE_LIST => workspace_lease_list(state, connection_id, params),
        method::RUN_OPEN => run_open(state, connection_id, params),
        method::RUN_APPEND => run_append(state, connection_id, params),
        method::RUN_CLOSE => run_close(state, connection_id, params),
        method::RUN_CANCEL => run_cancel(state, connection_id, params),
        method::COORDINATION_PROPOSE => coordination_propose(state, connection_id, params),
        method::COORDINATION_COMMIT => coordination_commit(state, connection_id, params),
        method::COORDINATION_RESPOND => coordination_respond(state, connection_id, params),
        method::COORDINATION_STEP => coordination_step(state, connection_id, params),
        method::COORDINATION_SKIP => coordination_skip(state, connection_id, params),
        method::COORDINATION_REASSIGN => coordination_reassign(state, connection_id, params),
        method::AGENT_CONFIG_PUBLISH => agent_config_publish(state, connection_id, params),
        method::AGENT_CONFIG_ACTIVATE => agent_config_activate(state, connection_id, params),
        method::MESSAGE_SEND => message_send(state, connection_id, params),
        method::MESSAGE_LIST => message_list(state, connection_id, params),
        method::MESSAGE_READ => message_read(state, connection_id, params),
        method::MESSAGE_REACTION_TOGGLE => message_reaction_toggle(state, connection_id, params),
        method::MESSAGE_SEARCH => message_search(state, connection_id, params),
        method::ARTIFACT_PUBLISH => artifact_publish(state, params),
        method::ARTIFACT_GET => artifact_get(state, params),
        method::ARTIFACT_READ => artifact_read(state, params),
        method::REMINDER_SCHEDULE => reminder_schedule(state, params),
        method::REMINDER_LIST => reminder_list(state, params),
        method::REMINDER_CANCEL => reminder_cancel(state, params),
        method::REMINDER_SNOOZE => reminder_snooze(state, params),
        method::REMINDER_UPDATE => reminder_update(state, params),
        method::INBOX_LIST => inbox_list(state, connection_id, params),
        method::DELIVERY_ACK => delivery_ack(state, connection_id, params),
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
        method::ACTOR_GROUP_CREATE => actor_group_create(state, connection_id, params),
        method::ACTOR_GROUP_LIST => actor_group_list(state, connection_id, params),
        method::ACTOR_GROUP_ADD_MEMBER => actor_group_add_member(state, connection_id, params),
        method::ACTOR_GROUP_REMOVE_MEMBER => {
            actor_group_remove_member(state, connection_id, params)
        }
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
    let channel = if p.topic.trim().is_empty() {
        state.store.create_channel(p.title, creator)
    } else {
        state
            .store
            .create_channel_with_topic(p.title, p.topic, creator)
    }
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

fn ensure_channel_config_reader(
    state: &AppState,
    connection_id: &str,
    channel_id: &str,
) -> Result<String, ErrorObject> {
    let caller = caller_actor(state, connection_id)?;
    if state.store.get_channel(channel_id).is_none() {
        return Err(ErrorObject::new(ErrorCode::APP_NOT_FOUND, "channel"));
    }
    if !state.store.is_channel_member(channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot read member config for channel {channel_id}"),
        ));
    }
    Ok(caller)
}

fn channel_member_config_get(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ChannelMemberConfigGetParams = parse_params(params)?;
    ensure_channel_config_reader(state, connection_id, &p.channel_id)?;
    ok(ChannelMemberConfigGetResult {
        config: state
            .store
            .get_channel_member_config(&p.channel_id, &p.actor_id),
    })
}

fn channel_member_config_list(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ChannelMemberConfigListParams = parse_params(params)?;
    ensure_channel_config_reader(state, connection_id, &p.channel_id)?;
    let configs = state
        .store
        .list_channel_member_configs(&p.channel_id)
        .map_err(map_store_err)?;
    ok(ChannelMemberConfigListResult { configs })
}

fn channel_member_config_set(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ChannelMemberConfigSetParams = parse_params(params)?;
    let caller = ensure_channel_config_reader(state, connection_id, &p.channel_id)?;
    if p.workspace_dir.trim().is_empty() {
        return Err(ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            "workspaceDir cannot be empty",
        ));
    }
    if p.workspace_dir.contains('\0') {
        return Err(ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            "workspaceDir cannot contain NUL bytes",
        ));
    }
    if state.store.get_actor(&p.actor_id).is_none() {
        return Err(ErrorObject::new(
            ErrorCode::APP_NOT_FOUND,
            format!("actor {}", p.actor_id),
        ));
    }
    let config = state
        .store
        .set_channel_member_workspace_dir(&p.channel_id, &p.actor_id, p.workspace_dir)
        .map_err(map_store_err)?;
    tracing::info!(
        channel = %p.channel_id,
        actor = %p.actor_id,
        caller = %caller,
        "channel member workspace config updated"
    );
    ok(ChannelMemberConfigSetResult { config })
}

fn channel_member_config_clear(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ChannelMemberConfigClearParams = parse_params(params)?;
    let caller = ensure_channel_config_reader(state, connection_id, &p.channel_id)?;
    let cleared = state
        .store
        .clear_channel_member_config(&p.channel_id, &p.actor_id)
        .map_err(map_store_err)?;
    tracing::info!(
        channel = %p.channel_id,
        actor = %p.actor_id,
        caller = %caller,
        cleared,
        "channel member workspace config cleared"
    );
    ok(ChannelMemberConfigClearResult { cleared })
}

fn channel_update(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ChannelUpdateParams = parse_params(params)?;
    let channel = state
        .store
        .update_channel(&p.channel_id, p.title, p.topic)
        .map_err(map_store_err)?;
    ok(ChannelUpdateResult { channel })
}

fn channel_set_instruction(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ChannelSetInstructionParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot set instructions for channel {}",
                p.channel_id
            ),
        ));
    }
    let channel = state
        .store
        .set_channel_instructions(&p.channel_id, Some(p.instructions), &caller)
        .map_err(map_store_err)?;
    ok(ChannelSetInstructionResult { channel })
}

fn channel_get_instruction(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ChannelGetInstructionParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot read instructions for channel {}",
                p.channel_id
            ),
        ));
    }
    let instructions = state
        .store
        .get_channel(&p.channel_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "channel"))?
        .instructions;
    ok(ChannelGetInstructionResult { instructions })
}

fn channel_clear_instruction(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ChannelClearInstructionParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot clear instructions for channel {}",
                p.channel_id
            ),
        ));
    }
    let existing = state
        .store
        .get_channel(&p.channel_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "channel"))?
        .instructions
        .is_some();
    let _ = state
        .store
        .set_channel_instructions(&p.channel_id, None, &caller)
        .map_err(map_store_err)?;
    ok(ChannelClearInstructionResult { cleared: existing })
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
        .create_thread(p.channel_id, p.title, p.root_message_id)
        .map_err(map_store_err)?;
    if let Err(e) = state
        .scope_skills
        .sync_thread_channel_memberships(&state.store, &thread)
    {
        warn_projection_failure("thread_create.sync_thread_channel_memberships", e);
    }
    let thread = state
        .store
        .attach_thread_activity_meta(vec![thread])
        .pop()
        .expect("one thread");
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
    let threads = state.store.attach_thread_activity_meta(threads);
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
    let thread = state
        .store
        .attach_thread_activity_meta(vec![thread])
        .pop()
        .expect("one thread");
    ok(ThreadUpdateResult { thread })
}

fn thread_set_instruction(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ThreadSetInstructionParams = parse_params(params)?;
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
                "actor {caller} cannot set instructions for thread {} in channel {channel_id}",
                p.thread_id
            ),
        ));
    }
    let thread = state
        .store
        .set_thread_instructions(&p.thread_id, Some(p.instructions), &caller)
        .map_err(map_store_err)?;
    let thread = state
        .store
        .attach_thread_activity_meta(vec![thread])
        .pop()
        .expect("one thread");
    ok(ThreadSetInstructionResult { thread })
}

fn thread_get_instruction(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ThreadGetInstructionParams = parse_params(params)?;
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
                "actor {caller} cannot read instructions for thread {} in channel {channel_id}",
                p.thread_id
            ),
        ));
    }
    let instructions = state
        .store
        .get_thread(&p.thread_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "thread"))?
        .instructions;
    ok(ThreadGetInstructionResult { instructions })
}

fn thread_clear_instruction(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ThreadClearInstructionParams = parse_params(params)?;
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
                "actor {caller} cannot clear instructions for thread {} in channel {channel_id}",
                p.thread_id
            ),
        ));
    }
    let existing = state
        .store
        .get_thread(&p.thread_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "thread"))?
        .instructions
        .is_some();
    let _ = state
        .store
        .set_thread_instructions(&p.thread_id, None, &caller)
        .map_err(map_store_err)?;
    ok(ThreadClearInstructionResult { cleared: existing })
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
    let thread = state
        .store
        .attach_thread_activity_meta(vec![thread])
        .pop()
        .expect("one thread");
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

fn thread_follow(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ThreadFollowParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let presence = state
        .store
        .follow_thread(caller, &p.thread_id, p.muted)
        .map_err(map_store_err)?;
    ok(ThreadFollowResult { presence })
}

fn thread_unfollow(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ThreadUnfollowParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let presence = state
        .store
        .unfollow_thread(caller, &p.thread_id)
        .map_err(map_store_err)?;
    ok(ThreadUnfollowResult { presence })
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
        .get_message(&p.source_message_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "source message"))?;
    state
        .store
        .check_scope_access(&source.scope, &caller)
        .map_err(map_store_err)?;
    let requester = p.requester_actor_id.unwrap_or(caller);
    let task = state
        .store
        .create_task(
            p.source_message_id,
            p.title,
            p.description,
            requester,
            p.owner_actor_id,
            p.status,
            p.parent_source_message_id,
            p.parent_task_id,
            p.practice_contract_epoch,
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
    let refs = state.store.list_task_refs(&task.id);
    let artifact_links = state.store.list_task_artifact_links(&task.id, None);
    let facts = state.store.list_task_facts(&task.id, None, None, None);
    let projections = state.store.list_task_projections(&task.id);
    ok(TaskGetResult {
        task,
        assignments,
        refs,
        artifact_links,
        facts,
        projections,
    })
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
            p.source_message_id.as_deref(),
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
    append_task_state_message(state, &caller, &task, "updated")?;
    ok(TaskUpdateResult { task })
}

fn task_claim(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskClaimParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let owner = p.actor_id.unwrap_or_else(|| caller.clone());
    let task_id = match (p.task_id, p.source_message_id) {
        (Some(task_id), None) => task_id,
        (None, Some(source_message_id)) => {
            let source = state
                .store
                .get_message(&source_message_id)
                .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "source message"))?;
            state
                .store
                .check_scope_access(&source.scope, &caller)
                .map_err(map_store_err)?;
            match state.store.create_task(
                source_message_id.clone(),
                None,
                String::new(),
                caller.clone(),
                Some(owner.clone()),
                Some(TaskStatus::Claimed),
                None,
                None,
                None,
            ) {
                Ok(task) => {
                    append_task_state_message(state, &caller, &task, "claimed")?;
                    return ok(TaskUpdateResult { task });
                }
                Err(StoreError::Conflict(_)) => {
                    state
                        .store
                        .find_task_by_source(&source_message_id)
                        .ok_or_else(|| {
                            ErrorObject::new(
                                ErrorCode::APP_CONFLICT,
                                format!(
                                    "task already exists for source message {source_message_id}"
                                ),
                            )
                        })?
                        .id
                }
                Err(err) => return Err(map_store_err(err)),
            }
        }
        (Some(_), Some(_)) | (None, None) => {
            return Err(ErrorObject::new(
                ErrorCode::INVALID_PARAMS,
                "pass exactly one of taskId or sourceMessageId",
            ))
        }
    };
    let existing = state
        .store
        .get_task(&task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &existing, &caller)?;
    let claim_would_change = existing.owner_actor_id.as_deref() != Some(owner.as_str())
        || existing.status == TaskStatus::Todo;
    let task = state
        .store
        .claim_task(&task_id, owner)
        .map_err(map_store_err)?;
    if claim_would_change {
        append_task_state_message(state, &caller, &task, "claimed")?;
    }
    ok(TaskUpdateResult { task })
}

fn task_assign(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskAssignParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = task_state_transition(
        state,
        &caller,
        &p.task_id,
        Some(TaskStatus::Claimed),
        Some(p.owner_actor_id),
        None,
        None,
        Vec::new(),
        "assigned",
    )?;
    ok(TaskUpdateResult { task })
}

fn task_complete(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskCompleteParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = task_state_transition(
        state,
        &caller,
        &p.task_id,
        Some(TaskStatus::Done),
        None,
        p.result_summary,
        None,
        p.artifact_ids,
        "completed",
    )?;
    ok(TaskUpdateResult { task })
}

fn task_reopen(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskReopenParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let next_status = if p.owner_actor_id.is_some() {
        TaskStatus::Claimed
    } else {
        TaskStatus::Todo
    };
    let task = task_state_transition(
        state,
        &caller,
        &p.task_id,
        Some(next_status),
        p.owner_actor_id,
        None,
        None,
        Vec::new(),
        "reopened",
    )?;
    ok(TaskUpdateResult { task })
}

fn task_cancel(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskCancelParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = task_state_transition(
        state,
        &caller,
        &p.task_id,
        Some(TaskStatus::Canceled),
        None,
        p.result_summary,
        None,
        Vec::new(),
        "canceled",
    )?;
    ok(TaskUpdateResult { task })
}

#[allow(clippy::too_many_arguments)]
fn task_state_transition(
    state: &AppState,
    caller: &str,
    task_id: &str,
    status: Option<TaskStatus>,
    owner_actor_id: Option<String>,
    result_summary: Option<String>,
    artifact_ids: Option<Vec<String>>,
    append_artifact_ids: Vec<String>,
    action: &str,
) -> Result<Task, ErrorObject> {
    let task = state
        .store
        .get_task(task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, caller)?;
    let task = state
        .store
        .update_task(
            task_id,
            status,
            owner_actor_id,
            result_summary,
            artifact_ids,
            append_artifact_ids,
        )
        .map_err(map_store_err)?;
    append_task_state_message(state, caller, &task, action)?;
    Ok(task)
}

fn append_task_state_message(
    state: &AppState,
    caller: &str,
    task: &Task,
    action: &str,
) -> Result<(), ErrorObject> {
    let target = format!("#{}:{}", task.channel_id, task.source_message_id);
    let mut audience = vec![AudienceRef {
        kind: AudienceKind::Actor,
        id: task.requester_actor_id.clone(),
        display: None,
    }];
    if let Some(owner) = task.owner_actor_id.as_ref() {
        audience.push(AudienceRef {
            kind: AudienceKind::Actor,
            id: owner.clone(),
            display: None,
        });
    }
    let audience = unique_audience(audience);
    let mut metadata = BTreeMap::new();
    metadata.insert("taskId".into(), json!(task.id));
    metadata.insert("taskStatus".into(), json!(task.status));
    metadata.insert("taskAction".into(), json!(action));
    state
        .store
        .append_message(
            caller.to_string(),
            target,
            MessageKind::TaskUpdate,
            format!("Task #{} {action}: {}", task.number, task.title),
            Vec::new(),
            audience,
            MessageIntent::StatusUpdate,
            DeliveryPolicy::NotifyOnly,
            None,
            Some(task.source_message_id.clone()),
            Vec::new(),
            metadata,
            None,
        )
        .map(|_| ())
        .map_err(map_store_err)
}

fn unique_audience(audience: Vec<AudienceRef>) -> Vec<AudienceRef> {
    let mut out = Vec::new();
    for entry in audience {
        if !entry.id.trim().is_empty()
            && !out.iter().any(|existing: &AudienceRef| {
                existing.kind == entry.kind && existing.id == entry.id
            })
        {
            out.push(entry);
        }
    }
    out
}

fn task_ref_attach(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskRefAttachParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let task_ref = state
        .store
        .attach_task_ref(
            &p.task_id,
            p.kind,
            p.subtype,
            p.value,
            p.normalized,
            p.fields,
            p.confidence.unwrap_or(TaskRefConfidence::Inferred),
            p.status.unwrap_or(TaskRefStatus::Active),
            p.superseded_by,
            p.source_message_id,
            caller,
        )
        .map_err(map_store_err)?;
    ok(TaskRefAttachResult { task_ref })
}

fn task_ref_find(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskRefFindParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if let Some(channel_id) = p.channel_id.as_ref() {
        if !state.store.is_channel_member(channel_id, &caller) {
            return Err(ErrorObject::new(
                ErrorCode::APP_INVALID_STATE,
                format!("actor {caller} cannot find refs in channel {channel_id}"),
            ));
        }
    }
    let (refs, tasks) = state.store.find_task_refs(
        p.channel_id.as_deref(),
        &p.kind,
        &p.subtype,
        &p.normalized,
        p.confidence,
        p.status,
    );
    let refs: Vec<TaskRef> = refs
        .into_iter()
        .filter(|r| state.store.is_channel_member(&r.channel_id, &caller))
        .collect();
    let tasks: Vec<Task> = tasks
        .into_iter()
        .filter(|task| state.store.is_channel_member(&task.channel_id, &caller))
        .collect();
    ok(TaskRefFindResult { refs, tasks })
}

fn task_ref_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskRefListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    ok(TaskRefListResult {
        refs: state.store.list_task_refs(&p.task_id),
    })
}

fn task_artifact_attach(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskArtifactAttachParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let link = state
        .store
        .attach_task_artifact_link(
            &p.task_id,
            p.artifact_id,
            p.schema,
            p.role,
            p.sequence,
            p.status.unwrap_or(TaskArtifactLinkStatus::Active),
            p.lineage,
            p.binding,
            caller,
        )
        .map_err(map_store_err)?;
    ok(TaskArtifactAttachResult { link })
}

fn task_artifact_activate(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskArtifactActivateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let link = state
        .store
        .get_task_artifact_link(&p.link_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "artifact link"))?;
    let task = state
        .store
        .get_task(&link.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let (link, superseded) = state
        .store
        .activate_task_artifact_link(&p.link_id, p.supersede_link_ids)
        .map_err(map_store_err)?;
    ok(TaskArtifactActivateResult { link, superseded })
}

fn task_artifact_list(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskArtifactListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    ok(TaskArtifactListResult {
        links: state.store.list_task_artifact_links(&p.task_id, p.status),
    })
}

fn task_fact_append(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskFactAppendParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let (fact, created) = state
        .store
        .append_task_fact(
            &p.task_id,
            p.target_key,
            p.kind,
            p.fact_type.unwrap_or(TaskFactType::UserDefined),
            p.subject,
            p.signature,
            p.status.unwrap_or(TaskFactStatus::Active),
            p.replaces,
            p.retracted_by,
            p.authority,
            p.authority_binding,
            p.observed_at,
            p.source_cursor,
            p.source_snapshot_id,
            p.external_updated_at,
            p.observed_fields,
            p.unobserved_fields,
            p.unavailable_reason,
            p.snapshot_completeness,
            p.producer_id.unwrap_or(caller),
            p.summary,
            p.raw_refs,
            p.artifact_id,
            p.payload_schema,
            p.payload,
        )
        .map_err(map_store_err)?;
    ok(TaskFactAppendResult { fact, created })
}

fn task_fact_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskFactListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    ok(TaskFactListResult {
        facts: state.store.list_task_facts(
            &p.task_id,
            p.kind.as_deref(),
            p.status,
            p.target_key.as_deref(),
        ),
    })
}

fn task_projection_put(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskProjectionPutParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let projection = state
        .store
        .put_task_projection(
            &p.task_id,
            if p.projection_type.trim().is_empty() {
                "summary".to_string()
            } else {
                p.projection_type
            },
            p.producer_actor_id.unwrap_or(caller),
            p.health.unwrap_or(TaskProjectionHealth::Fresh),
            p.watermark,
            p.payload_schema,
            p.payload,
        )
        .map_err(map_store_err)?;
    ok(TaskProjectionPutResult { projection })
}

fn task_projection_get(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskProjectionGetParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let projection_type = if p.projection_type.trim().is_empty() {
        "summary"
    } else {
        p.projection_type.as_str()
    };
    let projection = state.store.get_task_projection(&p.task_id, projection_type);
    let health = projection
        .as_ref()
        .map(|p| p.health)
        .unwrap_or(TaskProjectionHealth::Missing);
    ok(TaskProjectionGetResult { projection, health })
}

fn task_projection_list(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskProjectionListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let task = state
        .store
        .get_task(&p.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    ok(TaskProjectionListResult {
        projections: state.store.list_task_projections(&p.task_id),
    })
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
    let (assignment, task, _created) = state
        .store
        .create_task_assignment(
            &p.task_id,
            from.clone(),
            p.to_actor_id.clone(),
            p.assignment_type,
            p.instruction.clone(),
            p.contract.clone(),
            p.idempotency_key,
        )
        .map_err(map_store_err)?;

    let mut metadata = Meta::default();
    metadata.insert("taskId".into(), json!(task.id.clone()));
    metadata.insert("taskNumber".into(), json!(task.number));
    metadata.insert("assignmentId".into(), json!(assignment.id.clone()));
    metadata.insert(
        "assignmentType".into(),
        serde_json::to_value(assignment.assignment_type).map_err(map_runtime_err)?,
    );
    metadata.insert(
        "assignmentContract".into(),
        assignment.contract.clone().unwrap_or(Value::Null),
    );
    metadata.insert(
        "expectedOutput".into(),
        json!("Read assignment-context, publish typed outputs/facts, then complete this assignment with task/assignment.update."),
    );
    let target = format!("#{}:{}", task.channel_id, task.source_message_id);
    let message = state
        .store
        .ensure_assignment_message(
            &assignment.id,
            assignment.from_actor_id.clone(),
            target,
            task_assignment_message(&task, &assignment),
            vec![AudienceRef {
                kind: AudienceKind::Actor,
                id: assignment.to_actor_id.clone(),
                display: None,
            }],
            MessageIntent::AssignTask,
            DeliveryPolicy::WakeAgent,
            metadata,
        )
        .map_err(map_store_err)?;
    ok(TaskAssignmentCreateResult {
        assignment,
        message,
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
    let is_owner = task.owner_actor_id.as_deref() == Some(caller.as_str());
    if caller != assignment.to_actor_id && caller != assignment.from_actor_id && !is_owner {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot update assignment {} owned by {}",
                assignment.id, assignment.to_actor_id
            ),
        ));
    }
    let previous_status = assignment.status;
    let (assignment, task) = state
        .store
        .update_task_assignment(
            &p.assignment_id,
            requested_status,
            p.result_message_id,
            p.result_summary,
            p.result_envelope,
            p.result_artifact_ids,
            p.result_fact_ids,
            p.evidence_refs,
        )
        .map_err(map_store_err)?;
    if !is_terminal_assignment_status(previous_status)
        && is_terminal_assignment_status(assignment.status)
    {
        emit_assignment_return_message(state, &caller, &task, &assignment);
    }
    ok(TaskAssignmentUpdateResult { assignment, task })
}

fn task_assignment_context(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskAssignmentContextParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let (task, assignment, refs, artifact_links, facts, projection, guards) = state
        .store
        .assignment_context(&p.assignment_id)
        .map_err(map_store_err)?;
    ensure_task_access(state, &task, &caller)?;
    ok(TaskAssignmentContextResult {
        task,
        assignment,
        refs,
        artifact_links,
        facts,
        projection,
        guards,
    })
}

fn task_assignment_preflight(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: TaskAssignmentPreflightParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let assignment = state
        .store
        .get_assignment(&p.assignment_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "assignment"))?;
    let task = state
        .store
        .get_task(&assignment.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    if caller != assignment.to_actor_id && caller != assignment.from_actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot preflight assignment {} owned by {}",
                assignment.id, assignment.to_actor_id
            ),
        ));
    }
    let preflight = state
        .store
        .assignment_preflight(&p.assignment_id, p.target_key, p.head, p.effect)
        .map_err(map_store_err)?;
    ok(TaskAssignmentPreflightResult { preflight })
}

fn task_change_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskChangeListParams = parse_params(params).unwrap_or_default();
    let caller = caller_actor(state, connection_id)?;
    let recipient = p.recipient_actor_id.unwrap_or(caller.clone());
    if recipient != caller {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot list task changes for {recipient}"),
        ));
    }
    let deliveries = state.store.list_task_changes(
        &recipient,
        p.task_id.as_deref(),
        p.include_handled,
        p.after_cursor,
        p.limit,
    );
    ok(TaskChangeListResult { deliveries })
}

fn task_change_ack(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: TaskChangeAckParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let recipient = p.recipient_actor_id.unwrap_or(caller.clone());
    if recipient != caller {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot ack task changes for {recipient}"),
        ));
    }
    let delivery = state
        .store
        .ack_task_change(
            &p.change_id,
            &recipient,
            p.disposition,
            p.result_ref_ids,
            p.reason,
        )
        .map_err(map_store_err)?;
    ok(TaskChangeAckResult { delivery })
}

fn workspace_lease_acquire(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: WorkspaceLeaseAcquireParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let assignment = state
        .store
        .get_assignment(&p.assignment_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "assignment"))?;
    let task = state
        .store
        .get_task(&assignment.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    if caller != assignment.to_actor_id && caller != assignment.from_actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot acquire lease for assignment {}",
                assignment.id
            ),
        ));
    }
    let expires_at = p
        .expires_at
        .unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(p.ttl_seconds.unwrap_or(3600)));
    let (lease, conflicts) = state
        .store
        .acquire_workspace_lease(&p.assignment_id, p.resource_key, p.mode, expires_at)
        .map_err(map_store_err)?;
    ok(WorkspaceLeaseAcquireResult { lease, conflicts })
}

fn workspace_lease_release(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: WorkspaceLeaseReleaseParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let existing = state
        .store
        .get_workspace_lease(&p.lease_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "lease"))?;
    let assignment = state
        .store
        .get_assignment(&existing.holder_assignment_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "assignment"))?;
    let task = state
        .store
        .get_task(&assignment.task_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "task"))?;
    ensure_task_access(state, &task, &caller)?;
    let lease = state
        .store
        .release_workspace_lease(&p.lease_id)
        .map_err(map_store_err)?;
    ok(WorkspaceLeaseReleaseResult { lease })
}

fn workspace_lease_list(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: WorkspaceLeaseListParams = parse_params(params).unwrap_or_default();
    let caller = caller_actor(state, connection_id)?;
    let leases = state.store.list_workspace_leases(
        p.resource_key.as_deref(),
        p.assignment_id.as_deref(),
        p.active_only,
    );
    let leases: Vec<WorkspaceLease> = leases
        .into_iter()
        .filter(|lease| {
            state
                .store
                .get_assignment(&lease.holder_assignment_id)
                .and_then(|assignment| state.store.get_task(&assignment.task_id))
                .is_some_and(|task| state.store.is_channel_member(&task.channel_id, &caller))
        })
        .collect();
    ok(WorkspaceLeaseListResult { leases })
}

fn is_terminal_assignment_status(status: TaskAssignmentStatus) -> bool {
    matches!(
        status,
        TaskAssignmentStatus::Completed
            | TaskAssignmentStatus::Failed
            | TaskAssignmentStatus::Canceled
    )
}

fn emit_assignment_return_message(
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
        "Assignment `{assignment_id}` ({assignment_type:?}) is {status}.\n\n@{target_actor_id} Task #{number} is back with you. Continue from the assignment result: revise, create a follow-up assignment, ask the requester, or update the task status.",
        assignment_id = assignment.id,
        assignment_type = assignment.assignment_type,
        number = task.number,
    );
    if !assignment.result_summary.trim().is_empty() {
        text.push_str("\n\nResult summary:\n");
        text.push_str(assignment.result_summary.trim());
    }

    let mut metadata = Meta::default();
    metadata.insert("taskId".into(), json!(task.id.clone()));
    metadata.insert("taskNumber".into(), json!(task.number));
    metadata.insert("assignmentId".into(), json!(assignment.id.clone()));
    metadata.insert("assignmentStatus".into(), json!(status.clone()));
    metadata.insert(
        "expectedOutput".into(),
        json!("Continue the task in this thread; either revise, create follow-up assignments, ask the requester, or update the task status."),
    );
    let target = format!("#{}:{}", task.channel_id, task.source_message_id);
    if let Err(err) = state.store.append_message(
        from_actor_id.into(),
        target,
        MessageKind::TaskUpdate,
        text,
        Vec::new(),
        vec![AudienceRef {
            kind: AudienceKind::Actor,
            id: target_actor_id.into(),
            display: None,
        }],
        MessageIntent::RequestAction,
        DeliveryPolicy::WakeAgent,
        None,
        None,
        Vec::new(),
        metadata,
        None,
    ) {
        tracing::warn!(
            task = %task.id,
            assignment = %assignment.id,
            target = %target_actor_id,
            %err,
            "failed to append assignment return message"
        );
    }
}

fn task_assignment_message(task: &Task, assignment: &TaskAssignment) -> String {
    format!(
        "Task #{number}: {title}\n\n@{to_actor} Assignment `{assignment_id}` ({assignment_type:?}) is for you.\n\n{instruction}\n\nRead assignment-context before acting. Publish typed outputs with `loom task artifact attach` / `loom task fact append`. When complete, run `loom task assignment update {assignment_id} --status completed --result <summary> --result-artifact-id <art_id> ... --result-fact-id <fact_id> ...`. Do not wake another actor to finish this assignment; the assignment update returns the task to the assigner.",
        number = task.number,
        title = task.title,
        assignment_id = assignment.id,
        assignment_type = assignment.assignment_type,
        to_actor = assignment.to_actor_id,
        instruction = assignment.instruction.trim(),
    )
}

// ---- run ----

fn run_open(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: RunOpenParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if caller != p.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "connection actor {caller} cannot open run for {}",
                p.actor_id
            ),
        ));
    }
    let run = state
        .store
        .open_run(
            p.actor_id,
            p.scope,
            p.delivery_id,
            p.start_reason,
            p.agent_config_version_id,
            p.metadata,
        )
        .map_err(map_store_err)?;
    ok(RunOpenResult { run })
}

fn run_append(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: RunAppendParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let run = state
        .store
        .get_run(&p.run_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "run"))?;
    if caller != run.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("connection actor {caller} cannot append run {}", p.run_id),
        ));
    }
    let (run, frame) = state
        .store
        .append_run_frame(&p.run_id, p.status, p.frame_kind, p.payload)
        .map_err(map_store_err)?;
    ok(RunAppendResult { run, frame })
}

fn run_close(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: RunCloseParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let run = state
        .store
        .get_run(&p.run_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "run"))?;
    if caller != run.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("connection actor {caller} cannot close run {}", p.run_id),
        ));
    }
    let run = state
        .store
        .close_run(&p.run_id, p.status)
        .map_err(map_store_err)?;
    ok(RunCloseResult { run })
}

fn run_cancel(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: RunCancelParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let run = state
        .store
        .get_run(&p.run_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "run"))?;

    let channel_id = crate::ws::channel_id_for_scope(state, &run.scope)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "channel for run scope"))?;
    if !state.store.is_channel_member(&channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!("actor {caller} cannot cancel run in channel {channel_id}"),
        ));
    }
    if matches!(
        run.status,
        RunStatus::Completed | RunStatus::Failed | RunStatus::Canceled
    ) {
        return ok(RunCancelResult {
            run,
            cancel_message: None,
        });
    }

    let target = state
        .store
        .message_target_for_scope(&run.scope)
        .map_err(map_store_err)?;
    let mut metadata = Meta::default();
    metadata.insert("kind".into(), json!("run.cancel"));
    metadata.insert("runId".into(), json!(run.id.clone()));
    metadata.insert("canceledBy".into(), json!(caller.clone()));
    if let Some(reason) = p.reason.as_ref().filter(|value| !value.trim().is_empty()) {
        metadata.insert("reason".into(), json!(reason));
    }
    let body = p
        .reason
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .map(|reason| format!("Cancel run `{}`: {}", run.id, reason.trim()))
        .unwrap_or_else(|| format!("Cancel run `{}`.", run.id));
    let cancel_message = state
        .store
        .append_message(
            caller.clone(),
            target,
            MessageKind::System,
            body,
            Vec::new(),
            vec![AudienceRef {
                kind: AudienceKind::Actor,
                id: run.actor_id.clone(),
                display: None,
            }],
            MessageIntent::RequestAction,
            DeliveryPolicy::WakeAgent,
            None,
            None,
            Vec::new(),
            metadata,
            None,
        )
        .map_err(map_store_err)?;
    let run = state
        .store
        .close_run(&run.id, RunStatus::Canceled)
        .map_err(map_store_err)?;
    ok(RunCancelResult {
        run,
        cancel_message: Some(cancel_message),
    })
}

// ---- coordination ----

fn coordination_propose(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: CoordinationProposeParams = parse_params(params)?;
    let owner = caller_actor(state, connection_id)?;
    let session = state
        .store
        .propose_coordination_session(
            owner,
            p.target,
            p.mode,
            p.decision_rule,
            p.participants,
            p.task_id,
            p.thread_root_message_id,
            p.plan,
            p.metadata,
        )
        .map_err(map_store_err)?;
    ok(CoordinationProposeResult { session })
}

fn coordination_commit(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: CoordinationCommitParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let session = state
        .store
        .commit_coordination_session(&p.session_id, &caller)
        .map_err(map_store_err)?;
    ok(CoordinationCommitResult { session })
}

fn coordination_respond(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: CoordinationRespondParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let session = state
        .store
        .respond_coordination_session(&p.session_id, &caller, p.accept, p.reason)
        .map_err(map_store_err)?;
    ok(CoordinationRespondResult { session })
}

fn coordination_step(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: CoordinationStepParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let kind = message_kind_for_actor(state.store.get_actor(&caller));
    let (session, step, message) = state
        .store
        .apply_coordination_step(
            caller,
            &p.session_id,
            p.base_revision,
            p.step_type,
            p.output,
            p.message_body,
            kind,
        )
        .map_err(map_store_err)?;
    ok(CoordinationStepResult {
        session,
        step,
        message,
    })
}

fn coordination_skip(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: CoordinationSkipParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let (session, step) = state
        .store
        .skip_coordination_step(caller, &p.session_id, p.base_revision, p.reason)
        .map_err(map_store_err)?;
    ok(CoordinationSkipResult { session, step })
}

fn coordination_reassign(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: CoordinationReassignParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let (session, step) = state
        .store
        .reassign_coordination_baton(
            caller,
            &p.session_id,
            p.from_actor_id,
            p.to_actor_id,
            p.base_revision,
        )
        .map_err(map_store_err)?;
    ok(CoordinationReassignResult { session, step })
}

// ---- agent config ----

fn agent_config_publish(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: AgentConfigPublishParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let version = state
        .store
        .publish_agent_config_version(
            p.actor_id,
            p.version,
            p.prompt,
            p.model,
            p.adapter,
            p.tools,
            p.capability_tags,
            p.attention_policy,
            p.context_policy,
            p.reply_policy,
            caller,
            p.metadata,
        )
        .map_err(map_store_err)?;
    ok(AgentConfigPublishResult { version })
}

fn agent_config_activate(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: AgentConfigActivateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let (activation, version) = state
        .store
        .activate_agent_config_version(p.actor_id, p.version_id, p.scope, caller)
        .map_err(map_store_err)?;
    ok(AgentConfigActivateResult {
        activation,
        version,
    })
}

fn message_kind_for_actor(actor: Option<Actor>) -> MessageKind {
    match actor.map(|a| a.kind) {
        Some(ActorKind::Agent) => MessageKind::Agent,
        Some(ActorKind::Service) => MessageKind::System,
        Some(ActorKind::Human) | None => MessageKind::Human,
    }
}

fn message_send(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: MessageSendParams = parse_params(params)?;
    let actor_id = caller_actor(state, connection_id)?;
    let kind = message_kind_for_actor(state.store.get_actor(&actor_id));
    let message = state
        .store
        .append_message_idempotent(
            actor_id,
            p.target,
            kind,
            p.body,
            p.mentions,
            p.audience,
            p.intent,
            p.delivery_policy,
            p.parent_message_id,
            p.thread_root_message_id,
            p.attachments,
            p.metadata,
            p.if_latest_message_id,
            p.idempotency_key,
        )
        .map_err(map_store_err)?;
    ok(MessageSendResult { message })
}

fn message_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: MessageListParams = parse_params(params)?;
    let actor_id = caller_actor(state, connection_id)?;
    let (messages, has_more) = state
        .store
        .read_messages_for_target(
            &actor_id,
            &p.target,
            p.limit,
            p.before_message_id.as_deref(),
        )
        .map_err(map_store_err)?;
    ok(MessageListResult {
        messages,
        page_info: PageInfo {
            has_more,
            next_cursor: None,
            _meta: None,
        },
    })
}

fn message_read(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: MessageReadParams = parse_params(params)?;
    let actor_id = caller_actor(state, connection_id)?;
    let message = state
        .store
        .get_message(&p.message_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "message"))?;
    state
        .store
        .check_scope_access(&message.scope, &actor_id)
        .map_err(map_store_err)?;
    if !Store::message_visible_to_actor(&message, &actor_id) {
        return Err(ErrorObject::new(ErrorCode::APP_NOT_FOUND, "message"));
    }
    ok(MessageReadResult { message })
}

fn message_reaction_toggle(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: MessageReactionToggleParams = parse_params(params)?;
    let actor_id = caller_actor(state, connection_id)?;
    let message = state
        .store
        .toggle_message_reaction(actor_id, &p.message_id, p.emoji)
        .map_err(map_store_err)?;
    ok(MessageReactionToggleResult { message })
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
    ok(MessageSearchResult {
        messages: state
            .store
            .search_message_records(&actor_id, &p.query, p.target.as_deref(), p.limit)
            .map_err(map_store_err)?,
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
        .schedule_reminder(
            p.actor_id, p.title, p.scope, p.msg_id, fire_at, p.repeat, p._meta,
        )
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

// ---- inbox.list (§9.2 durable actor inbox) ----
//
// Cursor format is `<rfc3339_utc>|<source_id>`. The `|` separator avoids
// collision with the `:` characters inside an RFC3339 timestamp; source ids
// are generated without `|` so the split is unambiguous. Cursor is opaque to
// callers — the format is an implementation detail of this handler and
// `Store::list_deliveries`.

fn encode_delivery_cursor(d: &Delivery) -> String {
    format!("{}|{}", d.updated_at.to_rfc3339(), d.source_id)
}

fn decode_delivery_cursor(raw: &str) -> Result<(Timestamp, String), ErrorObject> {
    let (ts_str, eid) = raw.split_once('|').ok_or_else(|| {
        ErrorObject::new(
            ErrorCode::INVALID_PARAMS,
            format!("inbox.list: malformed cursor `{raw}` (expected `<ts>|<source_id>`)"),
        )
    })?;
    let ts = chrono::DateTime::parse_from_rfc3339(ts_str)
        .map_err(|e| {
            ErrorObject::new(
                ErrorCode::INVALID_PARAMS,
                format!("inbox.list: bad cursor timestamp `{ts_str}`: {e}"),
            )
        })?
        .with_timezone(&chrono::Utc);
    Ok((ts, eid.to_string()))
}

fn inbox_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: InboxListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if caller != p.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "inbox.list: caller actor `{caller}` cannot read inbox of `{}`",
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
    let entries = rows
        .into_iter()
        .map(|delivery| InboxListEntry {
            message: state
                .store
                .get_message(&delivery.source_id)
                .filter(|message| Store::message_visible_to_actor(message, &p.actor_id)),
            event: state.store.get_event(&delivery.source_id),
            delivery,
        })
        .collect();
    ok(InboxListResult {
        deliveries: entries,
        next_cursor,
    })
}

fn delivery_ack(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: DeliveryAckParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if caller != p.actor_id {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "delivery.ack: caller actor `{caller}` cannot ack inbox of `{}`",
                p.actor_id
            ),
        ));
    }
    let delivery = state
        .store
        .ack_delivery(&p.actor_id, &p.source_id)
        .map_err(map_store_err)?;
    ok(DeliveryAckResult { delivery })
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
        finish_legacy_machine_command(
            state,
            &command_id,
            MachineCommandStatus::Cancelled,
            "not_delivered",
            "machine daemon is not connected; command cancelled",
        )?;
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "machine daemon is not connected; command cancelled",
        ));
    }

    let result = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), rx).await
    {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            finish_legacy_machine_command(
                state,
                &command_id,
                MachineCommandStatus::Cancelled,
                "waiter_closed",
                "machine command result waiter closed",
            )?;
            return Err(ErrorObject::new(
                ErrorCode::APP_RUNTIME_ERROR,
                format!("machine command result channel closed: {command_id}"),
            ));
        }
        Err(_) => {
            state.machine_commands.cancel(&command_id);
            finish_legacy_machine_command(
                state,
                &command_id,
                MachineCommandStatus::Expired,
                "timeout",
                "machine command timed out",
            )?;
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

fn finish_legacy_machine_command(
    state: &AppState,
    command_id: &str,
    status: MachineCommandStatus,
    code: &str,
    message: &str,
) -> Result<(), ErrorObject> {
    let Some(mut command) = state.store.get_machine_command(command_id) else {
        return Ok(());
    };
    if command.status.is_terminal() {
        return Ok(());
    }
    let now = Utc::now();
    command.status = status;
    command.error = Some(MachineCommandError {
        code: code.into(),
        message: message.into(),
        retryable: false,
        details: None,
    });
    command.finished_at = Some(now);
    command.updated_at = now;
    state
        .store
        .upsert_machine_command(command)
        .map_err(map_store_err)?;
    Ok(())
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
        "agent.create" | "agent.update" | "agent.remove" | "provider.add" | "provider.remove"
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
/// pass `actorKind`. This dedicated RPC lets `loom-daemon` (and any
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

fn actor_group_create(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ActorGroupCreateParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if !state.store.is_channel_member(&p.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot create groups in channel {}",
                p.channel_id
            ),
        ));
    }
    let group = state
        .store
        .create_actor_group(
            p.channel_id,
            p.name,
            p.display_name,
            p.member_actor_ids,
            p.wake_agents,
        )
        .map_err(map_store_err)?;
    ok(ActorGroupCreateResult { group })
}

fn actor_group_list(state: &AppState, connection_id: &str, params: Option<Value>) -> HandlerResult {
    let p: ActorGroupListParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    if let Some(channel_id) = p.channel_id.as_deref() {
        if !state.store.is_channel_member(channel_id, &caller) {
            return Err(ErrorObject::new(
                ErrorCode::APP_INVALID_STATE,
                format!("actor {caller} cannot list groups in channel {channel_id}"),
            ));
        }
        return ok(ActorGroupListResult {
            groups: state.store.list_actor_groups(Some(channel_id)),
        });
    }
    let groups = state
        .store
        .list_actor_groups(None)
        .into_iter()
        .filter(|group| state.store.is_channel_member(&group.channel_id, &caller))
        .collect();
    ok(ActorGroupListResult { groups })
}

fn actor_group_add_member(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ActorGroupMemberParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let group = state
        .store
        .get_actor_group(&p.group_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "actor group"))?;
    if !state.store.is_channel_member(&group.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot edit groups in channel {}",
                group.channel_id
            ),
        ));
    }
    let group = state
        .store
        .add_actor_group_member(&p.group_id, &p.actor_id)
        .map_err(map_store_err)?;
    ok(ActorGroupMemberResult { group })
}

fn actor_group_remove_member(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
    let p: ActorGroupMemberParams = parse_params(params)?;
    let caller = caller_actor(state, connection_id)?;
    let group = state
        .store
        .get_actor_group(&p.group_id)
        .ok_or_else(|| ErrorObject::new(ErrorCode::APP_NOT_FOUND, "actor group"))?;
    if !state.store.is_channel_member(&group.channel_id, &caller) {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            format!(
                "actor {caller} cannot edit groups in channel {}",
                group.channel_id
            ),
        ));
    }
    let group = state
        .store
        .remove_actor_group_member(&p.group_id, &p.actor_id)
        .map_err(map_store_err)?;
    ok(ActorGroupMemberResult { group })
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
            "loom-handler-tests-{name}-{}",
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

    async fn open_agent_conn(state: &AppState, connection_id: &str, actor_id: &str) {
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
            Some(json!({
                "actorId": actor_id,
                "actorKind": "agent",
            })),
        )
        .await
        .expect("connection/open agent");
    }

    #[tokio::test]
    async fn message_send_rpc_returns_first_message_for_an_idempotent_retry() {
        let state = fresh_state("message-send-idempotency");
        open_conn(&state, "conn_alice", "actor_alice").await;
        let channel = state
            .store
            .create_channel("idempotent messages".into(), Some("actor_alice".into()))
            .unwrap();
        let target = format!("#{}", channel.id);

        let first_value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEND,
            Some(json!({
                "target": target,
                "body": "first body",
                "idempotencyKey": "status-for-head-abc"
            })),
        )
        .await
        .expect("first message.send");
        let first: MessageSendResult = serde_json::from_value(first_value).expect("first result");

        let retry_value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEND,
            Some(json!({
                "target": target,
                "body": "different retry body",
                "idempotencyKey": "status-for-head-abc",
                "ifLatestMessageId": "stale-optimistic-guard"
            })),
        )
        .await
        .expect("idempotent retry");
        let retry: MessageSendResult = serde_json::from_value(retry_value).expect("retry result");

        assert_eq!(retry.message.id, first.message.id);
        assert_eq!(retry.message.body, "first body");
        assert_eq!(
            retry.message.idempotency_key.as_deref(),
            Some("status-for-head-abc")
        );
        let (messages, _) = state
            .store
            .read_messages_for_target("actor_alice", &target, 10, None)
            .unwrap();
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn legacy_event_rpc_names_are_not_public_methods() {
        let state = fresh_state("legacy_event_rpc_names_are_not_public_methods");
        open_conn(&state, "conn_alice", "actor_alice").await;

        for method_name in [format!("event/{}", "append"), format!("scope/{}", "read")] {
            let err = dispatch(&state, "conn_alice", &method_name, None)
                .await
                .expect_err("legacy method should be rejected");
            assert_eq!(err.code, ErrorCode::METHOD_NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn channel_member_config_rpc_set_list_get_and_clear() {
        let state = fresh_state("channel_member_config_rpc_set_list_get_and_clear");
        open_conn(&state, "conn_owner", "actor_owner").await;
        open_agent_conn(&state, "conn_agent", "actor_agent").await;

        let channel_value = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "backend" })),
        )
        .await
        .expect("channel/create");
        let created: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel create result");

        dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_INVITE,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_agent",
            })),
        )
        .await
        .expect("channel/invite");

        let set_value = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_MEMBER_CONFIG_SET,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_agent",
                "workspaceDir": "F:/work/backend",
            })),
        )
        .await
        .expect("member config set");
        let set: ChannelMemberConfigSetResult =
            serde_json::from_value(set_value).expect("set result");
        assert_eq!(set.config.workspace_dir.as_deref(), Some("F:/work/backend"));

        let list_value = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_MEMBER_CONFIG_LIST,
            Some(json!({ "channelId": &created.channel.id })),
        )
        .await
        .expect("member config list");
        let list: ChannelMemberConfigListResult =
            serde_json::from_value(list_value).expect("list result");
        assert_eq!(list.configs.len(), 1);

        let get_value = dispatch(
            &state,
            "conn_agent",
            method::CHANNEL_MEMBER_CONFIG_GET,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_agent",
            })),
        )
        .await
        .expect("member config get");
        let get: ChannelMemberConfigGetResult =
            serde_json::from_value(get_value).expect("get result");
        assert_eq!(
            get.config.and_then(|config| config.workspace_dir),
            Some("F:/work/backend".into())
        );

        let clear_value = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_MEMBER_CONFIG_CLEAR,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_agent",
            })),
        )
        .await
        .expect("member config clear");
        let clear: ChannelMemberConfigClearResult =
            serde_json::from_value(clear_value).expect("clear result");
        assert!(clear.cleared);
        assert!(state
            .store
            .get_channel_member_config(&created.channel.id, "actor_agent")
            .is_none());
    }

    #[tokio::test]
    async fn channel_member_config_rpc_rejects_empty_path_and_non_members() {
        let state = fresh_state("channel_member_config_rpc_rejects_empty_path_and_non_members");
        open_conn(&state, "conn_owner", "actor_owner").await;
        open_agent_conn(&state, "conn_agent", "actor_agent").await;
        open_conn(&state, "conn_intruder", "actor_intruder").await;
        open_conn(&state, "conn_human", "actor_human").await;

        let channel_value = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "backend" })),
        )
        .await
        .expect("channel/create");
        let created: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel create result");
        dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_INVITE,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_agent",
            })),
        )
        .await
        .expect("channel/invite");
        dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_INVITE,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_human",
            })),
        )
        .await
        .expect("channel/invite human");

        let empty_err = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_MEMBER_CONFIG_SET,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_agent",
                "workspaceDir": "   ",
            })),
        )
        .await
        .expect_err("empty path rejected");
        assert_eq!(empty_err.code, ErrorCode::INVALID_PARAMS);

        let intruder_err = dispatch(
            &state,
            "conn_intruder",
            method::CHANNEL_MEMBER_CONFIG_LIST,
            Some(json!({ "channelId": &created.channel.id })),
        )
        .await
        .expect_err("non-member rejected");
        assert_eq!(intruder_err.code, ErrorCode::APP_INVALID_STATE);

        let target_err = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_MEMBER_CONFIG_SET,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_intruder",
                "workspaceDir": "F:/work/backend",
            })),
        )
        .await
        .expect_err("target must be explicit member");
        assert_eq!(target_err.code, ErrorCode::APP_INVALID_STATE);

        let human_err = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_MEMBER_CONFIG_SET,
            Some(json!({
                "channelId": &created.channel.id,
                "actorId": "actor_human",
                "workspaceDir": "F:/work/backend",
            })),
        )
        .await
        .expect_err("target must be an agent");
        assert_eq!(human_err.code, ErrorCode::APP_INVALID_STATE);
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

    #[tokio::test]
    async fn message_rpc_send_list_and_search_thread_target() {
        let state = fresh_state("message_rpc_thread");
        open_conn(&state, "conn_alice", "actor_alice").await;
        let channel_value = dispatch(
            &state,
            "conn_alice",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "backend" })),
        )
        .await
        .expect("channel/create");
        let channel: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel result");

        let root_value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEND,
            Some(json!({
                "target": format!("#{}", channel.channel.id),
                "body": "root message",
            })),
        )
        .await
        .expect("message.send root");
        let root: MessageSendResult = serde_json::from_value(root_value).expect("root result");
        let thread_target = format!("#{}:{}", channel.channel.id, root.message.id);
        let reply_value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEND,
            Some(json!({
                "target": thread_target.clone(),
                "body": "thread reply needle",
            })),
        )
        .await
        .expect("message.send reply");
        let reply: MessageSendResult = serde_json::from_value(reply_value).expect("reply result");

        let list_value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_LIST,
            Some(json!({
                "target": format!("#{}:{}", channel.channel.id, root.message.id),
                "limit": 10,
            })),
        )
        .await
        .expect("message.list");
        let list: MessageListResult = serde_json::from_value(list_value).expect("list result");
        assert_eq!(list.messages.len(), 1);
        assert_eq!(list.messages[0].id, reply.message.id);
        assert_eq!(
            list.messages[0].thread_root_message_id.as_deref(),
            Some(root.message.id.as_str())
        );

        let search_value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEARCH,
            Some(json!({
                "query": "needle",
                "target": format!("#{}:{}", channel.channel.id, root.message.id),
            })),
        )
        .await
        .expect("message.search");
        let search: MessageSearchResult =
            serde_json::from_value(search_value).expect("search result");
        assert_eq!(search.messages.len(), 1);
        assert_eq!(search.messages[0].id, reply.message.id);
    }

    #[tokio::test]
    async fn message_rpc_parses_middle_mention_into_delivery() {
        let state = fresh_state("message_rpc_mention");
        open_conn(&state, "conn_alice", "actor_alice").await;
        state
            .store
            .upsert_actor(Actor {
                id: "actor_agent_reviewer".into(),
                kind: ActorKind::Agent,
                display_name: "Reviewer".into(),
                capabilities: None,
                _meta: None,
            })
            .expect("upsert reviewer");
        let channel_value = dispatch(
            &state,
            "conn_alice",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "backend" })),
        )
        .await
        .expect("channel/create");
        let channel: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel result");
        dispatch(
            &state,
            "conn_alice",
            method::CHANNEL_INVITE,
            Some(json!({
                "channelId": channel.channel.id.clone(),
                "actorId": "actor_agent_reviewer",
            })),
        )
        .await
        .expect("channel/invite");

        let value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEND,
            Some(json!({
                "target": format!("#{}", channel.channel.id),
                "body": "please inspect @Reviewer before merge",
            })),
        )
        .await
        .expect("message.send");
        let sent: MessageSendResult = serde_json::from_value(value).expect("send result");
        assert_eq!(sent.message.mentions.len(), 1);
        assert_eq!(
            sent.message.mentions[0].actor_or_group_id,
            "actor_agent_reviewer"
        );
        let deliveries = state.store.list_deliveries(
            "actor_agent_reviewer",
            Some(DeliveryState::Pending),
            10,
            None,
        );
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].source_id, sent.message.id);

        open_conn(&state, "conn_reviewer", "actor_agent_reviewer").await;
        let inbox_value = dispatch(
            &state,
            "conn_reviewer",
            method::INBOX_LIST,
            Some(json!({
                "actorId": "actor_agent_reviewer",
                "state": "pending",
            })),
        )
        .await
        .expect("inbox.list");
        let inbox: InboxListResult = serde_json::from_value(inbox_value).expect("inbox result");
        assert_eq!(inbox.deliveries.len(), 1);
        assert_eq!(
            inbox.deliveries[0].message.as_ref().map(|m| m.id.as_str()),
            Some(sent.message.id.as_str())
        );

        dispatch(
            &state,
            "conn_reviewer",
            method::DELIVERY_ACK,
            Some(json!({
                "actorId": "actor_agent_reviewer",
                "sourceId": sent.message.id,
            })),
        )
        .await
        .expect("delivery.ack");
        assert!(state
            .store
            .list_deliveries(
                "actor_agent_reviewer",
                Some(DeliveryState::Pending),
                10,
                None,
            )
            .is_empty());
    }

    #[tokio::test]
    async fn actor_group_rpc_create_list_and_mutate_members() {
        let state = fresh_state("actor_group_rpc");
        open_conn(&state, "conn_alice", "actor_alice").await;
        state
            .store
            .upsert_actor(Actor {
                id: "actor_bob".into(),
                kind: ActorKind::Human,
                display_name: "Bob".into(),
                capabilities: None,
                _meta: None,
            })
            .expect("upsert bob");
        state
            .store
            .upsert_actor(Actor {
                id: "actor_agent_reviewer".into(),
                kind: ActorKind::Agent,
                display_name: "Reviewer".into(),
                capabilities: None,
                _meta: None,
            })
            .expect("upsert reviewer");
        let channel_value = dispatch(
            &state,
            "conn_alice",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "backend" })),
        )
        .await
        .expect("channel/create");
        let channel: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel result");
        for actor_id in ["actor_bob", "actor_agent_reviewer"] {
            dispatch(
                &state,
                "conn_alice",
                method::CHANNEL_INVITE,
                Some(json!({
                    "channelId": channel.channel.id.clone(),
                    "actorId": actor_id,
                })),
            )
            .await
            .expect("channel/invite");
        }

        let create_value = dispatch(
            &state,
            "conn_alice",
            method::ACTOR_GROUP_CREATE,
            Some(json!({
                "channelId": channel.channel.id.clone(),
                "name": "Reviewers",
                "memberActorIds": ["actor_bob"],
                "wakeAgents": true,
            })),
        )
        .await
        .expect("actor.group.create");
        let created: ActorGroupCreateResult =
            serde_json::from_value(create_value).expect("group create result");
        assert_eq!(created.group.name, "reviewers");
        assert_eq!(created.group.member_actor_ids, vec!["actor_bob"]);
        assert!(created.group.wake_agents);

        let list_value = dispatch(
            &state,
            "conn_alice",
            method::ACTOR_GROUP_LIST,
            Some(json!({ "channelId": channel.channel.id.clone() })),
        )
        .await
        .expect("actor.group.list");
        let list: ActorGroupListResult =
            serde_json::from_value(list_value).expect("group list result");
        assert_eq!(list.groups.len(), 1);
        assert_eq!(list.groups[0].id, created.group.id);

        let add_value = dispatch(
            &state,
            "conn_alice",
            method::ACTOR_GROUP_ADD_MEMBER,
            Some(json!({
                "groupId": created.group.id,
                "actorId": "actor_agent_reviewer",
            })),
        )
        .await
        .expect("actor.group.add_member");
        let added: ActorGroupMemberResult =
            serde_json::from_value(add_value).expect("group add result");
        assert_eq!(
            added.group.member_actor_ids,
            vec!["actor_bob", "actor_agent_reviewer"]
        );

        let remove_value = dispatch(
            &state,
            "conn_alice",
            method::ACTOR_GROUP_REMOVE_MEMBER,
            Some(json!({
                "groupId": added.group.id,
                "actorId": "actor_bob",
            })),
        )
        .await
        .expect("actor.group.remove_member");
        let removed: ActorGroupMemberResult =
            serde_json::from_value(remove_value).expect("group remove result");
        assert_eq!(removed.group.member_actor_ids, vec!["actor_agent_reviewer"]);
    }

    #[tokio::test]
    async fn run_rpc_open_append_and_close() {
        let state = fresh_state("run_rpc");
        open_conn(&state, "conn_agent", "actor_agent_bot").await;
        let channel_value = dispatch(
            &state,
            "conn_agent",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "runtime" })),
        )
        .await
        .expect("channel/create");
        let channel: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel result");
        let config_value = dispatch(
            &state,
            "conn_agent",
            method::AGENT_CONFIG_PUBLISH,
            Some(json!({
                "actorId": "actor_agent_bot",
                "version": "v1",
                "model": "test-model",
            })),
        )
        .await
        .expect("agent_config.publish");
        let config: AgentConfigPublishResult =
            serde_json::from_value(config_value).expect("config result");

        let open_value = dispatch(
            &state,
            "conn_agent",
            method::RUN_OPEN,
            Some(json!({
                "actorId": "actor_agent_bot",
                "scope": { "kind": "channel", "id": channel.channel.id },
                "startReason": "manual",
                "agentConfigVersionId": config.version.id,
            })),
        )
        .await
        .expect("run.open");
        let opened: RunOpenResult = serde_json::from_value(open_value).expect("run open result");
        assert_eq!(opened.run.status, RunStatus::Queued);

        let append_value = dispatch(
            &state,
            "conn_agent",
            method::RUN_APPEND,
            Some(json!({
                "runId": opened.run.id,
                "status": "running",
                "frameKind": "progress",
                "payload": { "phase": "adapter-started" },
            })),
        )
        .await
        .expect("run.append");
        let appended: RunAppendResult =
            serde_json::from_value(append_value).expect("run append result");
        assert_eq!(appended.run.status, RunStatus::Running);
        assert_eq!(appended.frame.seq, 1);

        let ignore_value = dispatch(
            &state,
            "conn_agent",
            method::RUN_APPEND,
            Some(json!({
                "runId": appended.run.id,
                "frameKind": "control.no_reply",
                "payload": {
                    "reason": "not directed at me",
                    "triggerSourceId": "msg_source"
                },
            })),
        )
        .await
        .expect("run.append control.no_reply");
        let ignored: RunAppendResult =
            serde_json::from_value(ignore_value).expect("run ignore result");
        assert_eq!(
            ignored
                .run
                .metadata
                .get("noReply")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            ignored
                .run
                .metadata
                .get("replyMode")
                .and_then(serde_json::Value::as_str),
            Some("none")
        );
        assert_eq!(ignored.frame.kind, "control.no_reply");

        let close_value = dispatch(
            &state,
            "conn_agent",
            method::RUN_CLOSE,
            Some(json!({
                "runId": ignored.run.id,
                "status": "completed",
            })),
        )
        .await
        .expect("run.close");
        let closed: RunCloseResult = serde_json::from_value(close_value).expect("run close result");
        assert_eq!(closed.run.status, RunStatus::Completed);
        assert!(closed.run.closed_at.is_some());
    }

    #[tokio::test]
    async fn run_cancel_closes_run_and_wakes_agent() {
        let state = fresh_state("run_cancel");
        open_conn(&state, "conn_agent", "actor_agent_bot").await;
        open_conn(&state, "conn_human", "actor_human").await;
        let channel_value = dispatch(
            &state,
            "conn_agent",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "runtime" })),
        )
        .await
        .expect("channel/create");
        let channel: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel result");
        state
            .store
            .grant_channel(&channel.channel.id, "actor_human")
            .expect("grant human");
        let config_value = dispatch(
            &state,
            "conn_agent",
            method::AGENT_CONFIG_PUBLISH,
            Some(json!({
                "actorId": "actor_agent_bot",
                "version": "v1",
                "model": "test-model",
            })),
        )
        .await
        .expect("agent_config.publish");
        let config: AgentConfigPublishResult =
            serde_json::from_value(config_value).expect("config result");
        let open_value = dispatch(
            &state,
            "conn_agent",
            method::RUN_OPEN,
            Some(json!({
                "actorId": "actor_agent_bot",
                "scope": { "kind": "channel", "id": channel.channel.id },
                "startReason": "manual",
                "agentConfigVersionId": config.version.id,
            })),
        )
        .await
        .expect("run.open");
        let opened: RunOpenResult = serde_json::from_value(open_value).expect("run open result");

        let cancel_value = dispatch(
            &state,
            "conn_human",
            method::RUN_CANCEL,
            Some(json!({
                "runId": opened.run.id,
                "reason": "operator requested stop",
            })),
        )
        .await
        .expect("run.cancel");
        let canceled: RunCancelResult =
            serde_json::from_value(cancel_value).expect("run cancel result");
        assert_eq!(canceled.run.status, RunStatus::Canceled);
        let message = canceled.cancel_message.expect("cancel message");
        assert_eq!(
            message.metadata.get("kind").and_then(Value::as_str),
            Some("run.cancel")
        );
        assert_eq!(
            message.metadata.get("runId").and_then(Value::as_str),
            Some(canceled.run.id.as_str())
        );
        assert!(message.audience.iter().any(
            |audience| audience.kind == AudienceKind::Actor && audience.id == "actor_agent_bot"
        ));

        let inbox_value = dispatch(
            &state,
            "conn_agent",
            method::INBOX_LIST,
            Some(json!({ "actorId": "actor_agent_bot" })),
        )
        .await
        .expect("inbox.list");
        let inbox: InboxListResult = serde_json::from_value(inbox_value).expect("inbox result");
        assert!(inbox
            .deliveries
            .iter()
            .any(|entry| entry.message.as_ref().is_some_and(|m| m.id == message.id)));
    }

    #[tokio::test]
    async fn coordination_rpc_sequential_steps() {
        let state = fresh_state("coordination_rpc");
        open_conn(&state, "conn_owner", "actor_owner").await;
        open_conn(&state, "conn_a", "actor_a").await;
        open_conn(&state, "conn_b", "actor_b").await;
        let channel_value = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_CREATE,
            Some(json!({ "title": "coord" })),
        )
        .await
        .expect("channel/create");
        let channel: ChannelCreateResult =
            serde_json::from_value(channel_value).expect("channel result");
        for actor_id in ["actor_a", "actor_b"] {
            dispatch(
                &state,
                "conn_owner",
                method::CHANNEL_INVITE,
                Some(json!({
                    "channelId": channel.channel.id.clone(),
                    "actorId": actor_id,
                })),
            )
            .await
            .expect("channel/invite");
        }

        let proposed_value = dispatch(
            &state,
            "conn_owner",
            method::COORDINATION_PROPOSE,
            Some(json!({
                "target": format!("#{}", channel.channel.id),
                "mode": "sequential",
                "decisionRule": "owner_decides",
                "participants": ["actor_a", "actor_b"],
                "plan": { "goal": "count" },
            })),
        )
        .await
        .expect("coordination.propose");
        let proposed: CoordinationProposeResult =
            serde_json::from_value(proposed_value).expect("propose result");

        let committed_value = dispatch(
            &state,
            "conn_owner",
            method::COORDINATION_COMMIT,
            Some(json!({ "sessionId": proposed.session.id })),
        )
        .await
        .expect("coordination.commit");
        let committed: CoordinationCommitResult =
            serde_json::from_value(committed_value).expect("commit result");
        assert_eq!(
            committed.session.baton_holder_actor_id.as_deref(),
            Some("actor_a")
        );

        let step_value = dispatch(
            &state,
            "conn_a",
            method::COORDINATION_STEP,
            Some(json!({
                "sessionId": committed.session.id,
                "baseRevision": 0,
                "output": { "count": 1 },
                "messageBody": "A counted 1",
            })),
        )
        .await
        .expect("coordination.step a");
        let step: CoordinationStepResult = serde_json::from_value(step_value).expect("step result");
        assert_eq!(step.session.revision, 1);
        assert_eq!(
            step.session.baton_holder_actor_id.as_deref(),
            Some("actor_b")
        );
        assert!(step.message.is_some());

        let done_value = dispatch(
            &state,
            "conn_b",
            method::COORDINATION_STEP,
            Some(json!({
                "sessionId": step.session.id,
                "baseRevision": 1,
                "output": { "count": 2 },
            })),
        )
        .await
        .expect("coordination.step b");
        let done: CoordinationStepResult = serde_json::from_value(done_value).expect("done result");
        assert_eq!(done.session.status, CoordinationStatus::Done);
    }

    fn append_channel_root(
        state: &AppState,
        channel_id: &str,
        actor_id: &str,
        text: &str,
    ) -> String {
        state
            .store
            .append_message(
                actor_id.into(),
                format!("#{channel_id}"),
                MessageKind::Human,
                text.into(),
                Vec::new(),
                Vec::new(),
                MessageIntent::Chat,
                DeliveryPolicy::NotifyOnly,
                None,
                None,
                Vec::new(),
                Meta::default(),
                None,
            )
            .expect("append root message")
            .id
    }

    fn create_thread_under(
        state: &AppState,
        channel_id: &str,
        actor_id: &str,
        title: &str,
    ) -> proto::types::Thread {
        let root_message_id = append_channel_root(state, channel_id, actor_id, title);
        state
            .store
            .create_thread(channel_id.into(), title.into(), root_message_id)
            .expect("create thread")
    }

    #[tokio::test]
    async fn thread_follow_rpc_delivers_thread_replies_until_unfollow() {
        let state = fresh_state("thread-follow-rpc");
        open_conn(&state, "conn_owner", "actor_owner").await;
        open_conn(&state, "conn_bob", "actor_bob").await;
        let channel = state
            .store
            .create_channel("backend".into(), Some("actor_owner".into()))
            .unwrap();
        state.store.grant_channel(&channel.id, "actor_bob").unwrap();
        let root_message_id = append_channel_root(&state, &channel.id, "actor_owner", "root");
        let thread = state
            .store
            .create_thread(channel.id.clone(), "root".into(), root_message_id.clone())
            .unwrap();

        let value = dispatch(
            &state,
            "conn_bob",
            method::THREAD_FOLLOW,
            Some(json!({ "threadId": thread.id })),
        )
        .await
        .expect("thread.follow");
        let followed: ThreadFollowResult = serde_json::from_value(value).expect("follow result");
        assert!(followed.presence.following);

        let reply = state
            .store
            .append_message(
                "actor_owner".into(),
                format!("#{}:{}", channel.id, root_message_id),
                MessageKind::Human,
                "reply".into(),
                Vec::new(),
                Vec::new(),
                MessageIntent::Chat,
                DeliveryPolicy::NotifyOnly,
                None,
                None,
                Vec::new(),
                Meta::default(),
                None,
            )
            .unwrap();
        assert!(state
            .store
            .list_deliveries("actor_bob", Some(DeliveryState::Pending), 10, None)
            .iter()
            .any(|delivery| delivery.source_id == reply.id));

        dispatch(
            &state,
            "conn_bob",
            method::THREAD_UNFOLLOW,
            Some(json!({ "threadId": thread.id })),
        )
        .await
        .expect("thread.unfollow");
        let silent = state
            .store
            .append_message(
                "actor_owner".into(),
                format!("#{}:{}", channel.id, root_message_id),
                MessageKind::Human,
                "reply after unfollow".into(),
                Vec::new(),
                Vec::new(),
                MessageIntent::Chat,
                DeliveryPolicy::NotifyOnly,
                None,
                None,
                Vec::new(),
                Meta::default(),
                None,
            )
            .unwrap();
        assert!(!state
            .store
            .list_deliveries("actor_bob", Some(DeliveryState::Pending), 10, None)
            .iter()
            .any(|delivery| delivery.source_id == silent.id));
    }

    #[tokio::test]
    async fn task_shortcut_rpcs_update_state_and_append_task_message() {
        let state = fresh_state("task-shortcuts-rpc");
        open_conn(&state, "conn_owner", "actor_owner").await;
        open_conn(&state, "conn_reviewer", "actor_reviewer").await;
        let channel = state
            .store
            .create_channel("backend".into(), Some("actor_owner".into()))
            .unwrap();
        state
            .store
            .grant_channel(&channel.id, "actor_reviewer")
            .unwrap();
        let root_message_id = append_channel_root(&state, &channel.id, "actor_owner", "fix bug");
        let created_value = dispatch(
            &state,
            "conn_owner",
            method::TASK_CREATE,
            Some(json!({
                "sourceMessageId": root_message_id,
                "title": "fix bug",
                "description": "fix it",
                "requesterActorId": "actor_owner",
            })),
        )
        .await
        .expect("task.create");
        let created: TaskCreateResult = serde_json::from_value(created_value).unwrap();

        let claimed_value = dispatch(
            &state,
            "conn_reviewer",
            method::TASK_CLAIM,
            Some(json!({ "taskId": created.task.id })),
        )
        .await
        .expect("task.claim");
        let claimed: TaskUpdateResult = serde_json::from_value(claimed_value).unwrap();
        assert_eq!(claimed.task.status, TaskStatus::Claimed);
        assert_eq!(
            claimed.task.owner_actor_id.as_deref(),
            Some("actor_reviewer")
        );

        let completed_value = dispatch(
            &state,
            "conn_reviewer",
            method::TASK_COMPLETE,
            Some(json!({ "taskId": created.task.id, "resultSummary": "done" })),
        )
        .await
        .expect("task.complete");
        let completed: TaskUpdateResult = serde_json::from_value(completed_value).unwrap();
        assert_eq!(completed.task.status, TaskStatus::Done);
        assert_eq!(completed.task.result_summary, "done");

        let (messages, _) = state
            .store
            .read_messages_for_target(
                "actor_owner",
                &format!("#{}:{}", channel.id, root_message_id),
                20,
                None,
            )
            .unwrap();
        let task_updates = messages
            .iter()
            .filter(|message| message.kind == MessageKind::TaskUpdate)
            .filter(|message| {
                message.metadata.get("taskId").and_then(Value::as_str)
                    == Some(created.task.id.as_str())
            })
            .count();
        assert!(task_updates >= 2);
    }

    #[tokio::test]
    async fn task_claim_by_source_message_is_owner_cas() {
        let state = fresh_state("task-claim-source-cas");
        open_conn(&state, "conn_owner", "actor_owner").await;
        open_conn(&state, "conn_reviewer", "actor_reviewer").await;
        let channel = state
            .store
            .create_channel("backend".into(), Some("actor_owner".into()))
            .unwrap();
        state
            .store
            .grant_channel(&channel.id, "actor_reviewer")
            .unwrap();
        let root_message_id = append_channel_root(&state, &channel.id, "actor_owner", "please fix");

        let claimed_value = dispatch(
            &state,
            "conn_reviewer",
            method::TASK_CLAIM,
            Some(json!({ "sourceMessageId": root_message_id })),
        )
        .await
        .expect("first source claim");
        let claimed: TaskUpdateResult = serde_json::from_value(claimed_value).unwrap();
        assert_eq!(claimed.task.status, TaskStatus::Claimed);
        assert_eq!(
            claimed.task.owner_actor_id.as_deref(),
            Some("actor_reviewer")
        );

        let reclaim_err = dispatch(
            &state,
            "conn_owner",
            method::TASK_CLAIM,
            Some(json!({ "sourceMessageId": claimed.task.source_message_id })),
        )
        .await
        .expect_err("other owner cannot steal claim");
        assert_eq!(reclaim_err.code, ErrorCode::APP_CONFLICT);

        let same_owner = dispatch(
            &state,
            "conn_reviewer",
            method::TASK_CLAIM,
            Some(json!({ "sourceMessageId": claimed.task.source_message_id })),
        )
        .await
        .expect("same owner reclaim is idempotent");
        let same_owner: TaskUpdateResult = serde_json::from_value(same_owner).unwrap();
        assert_eq!(same_owner.task.id, claimed.task.id);
    }

    #[tokio::test]
    async fn message_send_if_latest_rejects_stale_base() {
        let state = fresh_state("message-send-if-latest");
        open_conn(&state, "conn_alice", "actor_alice").await;
        open_conn(&state, "conn_bob", "actor_bob").await;
        let channel = state
            .store
            .create_channel("chat".into(), Some("actor_alice".into()))
            .unwrap();
        state.store.grant_channel(&channel.id, "actor_bob").unwrap();

        let first_value = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEND,
            Some(json!({
                "target": format!("#{}", channel.id),
                "body": "first",
            })),
        )
        .await
        .expect("first send");
        let first: MessageSendResult = serde_json::from_value(first_value).unwrap();

        let second_value = dispatch(
            &state,
            "conn_bob",
            method::MESSAGE_SEND,
            Some(json!({
                "target": format!("#{}", channel.id),
                "body": "second",
                "ifLatestMessageId": first.message.id.clone(),
            })),
        )
        .await
        .expect("send against latest base");
        let second: MessageSendResult = serde_json::from_value(second_value).unwrap();

        let stale_err = dispatch(
            &state,
            "conn_alice",
            method::MESSAGE_SEND,
            Some(json!({
                "target": format!("#{}", channel.id),
                "body": "stale",
                "ifLatestMessageId": first.message.id.clone(),
            })),
        )
        .await
        .expect_err("stale send should conflict");
        assert_eq!(stale_err.code, ErrorCode::APP_CONFLICT);

        let messages = state
            .store
            .read_messages_for_target("actor_alice", &format!("#{}", channel.id), 10, None)
            .unwrap()
            .0;
        assert_eq!(
            messages.last().map(|message| message.id.as_str()),
            Some(second.message.id.as_str())
        );
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
    async fn task_assignment_create_wakes_recipient_in_canonical_thread() {
        let state = fresh_state("task-assignment-create");
        open_conn(&state, "conn_owner", "actor_owner").await;
        let channel = state
            .store
            .create_channel("tasks".into(), Some("actor_owner".into()))
            .expect("create channel");
        state
            .store
            .upsert_actor(Actor {
                id: "actor_reviewer".into(),
                kind: ActorKind::Agent,
                display_name: "Reviewer".into(),
                capabilities: None,
                _meta: None,
            })
            .expect("upsert reviewer");
        state
            .store
            .grant_channel(&channel.id, "actor_reviewer")
            .expect("grant reviewer");
        let source_message_id =
            append_channel_root(&state, &channel.id, "actor_owner", "write story");

        let created = dispatch(
            &state,
            "conn_owner",
            method::TASK_CREATE,
            Some(json!({
                "sourceMessageId": source_message_id,
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
                "instruction": "review the story",
                "contract": {}
            })),
        )
        .await
        .expect("task/assignment.create");
        let assigned: TaskAssignmentCreateResult = serde_json::from_value(assigned).unwrap();

        assert_eq!(assigned.message.scope.kind, ScopeKind::Thread);
        assert_eq!(assigned.message.scope.id, created.task.canonical_thread_id);
        assert_eq!(assigned.message.intent, MessageIntent::AssignTask);
        assert_eq!(assigned.message.delivery_policy, DeliveryPolicy::WakeAgent);
        assert!(assigned.message.audience.iter().any(|audience| {
            matches!(audience.kind, AudienceKind::Actor) && audience.id == "actor_reviewer"
        }));
        let meta = &assigned.message.metadata;
        assert_eq!(
            meta.get("assignmentId").and_then(|value| value.as_str()),
            Some(assigned.assignment.id.as_str())
        );
    }

    #[tokio::test]
    async fn task_assignment_create_does_not_wake_actors_mentioned_in_task_title() {
        let state = fresh_state("task-assignment-title-mentions");
        open_conn(&state, "conn_owner", "actor_owner").await;
        let channel = state
            .store
            .create_channel("tasks".into(), Some("actor_owner".into()))
            .expect("create channel");
        state
            .store
            .upsert_actor(Actor {
                id: "actor_reviewer".into(),
                kind: ActorKind::Agent,
                display_name: "Reviewer".into(),
                capabilities: None,
                _meta: None,
            })
            .expect("upsert reviewer");
        state
            .store
            .grant_channel(&channel.id, "actor_reviewer")
            .expect("grant reviewer");
        let source_message_id = append_channel_root(
            &state,
            &channel.id,
            "actor_owner",
            "@actor_owner please handle this",
        );

        let created = dispatch(
            &state,
            "conn_owner",
            method::TASK_CREATE,
            Some(json!({
                "sourceMessageId": source_message_id,
                "title": "@actor_owner please handle this",
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
                "instruction": "review the story",
                "contract": {}
            })),
        )
        .await
        .expect("task/assignment.create");
        let assigned: TaskAssignmentCreateResult = serde_json::from_value(assigned).unwrap();

        assert!(assigned.message.mentions.iter().any(|mention| {
            mention.kind == MessageMentionKind::Actor && mention.actor_or_group_id == "actor_owner"
        }));
        assert_eq!(assigned.message.audience.len(), 1);
        assert_eq!(assigned.message.audience[0].kind, AudienceKind::Actor);
        assert_eq!(assigned.message.audience[0].id, "actor_reviewer");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn task_assignment_create_idempotent_race_returns_existing_message() {
        let state = fresh_state("task-assignment-create-idempotent-race");
        open_conn(&state, "conn_owner", "actor_owner").await;
        let channel = state
            .store
            .create_channel("tasks".into(), Some("actor_owner".into()))
            .expect("create channel");
        state
            .store
            .upsert_actor(Actor {
                id: "actor_reviewer".into(),
                kind: ActorKind::Agent,
                display_name: "Reviewer".into(),
                capabilities: None,
                _meta: None,
            })
            .expect("upsert reviewer");
        state
            .store
            .grant_channel(&channel.id, "actor_reviewer")
            .expect("grant reviewer");
        let source_message_id =
            append_channel_root(&state, &channel.id, "actor_owner", "write story");
        let created = dispatch(
            &state,
            "conn_owner",
            method::TASK_CREATE,
            Some(json!({
                "sourceMessageId": source_message_id,
                "title": "write story",
                "ownerActorId": "actor_owner"
            })),
        )
        .await
        .expect("task/create");
        let created: TaskCreateResult = serde_json::from_value(created).unwrap();

        let mut handles = Vec::new();
        for _ in 0..12 {
            let state = state.clone();
            let task_id = created.task.id.clone();
            handles.push(tokio::spawn(async move {
                dispatch(
                    &state,
                    "conn_owner",
                    method::TASK_ASSIGNMENT_CREATE,
                    Some(json!({
                        "taskId": task_id,
                        "toActorId": "actor_reviewer",
                        "type": "review",
                        "instruction": "review the story",
                        "contract": {"idempotency_key": "story-review"},
                        "idempotencyKey": "story-review"
                    })),
                )
                .await
            }));
        }

        let mut assignments = std::collections::HashSet::new();
        let mut messages = std::collections::HashSet::new();
        for handle in handles {
            let value = handle.await.expect("join").expect("task/assignment.create");
            let assigned: TaskAssignmentCreateResult = serde_json::from_value(value).unwrap();
            assignments.insert(assigned.assignment.id);
            messages.insert(assigned.message.id);
        }
        assert_eq!(assignments.len(), 1);
        assert_eq!(messages.len(), 1);

        let (thread_messages, _) = state
            .store
            .read_messages_for_target(
                "actor_owner",
                &format!(
                    "#{}:{}",
                    created.task.channel_id, created.task.source_message_id
                ),
                50,
                None,
            )
            .expect("read messages");
        let assignment_message_count = thread_messages
            .iter()
            .filter(|message| {
                message
                    .metadata
                    .get("assignmentId")
                    .and_then(serde_json::Value::as_str)
                    == assignments.iter().next().map(String::as_str)
            })
            .count();
        assert_eq!(assignment_message_count, 1);
    }

    #[tokio::test]
    async fn task_assignment_update_sends_terminal_result_back_to_assigner() {
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
        let source_message_id =
            append_channel_root(&state, &channel.id, "actor_owner", "write story");

        let created = dispatch(
            &state,
            "conn_owner",
            method::TASK_CREATE,
            Some(json!({
                "sourceMessageId": source_message_id,
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
                "instruction": "review the story",
                "contract": {}
            })),
        )
        .await
        .expect("task/assignment.create");
        let assigned: TaskAssignmentCreateResult = serde_json::from_value(assigned).unwrap();
        let result_message = state
            .store
            .append_message(
                "actor_reviewer".into(),
                format!(
                    "#{}:{}",
                    created.task.channel_id, created.task.source_message_id
                ),
                MessageKind::Agent,
                "looks good".into(),
                Vec::new(),
                Vec::new(),
                MessageIntent::Review,
                DeliveryPolicy::NotifyOnly,
                None,
                None,
                Vec::new(),
                Meta::default(),
                None,
            )
            .expect("result message");

        let updated = dispatch(
            &state,
            "conn_reviewer",
            method::TASK_ASSIGNMENT_UPDATE,
            Some(json!({
                "assignmentId": assigned.assignment.id,
                "status": "running"
            })),
        )
        .await
        .expect("task/assignment.update running");
        let updated: TaskAssignmentUpdateResult = serde_json::from_value(updated).unwrap();
        assert_eq!(updated.assignment.status, TaskAssignmentStatus::Running);

        let updated = dispatch(
            &state,
            "conn_reviewer",
            method::TASK_ASSIGNMENT_UPDATE,
            Some(json!({
                "assignmentId": assigned.assignment.id,
                "status": "completed",
                "resultMessageId": result_message.id.clone(),
                "resultSummary": "looks good",
                "resultEnvelope": {
                    "assignment_id": assigned.assignment.id,
                    "status": "completed",
                    "verdict": "pass",
                    "evidence_refs": [result_message.id.clone()]
                }
            })),
        )
        .await
        .expect("task/assignment.update");
        let updated: TaskAssignmentUpdateResult = serde_json::from_value(updated).unwrap();
        assert_eq!(updated.assignment.status, TaskAssignmentStatus::Completed);

        let (messages, _) = state
            .store
            .read_messages_for_target(
                "actor_delegate",
                &format!(
                    "#{}:{}",
                    created.task.channel_id, created.task.source_message_id
                ),
                20,
                None,
            )
            .expect("read messages");
        let callback = messages
            .iter()
            .rev()
            .find(|message| {
                message.author_actor_id == "actor_reviewer"
                    && message.audience.iter().any(|audience| {
                        matches!(audience.kind, AudienceKind::Actor)
                            && audience.id == "actor_delegate"
                    })
                    && message.body.contains("Task #")
            })
            .expect("assignment return message");
        let meta = &callback.metadata;
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
            .any(|delivery| delivery.source_id == callback.id));
        let owner_deliveries =
            state
                .store
                .list_deliveries("actor_owner", Some(DeliveryState::Pending), 20, None);
        assert!(!owner_deliveries
            .iter()
            .any(|delivery| delivery.source_id == callback.id));
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
            method::MESSAGE_SEND,
            Some(json!({
                "target": format!("#{}", channel.channel.id),
                "body": "root for attachment exchange"
            })),
        )
        .await
        .expect("root message");
        let root: MessageSendResult = serde_json::from_value(root).expect("root result");

        let thread = dispatch(
            &state,
            "conn_human",
            method::THREAD_CREATE,
            Some(json!({
                "channelId": channel.channel.id,
                "rootMessageId": root.message.id,
                "title": "attachment thread"
            })),
        )
        .await
        .expect("thread/create");
        let thread: ThreadCreateResult = serde_json::from_value(thread).expect("thread result");
        let thread_scope = json!({ "kind": "thread", "id": thread.thread.id });
        let thread_target = format!("#{}:{}", channel.channel.id, root.message.id);
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

        let human_message = dispatch(
            &state,
            "conn_human",
            method::MESSAGE_SEND,
            Some(json!({
                "target": thread_target.clone(),
                "body": "human attached file",
                "attachments": [human_artifact.artifact.id],
                "audience": [{ "kind": "actor", "id": "actor_agent" }],
                "intent": "request_action",
                "deliveryPolicy": "wake_agent",
                "metadata": { "kind": "content.add", "contentType": "text/markdown" }
            })),
        )
        .await
        .expect("human message.send");
        let human_message: MessageSendResult =
            serde_json::from_value(human_message).expect("human message result");
        assert!(human_message
            .message
            .attachments
            .iter()
            .any(|id| id == &human_artifact.artifact.id));
        assert!(human_message
            .message
            .audience
            .iter()
            .any(|audience| audience.kind == AudienceKind::Actor && audience.id == "actor_agent"));

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

        let agent_message = dispatch(
            &state,
            "conn_agent",
            method::MESSAGE_SEND,
            Some(json!({
                "target": thread_target.clone(),
                "body": "agent sent file",
                "attachments": [agent_artifact.artifact.id],
                "metadata": { "kind": "content.add", "contentType": "text/markdown" }
            })),
        )
        .await
        .expect("agent message.send");
        let agent_message: MessageSendResult =
            serde_json::from_value(agent_message).expect("agent message result");
        assert!(agent_message
            .message
            .attachments
            .iter()
            .any(|id| id == &agent_artifact.artifact.id));

        let history = dispatch(
            &state,
            "conn_human",
            method::MESSAGE_LIST,
            Some(json!({
                "target": thread_target,
                "limit": 100
            })),
        )
        .await
        .expect("message.list");
        let history: MessageListResult = serde_json::from_value(history).expect("history result");
        assert!(history
            .messages
            .iter()
            .any(|message| message.id == human_message.message.id));
        assert!(history
            .messages
            .iter()
            .any(|message| message.id == agent_message.message.id));

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
    async fn channel_delete_without_cascade_refuses_non_empty_channel() {
        let state = fresh_state("channel-delete-no-cascade");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        create_thread_under(&state, &channel.id, "actor_owner", "child");
        open_conn(&state, "conn_owner", "actor_owner").await;

        let err = dispatch(
            &state,
            "conn_owner",
            method::CHANNEL_DELETE,
            Some(json!({ "channelId": channel.id })),
        )
        .await
        .expect_err("delete should require explicit cascade");

        assert_eq!(err.code, ErrorCode::APP_CONFLICT);
        assert!(state.store.get_channel(&channel.id).is_some());
        assert_eq!(state.store.list_threads(Some(&channel.id)).len(), 1);
    }

    #[tokio::test]
    async fn thread_create_refuses_non_member_in_private_channel() {
        let state = fresh_state("auto");
        let channel = state
            .store
            .create_channel("private".into(), Some("actor_owner".into()))
            .expect("create channel");
        let root_message_id = append_channel_root(&state, &channel.id, "actor_owner", "sneaky");
        open_conn(&state, "conn_intruder", "actor_intruder").await;

        let err = dispatch(
            &state,
            "conn_intruder",
            method::THREAD_CREATE,
            Some(
                json!({ "channelId": channel.id, "rootMessageId": root_message_id, "title": "sneaky" }),
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
        let root_message_id = append_channel_root(&state, &channel.id, "actor_owner", "ok");
        open_conn(&state, "conn_owner", "actor_owner").await;

        let value = dispatch(
            &state,
            "conn_owner",
            method::THREAD_CREATE,
            Some(
                json!({ "channelId": channel.id, "rootMessageId": root_message_id, "title": "ok" }),
            ),
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
    async fn thread_list_includes_reply_summary_metadata() {
        let state = fresh_state("thread-summary");
        let channel = state
            .store
            .create_channel("summary".into(), Some("actor_alice".into()))
            .expect("create channel");
        state.store.grant_channel(&channel.id, "actor_bob").unwrap();
        state
            .store
            .grant_channel(&channel.id, "actor_charlie")
            .unwrap();
        let thread = create_thread_under(&state, &channel.id, "actor_alice", "root");
        let target = format!("#{}:{}", channel.id, thread.root_message_id);
        for (actor_id, body) in [
            ("actor_bob", "first reply"),
            ("actor_alice", "second reply"),
        ] {
            state
                .store
                .append_message(
                    actor_id.into(),
                    target.clone(),
                    MessageKind::Human,
                    body.into(),
                    Vec::new(),
                    Vec::new(),
                    MessageIntent::Chat,
                    DeliveryPolicy::NotifyOnly,
                    None,
                    None,
                    Vec::new(),
                    Meta::default(),
                    None,
                )
                .expect("append thread reply");
        }

        open_conn(&state, "conn_alice", "actor_alice").await;
        let value = dispatch(
            &state,
            "conn_alice",
            method::THREAD_LIST,
            Some(json!({ "channelId": channel.id })),
        )
        .await
        .expect("thread/list");
        let result: ThreadListResult = serde_json::from_value(value).expect("list result");
        let listed = result
            .threads
            .iter()
            .find(|candidate| candidate.id == thread.id)
            .expect("listed thread");
        let meta = listed._meta.as_ref().expect("thread summary meta");
        assert_eq!(meta.get("replyCount").and_then(Value::as_u64), Some(2));
        let participant_ids = meta
            .get("participantActorIds")
            .and_then(Value::as_array)
            .expect("participantActorIds");
        assert_eq!(participant_ids.len(), 2);
        assert!(participant_ids
            .iter()
            .any(|id| id.as_str() == Some("actor_alice")));
        assert!(participant_ids
            .iter()
            .any(|id| id.as_str() == Some("actor_bob")));
        assert!(!participant_ids
            .iter()
            .any(|id| id.as_str() == Some("actor_charlie")));
        assert!(meta.get("lastReplyAt").and_then(Value::as_str).is_some());
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

    // ---- inbox.list (§9.2 durable inbox) ----

    fn append_directed_message(state: &AppState, channel_id: &str, from: &str, to: &str) -> String {
        for actor_id in [from, to] {
            state
                .store
                .upsert_actor(Actor {
                    id: actor_id.into(),
                    kind: ActorKind::Agent,
                    display_name: actor_id.into(),
                    capabilities: None,
                    _meta: None,
                })
                .expect("upsert inbox test actor");
        }
        state
            .store
            .append_message(
                from.into(),
                format!("#{channel_id}"),
                MessageKind::Agent,
                "hi".into(),
                vec![],
                vec![AudienceRef {
                    kind: AudienceKind::Actor,
                    id: to.into(),
                    display: None,
                }],
                MessageIntent::Chat,
                DeliveryPolicy::WakeAgent,
                None,
                None,
                vec![],
                Meta::new(),
                None,
            )
            .expect("append message")
            .id
    }

    #[tokio::test]
    async fn inbox_list_returns_pending_for_caller_inbox() {
        // Baseline: a directed message writes a Pending delivery row for the
        // target. The bound caller can list it back with the inline message
        // payload (no follow-up history read needed).
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
        let message_id = append_directed_message(&state, &ch.id, "svc_writer", "actor_target");
        open_conn(&state, "conn_t", "actor_target").await;

        let value = dispatch(
            &state,
            "conn_t",
            method::INBOX_LIST,
            Some(json!({ "actorId": "actor_target", "state": "pending" })),
        )
        .await
        .expect("inbox.list ok");
        let res: InboxListResult = serde_json::from_value(value).expect("decode");

        assert_eq!(res.deliveries.len(), 1);
        let entry = &res.deliveries[0];
        assert_eq!(entry.delivery.source_id, message_id);
        assert_eq!(entry.delivery.actor_id, "actor_target");
        assert!(matches!(entry.delivery.state, DeliveryState::Pending));
        let message = entry.message.as_ref().expect("inline message payload");
        assert_eq!(message.id, message_id);
        assert!(res.next_cursor.is_none(), "single page");
    }

    #[tokio::test]
    async fn inbox_list_returns_pending_event_payload_for_caller_inbox() {
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
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: ch.id.clone(),
        };
        let event = state
            .store
            .append_event(
                "reminder.fire".into(),
                "svc_writer".into(),
                scope,
                None,
                json!({"title": "wake up"}),
                vec![Relation {
                    kind: RelationKind::DirectedTo,
                    target: Ref {
                        kind: RefKind::Actor,
                        id: "actor_target".into(),
                        _meta: None,
                    },
                    _meta: None,
                }],
                None,
            )
            .expect("append event");
        open_conn(&state, "conn_t", "actor_target").await;

        let value = dispatch(
            &state,
            "conn_t",
            method::INBOX_LIST,
            Some(json!({ "actorId": "actor_target", "state": "pending" })),
        )
        .await
        .expect("inbox.list ok");
        let res: InboxListResult = serde_json::from_value(value).expect("decode");

        assert_eq!(res.deliveries.len(), 1);
        let entry = &res.deliveries[0];
        assert_eq!(entry.delivery.source_id, event.id);
        assert!(entry.message.is_none());
        assert_eq!(
            entry.event.as_ref().map(|event| event.kind.as_str()),
            Some("reminder.fire")
        );
    }

    #[tokio::test]
    async fn inbox_list_filters_by_state_after_delivery_ack() {
        // delivery.ack advances Pending -> Delivered. After that the
        // pending filter must drop the row, and the delivered filter picks
        // it up. Proves state is a real index, not a noop.
        let state = fresh_state("auto");
        let ch = state.store.create_channel("c".into(), None).expect("ch");
        state.store.grant_channel(&ch.id, "svc_writer").expect("gw");
        state
            .store
            .grant_channel(&ch.id, "actor_target")
            .expect("gt");
        let message_id = append_directed_message(&state, &ch.id, "svc_writer", "actor_target");
        open_conn(&state, "conn_t", "actor_target").await;
        let _: DeliveryAckResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::DELIVERY_ACK,
                Some(json!({ "actorId": "actor_target", "sourceId": message_id.clone() })),
            )
            .await
            .expect("ack ok"),
        )
        .expect("decode ack");

        let pending: InboxListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::INBOX_LIST,
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

        let delivered: InboxListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::INBOX_LIST,
                Some(json!({ "actorId": "actor_target", "state": "delivered" })),
            )
            .await
            .expect("delivered ok"),
        )
        .expect("decode");
        assert_eq!(delivered.deliveries.len(), 1);
        assert_eq!(delivered.deliveries[0].delivery.source_id, message_id);
    }

    #[tokio::test]
    async fn inbox_list_paginates_with_cursor() {
        // Three deliveries, limit=2. First page returns 2 + cursor; second
        // page returns the remaining 1 with no cursor. Together they cover
        // every source id exactly once, order-independent so the test is
        // resilient to clock-resolution ties.
        let state = fresh_state("auto");
        let ch = state.store.create_channel("c".into(), None).expect("ch");
        state.store.grant_channel(&ch.id, "svc_writer").expect("gw");
        state
            .store
            .grant_channel(&ch.id, "actor_target")
            .expect("gt");
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(append_directed_message(
                &state,
                &ch.id,
                "svc_writer",
                "actor_target",
            ));
        }
        open_conn(&state, "conn_t", "actor_target").await;

        let page1: InboxListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::INBOX_LIST,
                Some(json!({ "actorId": "actor_target", "limit": 2 })),
            )
            .await
            .expect("p1"),
        )
        .expect("decode");
        assert_eq!(page1.deliveries.len(), 2);
        let cursor = page1.next_cursor.expect("more rows -> cursor present");

        let page2: InboxListResult = serde_json::from_value(
            dispatch(
                &state,
                "conn_t",
                method::INBOX_LIST,
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
            .map(|e| e.delivery.source_id.clone())
            .collect();
        seen.sort();
        let mut want = ids;
        want.sort();
        assert_eq!(seen, want, "pages must cover every id exactly once");
    }

    #[tokio::test]
    async fn inbox_list_refuses_other_actors_inbox() {
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
        append_directed_message(&state, &ch.id, "svc_writer", "actor_target");
        open_conn(&state, "conn_intruder", "actor_intruder").await;

        let err = dispatch(
            &state,
            "conn_intruder",
            method::INBOX_LIST,
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
    async fn inbox_list_requires_bound_actor() {
        // A connection that never called connection/open has no actor
        // identity; inbox.list cannot pick a default and must refuse.
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
            method::INBOX_LIST,
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
    async fn legacy_machine_command_cancels_when_not_delivered() {
        let state = fresh_state("machine-command-wrapper-cancel");
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
                        "ownerActorId": "actor_human",
                        "capabilities": ["inventory.read", "machine.command"],
                        "revision": 11
                    }
                }
            })),
        )
        .await
        .expect("actor/upsert machine");

        let err = dispatch(
            &state,
            "conn_human",
            method::MACHINE_COMMAND,
            Some(json!({
                "machineId": "machine_remote",
                "machineActorId": "actor_service_machine_remote",
                "ifInventoryRevision": 11,
                "command": { "op": "agent.remove", "actorId": "actor_agent" },
                "timeoutMs": 5_000
            })),
        )
        .await
        .expect_err("wrapper should fail when daemon is offline");
        assert_eq!(err.code, ErrorCode::APP_INVALID_STATE);

        let commands = state.store.list_machine_commands(
            Some("machine_remote"),
            Some("actor_service_machine_remote"),
            &[],
            Some("actor_human"),
            10,
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].status, MachineCommandStatus::Cancelled);
        assert_eq!(
            commands[0].error.as_ref().map(|error| error.code.as_str()),
            Some("not_delivered")
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
