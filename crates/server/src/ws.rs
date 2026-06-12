use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use proto::methods::method;
use proto::types::{ActorKind, ChannelVisibility, ScopeKind, ScopeRef};
use proto::{ErrorCode, ErrorObject, RpcEnvelope};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::handlers;
use crate::state::AppState;
use crate::store::{Store, StoreEvent};
use crate::subscribe::Connection;

pub async fn ws_upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

pub fn spawn_file_rpc(state: AppState, root: PathBuf) -> std::io::Result<JoinHandle<()>> {
    std::fs::create_dir_all(root.join("clients"))?;
    Ok(tokio::spawn(async move {
        let mut connections: HashMap<String, FileRpcConnection> = HashMap::new();
        loop {
            if let Err(err) = poll_file_rpc(&state, &root, &mut connections).await {
                tracing::warn!(error = %err, "file-rpc poll failed");
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }))
}

struct FileRpcConnection {
    tx: mpsc::UnboundedSender<String>,
    seen: BTreeSet<String>,
}

async fn poll_file_rpc(
    state: &AppState,
    root: &Path,
    connections: &mut HashMap<String, FileRpcConnection>,
) -> std::io::Result<()> {
    let clients_dir = root.join("clients");
    for entry in std::fs::read_dir(clients_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(connection_id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !connections.contains_key(&connection_id) {
            let client_dir = entry.path();
            let out_dir = client_dir.join("out");
            std::fs::create_dir_all(client_dir.join("in"))?;
            std::fs::create_dir_all(&out_dir)?;
            let (tx, rx) = mpsc::unbounded_channel::<String>();
            state.subscriptions.add_connection(Connection {
                id: connection_id.clone(),
                actor_id: None,
                tx: tx.clone(),
            });
            tokio::spawn(file_rpc_writer(out_dir, rx));
            connections.insert(
                connection_id.clone(),
                FileRpcConnection {
                    tx,
                    seen: BTreeSet::new(),
                },
            );
        }
        if let Some(connection) = connections.get_mut(&connection_id) {
            let in_dir = entry.path().join("in");
            let mut files = std::fs::read_dir(in_dir)?
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("json"))
                .collect::<Vec<_>>();
            files.sort();
            for file in files {
                let Some(name) = file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                else {
                    continue;
                };
                if !connection.seen.insert(name) {
                    continue;
                }
                match std::fs::read_to_string(&file) {
                    Ok(text) => {
                        handle_text_frame(state, &connection_id, &connection.tx, text).await;
                    }
                    Err(err) => {
                        tracing::warn!(file = %file.display(), error = %err, "read file-rpc request failed")
                    }
                }
            }
        }
    }
    Ok(())
}

async fn file_rpc_writer(out_dir: PathBuf, mut rx: mpsc::UnboundedReceiver<String>) {
    let mut seq = 0_u64;
    while let Some(frame) = rx.recv().await {
        seq = seq.saturating_add(1);
        if let Err(err) = write_frame_file(&out_dir, seq, &frame) {
            tracing::warn!(dir = %out_dir.display(), error = %err, "write file-rpc response failed");
            break;
        }
    }
}

fn write_frame_file(dir: &Path, seq: u64, frame: &str) -> std::io::Result<()> {
    let final_path = dir.join(format!("{seq:020}.json"));
    let tmp_path = dir.join(format!("{seq:020}.json.tmp"));
    std::fs::write(&tmp_path, frame)?;
    std::fs::rename(tmp_path, final_path)
}

fn new_connection_id() -> String {
    let suffix = Uuid::new_v4().simple().to_string();
    format!("conn_{}", &suffix[..12])
}

async fn handle_socket(state: AppState, socket: WebSocket) {
    let connection_id = new_connection_id();
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    state.subscriptions.add_connection(Connection {
        id: connection_id.clone(),
        actor_id: None,
        tx: tx.clone(),
    });

    // Outbound writer.
    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if sink.send(Message::Text(frame)).await.is_err() {
                break;
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    });

    while let Some(msg) = stream.next().await {
        let Ok(msg) = msg else {
            break;
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => match String::from_utf8(b) {
                Ok(s) => s,
                Err(_) => continue,
            },
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(_) => break,
        };
        handle_text_frame(&state, &connection_id, &tx, text).await;
    }

    cleanup_connection(state.subscriptions.as_ref(), &connection_id, tx, writer).await;
}

#[cfg(unix)]
pub async fn handle_unix_socket(state: AppState, socket: tokio::net::UnixStream) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let connection_id = new_connection_id();
    let (reader, mut writer) = socket.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    state.subscriptions.add_connection(Connection {
        id: connection_id.clone(),
        actor_id: None,
        tx: tx.clone(),
    });

    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if writer.write_all(frame.as_bytes()).await.is_err() {
                break;
            }
            if writer.write_all(b"\n").await.is_err() {
                break;
            }
        }
        let _ = writer.shutdown().await;
    });

    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(text)) => handle_text_frame(&state, &connection_id, &tx, text).await,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(error = %err, "unix rpc socket read failed");
                break;
            }
        }
    }

    cleanup_connection(state.subscriptions.as_ref(), &connection_id, tx, writer).await;
}

async fn cleanup_connection(
    subscriptions: &crate::subscribe::Subscriptions,
    connection_id: &str,
    tx: mpsc::UnboundedSender<String>,
    writer: JoinHandle<()>,
) {
    subscriptions.remove_connection(connection_id);
    drop(tx);
    let _ = writer.await;
}

async fn handle_text_frame(
    state: &AppState,
    connection_id: &str,
    tx: &mpsc::UnboundedSender<String>,
    text: String,
) {
    let envelope = match serde_json::from_str::<RpcEnvelope>(&text) {
        Ok(e) => e,
        Err(e) => {
            let err = proto::Response::err(
                Value::Null,
                ErrorObject::new(ErrorCode::PARSE_ERROR, e.to_string()),
            );
            let _ = tx.send(serde_json::to_string(&err).unwrap_or_default());
            return;
        }
    };
    match envelope {
        RpcEnvelope::Request(req) => {
            let result = handlers::dispatch(state, connection_id, &req.method, req.params).await;
            let response = match result {
                Ok(value) => proto::Response::ok(req.id, value),
                Err(err) => proto::Response::err(req.id, err),
            };
            let _ = tx.send(serde_json::to_string(&response).unwrap_or_default());
        }
        RpcEnvelope::Notification(_) => {
            // v0 has no inbound notifications; ignore silently.
        }
        RpcEnvelope::Response(_) => {
            // Server doesn't currently make outbound requests, so ignore.
        }
    }
}

/// Spawns the broadcaster that turns `StoreEvent`s into `stream/update`
/// notifications and pushes them to subscribed connections.
pub fn spawn_stream_broadcaster(state: AppState) {
    let mut rx = state.store.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => fanout(&state, ev),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "stream broadcaster lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn fanout(state: &AppState, ev: StoreEvent) {
    use proto::methods::stream_kind as sk;

    // Channel ACL invite/revoke events: actor-inbox only — never scope fan
    // out (the recipient may not yet be subscribed to anything in this
    // channel). The payload mirrors a stream/update so existing client-side
    // notification handling (`handle_notification`) just works.
    if let StoreEvent::ChannelGranted { channel, actor_id } = &ev {
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: channel.id.clone(),
        };
        let payload = json!({
            "kind": sk::CHANNEL_INVITED,
            "scope": scope,
            "data": {
                "channelId": channel.id,
                "actorId": actor_id,
                "channel": channel,
            },
        });
        let delivered = send_actor_inbox(state, actor_id, method::STREAM_UPDATE, payload);
        tracing::debug!(
            channel = %channel.id,
            actor = %actor_id,
            delivered,
            "channel.invited actor-inbox push",
        );
        return;
    }
    if let StoreEvent::ChannelRevoked {
        channel_id,
        actor_id,
    } = &ev
    {
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: channel_id.clone(),
        };
        let payload = json!({
            "kind": sk::CHANNEL_REVOKED,
            "scope": scope,
            "data": {
                "channelId": channel_id,
                "actorId": actor_id,
            },
        });
        let delivered = send_actor_inbox(state, actor_id, method::STREAM_UPDATE, payload);
        tracing::debug!(
            channel = %channel_id,
            actor = %actor_id,
            delivered,
            "channel.revoked actor-inbox push",
        );
        return;
    }

    // ChannelCreated: public channels → broadcast to all connections;
    // private channels → only to the creator (sole initial member).
    if let StoreEvent::ChannelCreated(channel) = &ev {
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: channel.id.clone(),
        };
        let payload = json!({
            "kind": sk::CHANNEL_CREATED,
            "scope": scope,
            "data": { "channel": channel },
        });
        match channel.visibility {
            ChannelVisibility::Public => {
                state
                    .subscriptions
                    .broadcast_to_all(method::STREAM_UPDATE, payload);
                tracing::debug!(
                    channel = %channel.id,
                    "channel.created broadcast to all",
                );
            }
            ChannelVisibility::Private => {
                if let Some(creator) = channel.members.first() {
                    let delivered =
                        send_actor_inbox(state, creator, method::STREAM_UPDATE, payload);
                    tracing::debug!(
                        channel = %channel.id,
                        creator = %creator,
                        delivered,
                        "channel.created actor-inbox push (private)",
                    );
                }
            }
        }
        return;
    }

    if let StoreEvent::ChannelDeleted {
        channel_id,
        visibility,
        members,
    } = &ev
    {
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: channel_id.clone(),
        };
        let payload = json!({
            "kind": sk::CHANNEL_DELETED,
            "scope": scope,
            "data": { "channelId": channel_id },
        });
        match visibility {
            ChannelVisibility::Public => {
                state
                    .subscriptions
                    .broadcast_to_all(method::STREAM_UPDATE, payload);
                tracing::debug!(
                    channel = %channel_id,
                    "channel.deleted broadcast to all",
                );
            }
            ChannelVisibility::Private => {
                for actor_id in members {
                    let delivered =
                        send_actor_inbox(state, actor_id, method::STREAM_UPDATE, payload.clone());
                    tracing::debug!(
                        channel = %channel_id,
                        actor = %actor_id,
                        delivered,
                        "channel.deleted actor-inbox push (private)",
                    );
                }
            }
        }
        return;
    }

    // Internal events are not scope-broadcast as chat content, but directed
    // events still need the actor-inbox wake path. This is what lets
    // reminder.fire and service callback events wake an agent that is not
    // subscribed to the scope at the moment the event is appended.
    if let StoreEvent::EventCreated(event) = &ev {
        let scope = event.scope.clone();
        let payload = json!({
            "kind": sk::EVENT_CREATED,
            "scope": scope,
            "data": { "event": event },
        });
        let mut already_sent: std::collections::HashSet<String> = std::collections::HashSet::new();
        for target_id in state.store.delivery_recipients_for_source(&event.id) {
            if !already_sent.insert(target_id.clone()) {
                continue;
            }
            if !state.store.is_channel_member(
                &channel_id_for_scope(state, &event.scope).unwrap_or_default(),
                &target_id,
            ) && !is_public_scope(state, &event.scope)
            {
                tracing::debug!(
                    event = %event.id,
                    target = %target_id,
                    scope = ?event.scope,
                    "event actor-inbox push skipped: target not a member of private channel",
                );
                continue;
            }
            let mut inbox_payload = payload.clone();
            inbox_payload["delivery"] = json!({
                "sourceId": event.id.clone(),
                "actorId": target_id.clone(),
                "source": "actor_inbox",
            });
            let delivered =
                send_actor_inbox(state, &target_id, method::STREAM_UPDATE, inbox_payload);
            tracing::debug!(
                event = %event.id,
                from = %event.actor_id,
                target = %target_id,
                delivered,
                "event actor-inbox fanout",
            );
        }
        return;
    }

    let scope = ev.scope();
    let (kind, data) = match &ev {
        StoreEvent::MessageCreated(m) => (sk::MESSAGE_CREATED, json!({ "message": m })),
        StoreEvent::MessageUpdated(m) => (sk::MESSAGE_UPDATED, json!({ "message": m })),
        StoreEvent::EventCreated(_) => return,
        StoreEvent::RunUpdated(r) => (sk::RUN_UPDATED, json!({ "run": r })),
        StoreEvent::ThreadCreated(t) => (sk::THREAD_CREATED, json!({ "thread": t })),
        StoreEvent::ThreadUpdated(t) => (sk::THREAD_UPDATED, json!({ "thread": t })),
        StoreEvent::TaskChanged(t) => (sk::TASK_CHANGED, json!({ "task": t })),
        StoreEvent::ChannelUpdated(c) => (sk::CHANNEL_UPDATED, json!({ "channel": c })),
        StoreEvent::TaskAssignmentChanged { assignment, task } => (
            sk::TASK_ASSIGNMENT_CHANGED,
            json!({ "assignment": assignment, "task": task }),
        ),
        StoreEvent::ArtifactPublished(a) => (sk::ARTIFACT_PUBLISHED, json!({ "artifact": a })),
        StoreEvent::DeliveryUpdated(d) => (sk::DELIVERY_UPDATED, json!({ "delivery": d })),
        StoreEvent::MachineCommandUpdated(command) => {
            let _ = command.command_id.as_str();
            return;
        }
        StoreEvent::ChannelGranted { .. }
        | StoreEvent::ChannelRevoked { .. }
        | StoreEvent::ChannelCreated(_)
        | StoreEvent::ChannelDeleted { .. } => {
            unreachable!("channel grant/revoke/create/delete handled above")
        }
    };
    let Some(scope) = scope else {
        return;
    };
    let payload = json!({ "kind": kind, "scope": scope, "data": data });
    let private_allowed = match &ev {
        StoreEvent::MessageCreated(message) | StoreEvent::MessageUpdated(message) => {
            Store::message_private_actor_ids(message)
        }
        _ => None,
    };
    if let Some(allowed) = private_allowed.as_ref() {
        broadcast_filtered(state, &scope, method::STREAM_UPDATE, &payload, allowed);
    } else {
        broadcast_stream_update(state, &scope, &payload);
    }
    if let Some(thread_scope) = thread_scope_for_event(&ev) {
        let thread_payload =
            json!({ "kind": kind, "scope": thread_scope.clone(), "data": data.clone() });
        if let Some(allowed) = private_allowed.as_ref() {
            broadcast_filtered(
                state,
                &thread_scope,
                method::STREAM_UPDATE,
                &thread_payload,
                allowed,
            );
        } else {
            broadcast_stream_update(state, &thread_scope, &thread_payload);
        }
    }

    // Actor-inbox delivery: new messages with delivery rows are pushed
    // directly to recipient actor connections. Agents do not subscribe to
    // every channel; their wake path is the durable delivery queue plus this
    // immediate fanout.
    if let StoreEvent::MessageCreated(message) = &ev {
        let mut already_sent: std::collections::HashSet<String> = std::collections::HashSet::new();
        already_sent.insert(message.author_actor_id.clone());
        for target_id in state.store.delivery_recipients_for_source(&message.id) {
            if !already_sent.insert(target_id.clone()) {
                continue;
            }
            if !Store::message_visible_to_actor(message, &target_id) {
                continue;
            }
            if !state.store.is_channel_member(
                &channel_id_for_scope(state, &message.scope).unwrap_or_default(),
                &target_id,
            ) && !is_public_scope(state, &message.scope)
            {
                tracing::debug!(
                    message = %message.id,
                    target = %target_id,
                    scope = ?message.scope,
                    "message actor-inbox push skipped: target not a member of private channel",
                );
                continue;
            }
            let mut inbox_payload = payload.clone();
            inbox_payload["delivery"] = json!({
                "sourceId": message.id.clone(),
                "actorId": target_id.clone(),
                "source": "actor_inbox",
            });
            let delivered =
                send_actor_inbox(state, &target_id, method::STREAM_UPDATE, inbox_payload);
            tracing::debug!(
                message = %message.id,
                from = %message.author_actor_id,
                target = %target_id,
                delivered,
                "message actor-inbox fanout",
            );
        }
    }
}

fn broadcast_stream_update(state: &AppState, scope: &ScopeRef, payload: &Value) {
    // ACL gate the scope broadcast: in private channels, drop frames for
    // any subscriber whose connection isn't bound to a member actor. We
    // resolve membership once per fanout (not per subscriber) by computing
    // the channel-owning members set up front.
    if let Some(members_filter) = scope_acl_filter(state, scope) {
        broadcast_filtered(
            state,
            scope,
            method::STREAM_UPDATE,
            payload,
            &members_filter,
        );
    } else {
        state
            .subscriptions
            .broadcast_to_scope(scope, method::STREAM_UPDATE, payload.clone());
    }
}

fn thread_scope_for_event(ev: &StoreEvent) -> Option<ScopeRef> {
    let id = match ev {
        StoreEvent::ThreadUpdated(thread) => &thread.id,
        StoreEvent::TaskChanged(task) => &task.canonical_thread_id,
        StoreEvent::TaskAssignmentChanged { task, .. } => &task.canonical_thread_id,
        _ => return None,
    };
    Some(ScopeRef {
        kind: ScopeKind::Thread,
        id: id.clone(),
    })
}

fn send_actor_inbox(state: &AppState, actor_id: &str, method: &str, payload: Value) -> usize {
    match state.store.get_actor(actor_id).map(|a| a.kind) {
        Some(ActorKind::Human) => state
            .subscriptions
            .send_to_actor_connections(actor_id, method, payload),
        _ => usize::from(state.subscriptions.send_to_actor(actor_id, method, payload)),
    }
}

/// Returns `Some(members_set)` when the scope belongs to a private channel
/// (callers must filter broadcasts to only those members). Returns `None`
/// for public channels and unknown scopes (broadcast unconditionally).
fn scope_acl_filter(
    state: &AppState,
    scope: &ScopeRef,
) -> Option<std::collections::HashSet<String>> {
    let channel_id = channel_id_for_scope(state, scope)?;
    let ch = state.store.get_channel(&channel_id)?;
    match ch.visibility {
        ChannelVisibility::Public => None,
        ChannelVisibility::Private => Some(ch.members.into_iter().collect()),
    }
}

pub(crate) fn channel_id_for_scope(state: &AppState, scope: &ScopeRef) -> Option<String> {
    match scope.kind {
        ScopeKind::Channel => Some(scope.id.clone()),
        ScopeKind::Thread => state.store.get_thread(&scope.id).map(|t| t.channel_id),
    }
}

fn is_public_scope(state: &AppState, scope: &ScopeRef) -> bool {
    let Some(channel_id) = channel_id_for_scope(state, scope) else {
        return false;
    };
    state
        .store
        .get_channel(&channel_id)
        .map(|c| matches!(c.visibility, ChannelVisibility::Public))
        .unwrap_or(false)
}

/// Member-aware variant of `Subscriptions::broadcast_to_scope`: only sends
/// the frame to subscribers whose connection's bound actor is in `allowed`.
/// Used for private channel scope fan-out so non-members holding an
/// (impossible-to-acquire-but-defensive) subscription don't see updates.
fn broadcast_filtered(
    state: &AppState,
    scope: &ScopeRef,
    method: &str,
    payload: &Value,
    allowed: &std::collections::HashSet<String>,
) {
    let frame =
        match serde_json::to_string(&proto::Notification::new(method, Some(payload.clone()))) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, "failed to serialize filtered stream/update");
                return;
            }
        };
    for conn_id in state.subscriptions.scope_subscribers(scope) {
        let Some(actor) = state.subscriptions.actor_for_connection(&conn_id) else {
            continue;
        };
        if !allowed.contains(&actor) {
            continue;
        }
        if state.subscriptions.is_suppressed_wake_target(&conn_id) {
            continue;
        }
        state
            .subscriptions
            .send_to_connection(&conn_id, frame.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use proto::types::{
        Actor, AudienceKind, AudienceRef, DeliveryPolicy, MessageIntent, MessageKind, Meta,
    };
    use tokio::sync::oneshot;

    use crate::artifacts::ArtifactStore;
    use crate::journal::Journal;
    use crate::machine_commands::MachineCommandWaiters;
    use crate::scope_skills::ScopeSkills;
    use crate::store::Store;
    use crate::subscribe::Subscriptions;

    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-ws-tests-{name}-{}",
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

    #[tokio::test]
    async fn cleanup_connection_releases_writer_after_registry_removal() {
        let subscriptions = crate::subscribe::Subscriptions::new();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        subscriptions.add_connection(Connection {
            id: "conn_test".into(),
            actor_id: None,
            tx: tx.clone(),
        });

        let (closed_tx, closed_rx) = oneshot::channel();
        let writer = tokio::spawn(async move {
            while rx.recv().await.is_some() {}
            let _ = closed_tx.send(());
        });

        cleanup_connection(subscriptions.as_ref(), "conn_test", tx, writer).await;

        tokio::time::timeout(Duration::from_secs(1), closed_rx)
            .await
            .expect("writer should exit once the last sender is dropped")
            .expect("writer close signal should be delivered");
        assert!(!subscriptions.send_to_connection("conn_test", "frame".into()));
    }

    #[test]
    fn fanout_delivers_message_delivery_through_actor_inbox() {
        let state = fresh_state("message-inbox");
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        state.subscriptions.add_connection(Connection {
            id: "conn_agent".into(),
            actor_id: Some("actor_agent".into()),
            tx,
        });
        state
            .store
            .upsert_actor(Actor {
                id: "actor_agent".into(),
                display_name: "Agent".into(),
                kind: ActorKind::Agent,
                capabilities: None,
                _meta: None,
            })
            .expect("agent actor");
        let channel = state
            .store
            .create_channel("c".into(), None)
            .expect("channel");
        state
            .store
            .grant_channel(&channel.id, "actor_agent")
            .expect("grant");
        let message = state
            .store
            .append_message(
                "actor_human".into(),
                format!("#{}", channel.id),
                MessageKind::Human,
                "@actor_agent please handle this".into(),
                Vec::new(),
                vec![AudienceRef {
                    kind: AudienceKind::Actor,
                    id: "actor_agent".into(),
                    display: None,
                }],
                MessageIntent::RequestAction,
                DeliveryPolicy::WakeAgent,
                None,
                None,
                Vec::new(),
                Meta::default(),
                None,
            )
            .expect("message");

        fanout(&state, StoreEvent::MessageCreated(message.clone()));

        let frame = rx.try_recv().expect("message actor-inbox frame");
        let value: Value = serde_json::from_str(&frame).expect("json notification");
        assert_eq!(value["method"], method::STREAM_UPDATE);
        assert_eq!(
            value["params"]["kind"],
            proto::methods::stream_kind::MESSAGE_CREATED
        );
        assert_eq!(value["params"]["data"]["message"]["id"], message.id);
        assert_eq!(value["params"]["delivery"]["actorId"], "actor_agent");
        assert_eq!(value["params"]["delivery"]["sourceId"], message.id);
    }

    #[test]
    fn fanout_scope_private_message_only_reaches_allowed_subscribers() {
        let state = fresh_state("scope-private-message");
        for (id, kind, name) in [
            ("actor_alice", ActorKind::Human, "Alice"),
            ("actor_bob", ActorKind::Human, "Bob"),
            ("actor_carol", ActorKind::Human, "Carol"),
        ] {
            state
                .store
                .upsert_actor(Actor {
                    id: id.into(),
                    display_name: name.into(),
                    kind,
                    capabilities: None,
                    _meta: None,
                })
                .expect("actor");
        }
        let channel = state
            .store
            .create_channel("private delivery".into(), None)
            .expect("channel");
        state
            .store
            .grant_channel(&channel.id, "actor_alice")
            .expect("grant alice");
        state
            .store
            .grant_channel(&channel.id, "actor_bob")
            .expect("grant bob");
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: channel.id.clone(),
        };
        let (tx_bob, mut rx_bob) = mpsc::unbounded_channel::<String>();
        let (tx_carol, mut rx_carol) = mpsc::unbounded_channel::<String>();
        state.subscriptions.add_connection(Connection {
            id: "conn_bob".into(),
            actor_id: Some("actor_bob".into()),
            tx: tx_bob,
        });
        state.subscriptions.add_connection(Connection {
            id: "conn_carol".into(),
            actor_id: Some("actor_carol".into()),
            tx: tx_carol,
        });
        assert!(state.subscriptions.subscribe("conn_bob", scope.clone()));
        assert!(state.subscriptions.subscribe("conn_carol", scope.clone()));

        let mut metadata = Meta::default();
        metadata.insert("private".into(), json!(true));
        metadata.insert("privateTo".into(), json!(["actor_bob"]));
        let message = state
            .store
            .append_message(
                "actor_alice".into(),
                format!("#{}", channel.id),
                MessageKind::Human,
                "secret".into(),
                Vec::new(),
                vec![AudienceRef {
                    kind: AudienceKind::Actor,
                    id: "actor_bob".into(),
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
            .expect("message");

        fanout(&state, StoreEvent::MessageCreated(message));

        let bob_frame = rx_bob.try_recv().expect("bob receives private frame");
        let value: Value = serde_json::from_str(&bob_frame).expect("json notification");
        assert_eq!(
            value["params"]["kind"],
            proto::methods::stream_kind::MESSAGE_CREATED
        );
        assert!(
            rx_carol.try_recv().is_err(),
            "non-recipient subscriber must not receive private message frames"
        );
    }

    #[test]
    fn fanout_delivers_task_changes_to_canonical_thread_subscribers() {
        let state = fresh_state("task-thread");
        let channel = state
            .store
            .create_channel("tasks".into(), None)
            .expect("channel");
        state
            .store
            .grant_channel(&channel.id, "actor_human")
            .expect("grant human");
        state
            .store
            .grant_channel(&channel.id, "actor_agent")
            .expect("grant agent");
        let source = state
            .store
            .append_message(
                "actor_human".into(),
                format!("#{}", channel.id),
                MessageKind::Human,
                "please do this".into(),
                Vec::new(),
                Vec::new(),
                MessageIntent::RequestAction,
                DeliveryPolicy::NotifyOnly,
                None,
                None,
                Vec::new(),
                Meta::default(),
                None,
            )
            .expect("source message");
        let task = state
            .store
            .create_task(
                source.id,
                Some("task".into()),
                String::new(),
                "actor_human".into(),
                Some("actor_agent".into()),
                None,
                None,
                None,
                None,
            )
            .expect("task");
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        state.subscriptions.add_connection(Connection {
            id: "conn_thread".into(),
            actor_id: Some("actor_human".into()),
            tx,
        });
        let thread_scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: task.canonical_thread_id.clone(),
        };
        assert!(state
            .subscriptions
            .subscribe("conn_thread", thread_scope.clone()));

        fanout(&state, StoreEvent::TaskChanged(task.clone()));

        let frame = rx.try_recv().expect("task thread frame");
        let value: Value = serde_json::from_str(&frame).expect("json notification");
        assert_eq!(value["method"], method::STREAM_UPDATE);
        assert_eq!(
            value["params"]["kind"],
            proto::methods::stream_kind::TASK_CHANGED
        );
        assert_eq!(value["params"]["scope"]["kind"], "thread");
        assert_eq!(value["params"]["scope"]["id"], task.canonical_thread_id);
        assert_eq!(value["params"]["data"]["task"]["id"], task.id);
    }
}
