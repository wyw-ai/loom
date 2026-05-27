//! Pipe WebSocket notifications from the shared `Client` to the front-end as
//! Tauri events. The front-end binds listeners in `ipc/bridge.ts`.
//!
//! Event names intentionally mirror the notification `method` when there's a
//! 1:1 mapping, and collapse `stream/update` into `loom://stream` (the
//! discriminator travels in the payload `kind`).

use std::sync::{Arc, Weak};

use proto::methods::method;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::ws::Client;

#[derive(Serialize, Clone)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionEvent {
    Open,
    Closed { reason: Option<String> },
}

pub fn spawn(app: AppHandle, current_client: Arc<Mutex<Option<Arc<Client>>>>, client: Arc<Client>) {
    tokio::spawn(async move {
        let client_ref = Arc::downgrade(&client);
        let Some(mut rx) = client.take_notifications().await else {
            tracing::warn!("notifications channel already taken — forward not started");
            return;
        };
        drop(client);

        while let Some(n) = rx.recv().await {
            let emitted = match n.method.as_str() {
                method::STREAM_UPDATE => {
                    app.emit("loom://stream", n.params.unwrap_or(serde_json::Value::Null))
                }
                other => {
                    // Unknown notifications are surfaced under a catch-all so
                    // we can see them in devtools without silently dropping.
                    let payload = serde_json::json!({
                        "method": other,
                        "params": n.params,
                    });
                    app.emit("loom://unknown", payload)
                }
            };
            if let Err(e) = emitted {
                tracing::warn!(error = %e, "emit failed");
            }
        }

        if clear_if_current(&current_client, &client_ref).await {
            let _ = app.emit(
                "loom://connection",
                ConnectionEvent::Closed {
                    reason: Some("connection lost".into()),
                },
            );
        } else {
            tracing::debug!("stale notification stream ended after client replacement");
        }
    });
}

async fn clear_if_current(
    current_client: &Arc<Mutex<Option<Arc<Client>>>>,
    client_ref: &Weak<Client>,
) -> bool {
    let Some(disconnected) = client_ref.upgrade() else {
        return false;
    };
    let mut current = current_client.lock().await;
    if current
        .as_ref()
        .is_some_and(|client| Arc::ptr_eq(client, &disconnected))
    {
        *current = None;
        true
    } else {
        false
    }
}
