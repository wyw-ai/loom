use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use proto::methods::method;
use proto::{ErrorCode, ErrorObject, RpcEnvelope};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::handlers;
use crate::state::AppState;
use crate::store::StoreEvent;
use crate::subscribe::Connection;

pub async fn ws_upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: AppState, socket: WebSocket) {
    let connection_id = format!(
        "conn_{}",
        Uuid::new_v4().simple().to_string()[..12].to_string()
    );
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

    // Inbound reader.
    let reader_state = state.clone();
    let reader_conn = connection_id.clone();
    let reader_tx = tx.clone();
    let reader = tokio::spawn(async move {
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
            handle_text_frame(&reader_state, &reader_conn, &reader_tx, text).await;
        }
    });

    let _ = tokio::join!(writer, reader);
    state.subscriptions.remove_connection(&connection_id);
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

    let scope = ev.scope();
    let (kind, data) = match &ev {
        StoreEvent::EventCreated(e) => (sk::EVENT_CREATED, json!({ "event": e })),
        StoreEvent::TurnOpened(t) => (sk::TURN_OPENED, json!({ "turn": t })),
        StoreEvent::TurnClosed(t) => (sk::TURN_CLOSED, json!({ "turn": t })),
        StoreEvent::ThreadCreated(t) => (sk::THREAD_CREATED, json!({ "thread": t })),
        StoreEvent::ArtifactPublished(a) => (sk::ARTIFACT_PUBLISHED, json!({ "artifact": a })),
        StoreEvent::ReceiptRecorded(r) => (sk::RECEIPT_RECORDED, json!({ "receipt": r })),
        StoreEvent::DeliveryUpdated(d) => (sk::DELIVERY_UPDATED, json!({ "delivery": d })),
        StoreEvent::TraceAppended(_) => unreachable!("trace handled above"),
    };
    let Some(scope) = scope else {
        return;
    };
    let payload = json!({ "kind": kind, "scope": scope, "data": data });
    state
        .subscriptions
        .broadcast_to_scope(&scope, method::STREAM_UPDATE, payload.clone());

    // Actor-inbox delivery: when an EventCreated event hands off to an actor,
    // also push the same stream/update directly to that actor's connection
    // (if any). This lets an external `joi agent serve` process learn about
    // its work without having to subscribe to every channel/thread it might
    // care about. For non-event store events (turn open/close, threads, ...)
    // there's no hands_off_to to follow, so they only ride the scope fan-out.
    if let StoreEvent::EventCreated(e) = &ev {
        use proto::types::{RefKind, RelationKind};
        let mut already_sent: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        // Don't double-send to an actor whose own connection is also a scope
        // subscriber — that's a minor optimization but more importantly avoids
        // self-loops when the agent emits its own events on the same scope.
        already_sent.insert(e.actor_id.as_str());
        for r in &e.relations {
            if !matches!(r.kind, RelationKind::HandsOffTo) {
                continue;
            }
            if r.target.kind != RefKind::Actor {
                continue;
            }
            if !already_sent.insert(r.target.id.as_str()) {
                continue;
            }
            state
                .subscriptions
                .send_to_actor(&r.target.id, method::STREAM_UPDATE, payload.clone());
        }
    }
}
