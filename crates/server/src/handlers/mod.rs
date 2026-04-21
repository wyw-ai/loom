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
        method::SCOPE_READ => scope_read(state, params),
        method::CHANNEL_CREATE => channel_create(state, params),
        method::CHANNEL_LIST => channel_list(state),
        method::THREAD_CREATE => thread_create(state, params),
        method::THREAD_LIST => thread_list(state, params),
        method::TURN_OPEN => turn_open(state, params),
        method::TURN_CLOSE => turn_close(state, params),
        method::TURN_TRACE_READ => turn_trace_read(state, connection_id, params),
        method::EVENT_APPEND => event_append(state, params).await,
        method::ARTIFACT_PUBLISH => artifact_publish(state, params),
        method::ARTIFACT_GET => artifact_get(state, params),
        method::ARTIFACT_READ => artifact_read(state, params),
        method::RECEIPT_RECORD => receipt_record(state, params),
        method::ACTOR_LIST => actor_list(state),
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
    let actor = match state.store.get_actor(&actor_id) {
        Some(existing) => existing,
        None => {
            let new_actor = Actor {
                id: actor_id.clone(),
                kind: p.actor_kind.unwrap_or(ActorKind::Human),
                display_name: p.display_name.unwrap_or_else(|| actor_id.clone()),
                capabilities: None,
                _meta: None,
            };
            state.store.upsert_actor(new_actor).map_err(map_store_err)?
        }
    };
    state
        .subscriptions
        .bind_actor(connection_id, actor_id.clone());
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
    if !state
        .subscriptions
        .subscribe(connection_id, p.scope.clone())
    {
        return Err(ErrorObject::new(
            ErrorCode::APP_INVALID_STATE,
            "connection not registered",
        ));
    }
    let actor_id = state
        .subscriptions
        .actor_for_connection(connection_id)
        .unwrap_or_default();
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

fn scope_read(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ScopeReadParams = parse_params(params)?;
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

// ---- channel ----

fn channel_create(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: ChannelCreateParams = parse_params(params)?;
    let channel = state.store.create_channel(p.title).map_err(map_store_err)?;
    ok(ChannelCreateResult { channel })
}

fn channel_list(state: &AppState) -> HandlerResult {
    ok(ChannelListResult {
        channels: state.store.list_channels(),
    })
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
    let p: ThreadListParams =
        parse_params(params).unwrap_or(ThreadListParams { channel_id: None });
    let threads = state.store.list_threads(p.channel_id.as_deref());
    ok(ThreadListResult { threads })
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

fn turn_close(state: &AppState, params: Option<Value>) -> HandlerResult {
    let p: TurnCloseParams = parse_params(params)?;
    let turn = state
        .store
        .close_turn(&p.turn_id, p.status)
        .map_err(map_store_err)?;
    ok(TurnCloseResult { turn })
}

fn turn_trace_read(
    state: &AppState,
    connection_id: &str,
    params: Option<Value>,
) -> HandlerResult {
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
        .ok_or_else(|| {
            ErrorObject::new(ErrorCode::APP_INVALID_STATE, "connection has no actor")
        })?;
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
        },
        autostart: false,
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

// ---- stream/update fanout (called from supervisor) ----

pub fn stream_update_payload(kind: &str, scope: ScopeRef, data: Value) -> Value {
    json!(StreamUpdate {
        kind: kind.into(),
        scope,
        data,
    })
}

#[allow(dead_code)]
pub fn _ensure_arc<T>(x: Arc<T>) -> Arc<T> {
    x
}
