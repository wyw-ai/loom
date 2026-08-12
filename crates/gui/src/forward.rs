//! Pipe WebSocket notifications from the shared `Client` to the front-end as
//! Tauri events. The front-end binds listeners in `ipc/bridge.ts`.
//!
//! Event names intentionally mirror the notification `method` when there's a
//! 1:1 mapping, and collapse `stream/update` into `loom://stream` (the
//! discriminator travels in the payload `kind`). Global updates such as
//! `channel.layout.updated` use this same path and do not require a scope.

use std::sync::{Arc, Weak};

use proto::methods::method;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::state::ConnectionSlot;
use crate::ws::Client;

#[derive(Serialize, Clone)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionEvent {
    Open {
        #[serde(rename = "connectionId")]
        connection_id: u64,
    },
    Closed {
        #[serde(rename = "connectionId")]
        connection_id: u64,
        reason: Option<String>,
    },
}

pub fn spawn(
    app: AppHandle,
    connection_slot: Arc<Mutex<ConnectionSlot>>,
    client: Arc<Client>,
    generation: u64,
) {
    tokio::spawn(async move {
        let client_ref = Arc::downgrade(&client);
        let Some(mut rx) = client.take_notifications().await else {
            tracing::warn!("notifications channel already taken — forward not started");
            return;
        };
        drop(client);

        while let Some(n) = rx.recv().await {
            let Some(disconnected) = client_ref.upgrade() else {
                break;
            };
            // Hold the slot only for the synchronous enqueue into Tauri. This
            // guarantees that a replacement generation is published after
            // every accepted old event, never before it.
            let slot = connection_slot.lock().await;
            let is_current = connection_matches(&slot, generation, &disconnected);
            if !is_current {
                break;
            }
            let emitted = emit_notification(&app, n);
            drop(slot);
            if let Err(e) = emitted {
                tracing::warn!(error = %e, "emit failed");
            }
        }

        let reason = match client_ref.upgrade() {
            Some(client) => client.disconnect_reason().await,
            None => None,
        };
        if clear_if_current(&connection_slot, &client_ref, generation).await {
            let _ = app.emit(
                "loom://connection",
                ConnectionEvent::Closed {
                    connection_id: generation,
                    reason: Some(reason.unwrap_or_else(|| "connection lost".into())),
                },
            );
        } else {
            tracing::debug!("stale notification stream ended after client replacement");
        }
    });
}

fn emit_notification(app: &AppHandle, n: proto::Notification) -> tauri::Result<()> {
    match n.method.as_str() {
        method::STREAM_UPDATE => {
            app.emit("loom://stream", n.params.unwrap_or(serde_json::Value::Null))
        }
        other => {
            // Unknown notifications are surfaced under a catch-all so we can
            // see them in devtools without silently dropping.
            let payload = serde_json::json!({
                "method": other,
                "params": n.params,
            });
            app.emit("loom://unknown", payload)
        }
    }
}

async fn clear_if_current(
    connection_slot: &Arc<Mutex<ConnectionSlot>>,
    client_ref: &Weak<Client>,
    generation: u64,
) -> bool {
    let Some(disconnected) = client_ref.upgrade() else {
        return false;
    };
    let mut slot = connection_slot.lock().await;
    if connection_matches(&slot, generation, &disconnected) {
        slot.current = None;
        true
    } else {
        false
    }
}

fn connection_matches(slot: &ConnectionSlot, generation: u64, client: &Arc<Client>) -> bool {
    slot.current.as_ref().is_some_and(|current| {
        current.generation == generation && Arc::ptr_eq(&current.client, client)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CurrentConnection;

    #[test]
    fn stale_generation_or_replaced_client_cannot_forward_notifications() {
        let current_client = Client::test_client();
        let replacement_client = Client::test_client();
        let slot = ConnectionSlot {
            generation: 7,
            current: Some(CurrentConnection {
                generation: 7,
                client: Arc::clone(&current_client),
            }),
        };

        assert!(connection_matches(&slot, 7, &current_client));
        assert!(!connection_matches(&slot, 6, &current_client));
        assert!(!connection_matches(&slot, 7, &replacement_client));
    }
}
