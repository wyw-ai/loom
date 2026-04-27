//! Pipe WebSocket notifications from the shared `Client` to the front-end as
//! Tauri events. The front-end binds listeners in `ipc/bridge.ts`.
//!
//! Event names intentionally mirror the notification `method` when there's a
//! 1:1 mapping, and collapse `stream/update` into `joi://stream` (the
//! discriminator travels in the payload `kind`).

use std::sync::Arc;

use proto::methods::method;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::ws::Client;

#[derive(Serialize, Clone)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionEvent {
    Open,
    Closed { reason: Option<String> },
}

pub fn spawn(app: AppHandle, client: Arc<Client>) {
    tokio::spawn(async move {
        let Some(mut rx) = client.take_notifications().await else {
            tracing::warn!("notifications channel already taken — forward not started");
            return;
        };
        while let Some(n) = rx.recv().await {
            let emitted = match n.method.as_str() {
                method::STREAM_UPDATE => {
                    app.emit("joi://stream", n.params.unwrap_or(serde_json::Value::Null))
                }
                method::TURN_STREAM_UPDATE => app.emit(
                    "joi://stream-delta",
                    n.params.unwrap_or(serde_json::Value::Null),
                ),
                method::TURN_TRACE_UPDATE => app.emit(
                    "joi://trace",
                    n.params.unwrap_or(serde_json::Value::Null),
                ),
                other => {
                    // Unknown notifications are surfaced under a catch-all so
                    // we can see them in devtools without silently dropping.
                    let payload = serde_json::json!({
                        "method": other,
                        "params": n.params,
                    });
                    app.emit("joi://unknown", payload)
                }
            };
            if let Err(e) = emitted {
                tracing::warn!(error = %e, "emit failed");
            }
        }
        let _ = app.emit(
            "joi://connection",
            ConnectionEvent::Closed {
                reason: Some("notification stream ended".into()),
            },
        );
    });
}
