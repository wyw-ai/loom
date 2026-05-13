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
use crate::store::StoreEvent;
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

    // Trace frames are turn-owner-private; route them by owner actor and
    // never broadcast on a scope subscription.
    if let StoreEvent::TraceAppended(frame) = &ev {
        let Some(turn) = state.store.get_turn(&frame.turn_id) else {
            tracing::warn!(turn = %frame.turn_id, "trace frame for unknown turn; dropping");
            return;
        };
        let payload = json!({
            "turnId": frame.turn_id,
            "frame": frame,
        });
        state
            .subscriptions
            .send_to_actor(&turn.actor_id, method::TURN_TRACE_UPDATE, payload);
        return;
    }

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

    let scope = ev.scope();
    let (kind, data) = match &ev {
        StoreEvent::EventCreated(e) => (sk::EVENT_CREATED, json!({ "event": e })),
        StoreEvent::TurnOpened(t) => (sk::TURN_OPENED, json!({ "turn": t })),
        StoreEvent::TurnClosed(t) => (sk::TURN_CLOSED, json!({ "turn": t })),
        StoreEvent::ThreadCreated(t) => (sk::THREAD_CREATED, json!({ "thread": t })),
        StoreEvent::ThreadUpdated(t) => (sk::THREAD_UPDATED, json!({ "thread": t })),
        StoreEvent::TaskChanged(t) => (sk::TASK_CHANGED, json!({ "task": t })),
        StoreEvent::TaskAssignmentChanged { assignment, task } => (
            sk::TASK_ASSIGNMENT_CHANGED,
            json!({ "assignment": assignment, "task": task }),
        ),
        StoreEvent::ArtifactPublished(a) => (sk::ARTIFACT_PUBLISHED, json!({ "artifact": a })),
        StoreEvent::ReceiptRecorded(r) => (sk::RECEIPT_RECORDED, json!({ "receipt": r })),
        StoreEvent::DeliveryUpdated(d) => (sk::DELIVERY_UPDATED, json!({ "delivery": d })),
        StoreEvent::MachineCommandUpdated(command) => {
            let _ = command.command_id.as_str();
            return;
        }
        StoreEvent::TraceAppended(_) => unreachable!("trace handled above"),
        StoreEvent::ChannelGranted { .. }
        | StoreEvent::ChannelRevoked { .. }
        | StoreEvent::ChannelCreated(_) => {
            unreachable!("channel grant/revoke/created handled above")
        }
    };
    let Some(scope) = scope else {
        return;
    };
    let payload = json!({ "kind": kind, "scope": scope, "data": data });
    // ACL gate the scope broadcast: in private channels, drop frames for
    // any subscriber whose connection isn't bound to a member actor. We
    // resolve membership once per fanout (not per subscriber) by computing
    // the channel-owning members set up front.
    if let Some(members_filter) = scope_acl_filter(state, &scope) {
        broadcast_filtered(
            state,
            &scope,
            method::STREAM_UPDATE,
            &payload,
            &members_filter,
        );
    } else {
        state
            .subscriptions
            .broadcast_to_scope(&scope, method::STREAM_UPDATE, payload.clone());
    }

    // Actor-inbox delivery: when an EventCreated event hands off to an actor,
    // also push the same stream/update directly to that actor's connection
    // (if any). This lets a daemon-managed agent worker learn about
    // its work without having to subscribe to every channel/thread it might
    // care about. For non-event store events (turn open/close, threads, ...)
    // there's no hands_off_to to follow, so they only ride the scope fan-out.
    if let StoreEvent::EventCreated(e) = &ev {
        use proto::types::{RefKind, RelationKind};
        let mut already_sent: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Don't double-send ordinary self-authored events to an actor whose
        // own connection is also a scope subscriber. Explicit self-handoffs
        // are handled as forced deliveries below because they are how an agent
        // moves a triage turn into a newly-created task thread before it has
        // subscribed to that thread.
        already_sent.insert(e.actor_id.clone());

        // Resolve targets up-front: explicit HandsOffTo plus the implicit
        // RespondsTo reverse-target (the actor whose event is being replied
        // to). Reverse-delivery makes service plugins reachable without
        // subscribing to every scope they touch — see store.rs append_event.
        let mut targets: Vec<(String, &'static str, bool)> = Vec::new();
        for r in &e.relations {
            match r.kind {
                RelationKind::HandsOffTo if r.target.kind == RefKind::Actor => {
                    let force_self = r.target.id == e.actor_id;
                    targets.push((r.target.id.clone(), "hands_off_to", force_self));
                }
                RelationKind::RespondsTo if r.target.kind == RefKind::Event => {
                    if let Some(orig) = state.store.get_event(&r.target.id) {
                        // Usually self-responses are deliberately ignored by
                        // the actor-inbox path. Permission approvals are the
                        // exception: a GUI may be misconfigured with the same
                        // actor id as the agent runtime, but the response must
                        // still reach the long-lived daemon worker
                        // connection so it can unblock the ACP child.
                        let force_self = e.kind == "action.response"
                            && orig.kind == "action.request"
                            && orig.actor_id == e.actor_id;
                        targets.push((orig.actor_id, "responds_to", force_self));
                    }
                }
                _ => {}
            }
        }

        for (target_id, reason, force_self) in targets {
            let dedupe_key = if force_self && target_id == e.actor_id {
                format!("{target_id}:forced:{reason}")
            } else {
                target_id.clone()
            };
            if !already_sent.insert(dedupe_key) {
                continue;
            }
            // ACL gate the actor-inbox push: an outsider being mentioned
            // into a private channel must NOT receive the event — invite
            // them first with `channel/invite`. Public channels or
            // already-invited actors fall through.
            if !state.store.is_channel_member(
                &channel_id_for_scope(state, &e.scope).unwrap_or_default(),
                &target_id,
            ) && !is_public_scope(state, &e.scope)
            {
                tracing::debug!(
                    event = %e.id,
                    target = %target_id,
                    reason = reason,
                    scope = ?e.scope,
                    "actor-inbox push skipped: target not a member of private channel",
                );
                continue;
            }
            let delivered =
                send_actor_inbox(state, &target_id, method::STREAM_UPDATE, payload.clone());
            tracing::debug!(
                event = %e.id,
                kind = %e.kind,
                from = %e.actor_id,
                target = %target_id,
                reason = reason,
                delivered,
                "actor-inbox fanout",
            );
        }
    }
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
        state
            .subscriptions
            .send_to_connection(&conn_id, frame.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use proto::types::{Ref, RefKind, Relation, RelationKind};
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
            "joi-ws-tests-{name}-{}",
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
    fn fanout_delivers_explicit_self_handoff_through_actor_inbox() {
        let state = fresh_state("self-handoff-inbox");
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        state.subscriptions.add_connection(Connection {
            id: "conn_emma".into(),
            actor_id: Some("actor_agent_emma".into()),
            tx,
        });

        let channel = state
            .store
            .create_channel("story".into(), None)
            .expect("channel");
        let root = state
            .store
            .append_event(
                "content.add".into(),
                "actor_human".into(),
                ScopeRef {
                    kind: ScopeKind::Channel,
                    id: channel.id.clone(),
                },
                None,
                json!({ "text": "@Emma write a story" }),
                Vec::new(),
                None,
            )
            .expect("root event");
        let thread = state
            .store
            .create_thread(channel.id.clone(), "story task".into(), root.id)
            .expect("thread");
        let handoff = state
            .store
            .append_event(
                "content.add".into(),
                "actor_agent_emma".into(),
                ScopeRef {
                    kind: ScopeKind::Thread,
                    id: thread.id,
                },
                None,
                json!({ "text": "continue in task thread" }),
                vec![Relation {
                    kind: RelationKind::HandsOffTo,
                    target: Ref {
                        kind: RefKind::Actor,
                        id: "actor_agent_emma".into(),
                        _meta: None,
                    },
                    _meta: None,
                }],
                None,
            )
            .expect("self handoff");

        fanout(&state, StoreEvent::EventCreated(handoff.clone()));

        let frame = rx.try_recv().expect("self handoff actor-inbox frame");
        let value: Value = serde_json::from_str(&frame).expect("json notification");
        assert_eq!(value["method"], method::STREAM_UPDATE);
        assert_eq!(
            value["params"]["kind"],
            proto::methods::stream_kind::EVENT_CREATED
        );
        assert_eq!(value["params"]["data"]["event"]["id"], handoff.id);
    }
}
