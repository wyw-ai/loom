//! Thin WebSocket JSON-RPC client for the Loom server.
//!
//! Mirrors `crates/cli/src/client.rs` — the two will be merged into a shared
//! `loom-client` crate in a follow-up; for this scaffold we duplicate the file
//! verbatim so the GUI can land without disturbing the TUI's call sites.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use proto::methods::method;
use proto::{Notification, Request, Response, RpcEnvelope};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Response>>>>;

pub struct Client {
    out_tx: mpsc::Sender<String>,
    pending: Pending,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    pub notifications: Mutex<Option<mpsc::Receiver<Notification>>>,
}

impl Client {
    pub async fn connect(url: &str) -> Result<Arc<Self>> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .with_context(|| format!("ws connect {}", url))?;
        let (mut sink, mut stream) = ws.split();
        let (out_tx, mut out_rx) = mpsc::channel::<String>(4096);
        let (notif_tx, notif_rx) = mpsc::channel::<Notification>(1024);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if sink.send(Message::Text(frame)).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let pending_r = pending.clone();
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                let Ok(msg) = msg else { break };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Binary(b) => match String::from_utf8(b) {
                        Ok(s) => s,
                        Err(_) => continue,
                    },
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                    Message::Close(_) => break,
                };
                dispatch_frame(text, &pending_r, &notif_tx).await;
            }
        });

        Ok(Self::new(out_tx, pending, notif_rx))
    }

    fn new(
        out_tx: mpsc::Sender<String>,
        pending: Pending,
        notif_rx: mpsc::Receiver<Notification>,
    ) -> Arc<Self> {
        Arc::new(Self {
            out_tx,
            pending,
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            notifications: Mutex::new(Some(notif_rx)),
        })
    }

    pub async fn take_notifications(&self) -> Option<mpsc::Receiver<Notification>> {
        self.notifications.lock().await.take()
    }

    pub async fn call_raw(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id_value = json!(id);
        let key = id_to_key(&id_value);
        let req = Request::new(id_value, method, params);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(key, tx);
        let frame = serde_json::to_string(&req)?;
        self.out_tx
            .send(frame)
            .await
            .map_err(|_| anyhow!("ws writer closed"))?;
        let resp = tokio::time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow!("rpc `{}` timed out", method))?
            .map_err(|_| anyhow!("rpc `{}` channel closed", method))?;
        if let Some(err) = resp.error {
            return Err(anyhow!(
                "rpc `{}` failed: {} (code {})",
                method,
                err.message,
                err.code
            ));
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }

    pub async fn initialize(&self, client_name: &str, client_version: &str) -> Result<()> {
        let _: Value = self
            .call_raw(
                method::INITIALIZE,
                Some(json!({
                    "protocolVersion": proto::PROTOCOL_VERSION,
                    "clientInfo": {
                        "name": client_name,
                        "title": "Loom Desktop",
                        "version": client_version,
                    },
                })),
            )
            .await?;
        Ok(())
    }

    pub async fn open_connection(
        &self,
        actor_id: &str,
        display_name: Option<&str>,
    ) -> Result<Value> {
        self.call_raw(
            method::CONNECTION_OPEN,
            Some(json!({
                "actorId": actor_id,
                "actorKind": "human",
                "displayName": display_name.unwrap_or(actor_id),
            })),
        )
        .await
    }
}

async fn dispatch_frame(text: String, pending: &Pending, notif_tx: &mpsc::Sender<Notification>) {
    let envelope: RpcEnvelope = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(%e, frame = %text, "bad frame");
            return;
        }
    };
    match envelope {
        RpcEnvelope::Response(r) => {
            let key = id_to_key(&r.id);
            if let Some(tx) = pending.lock().await.remove(&key) {
                let _ = tx.send(r);
            }
        }
        RpcEnvelope::Notification(n) => {
            let _ = notif_tx.try_send(n);
        }
        RpcEnvelope::Request(_) => {
            // Server doesn't send requests in v0.
        }
    }
}

fn id_to_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => "_".into(),
    }
}
