use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use proto::methods::method;
use proto::{Notification, Request, Response, RpcEnvelope};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Response>>>>;

pub struct Client {
    out_tx: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    pub notifications: Mutex<mpsc::UnboundedReceiver<Notification>>,
}

impl Client {
    pub async fn connect(url: &str) -> Result<Arc<Self>> {
        Self::connect_ws(url).await
    }

    pub async fn connect_ws(url: &str) -> Result<Arc<Self>> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .with_context(|| format!("ws connect {}", url))?;
        let (mut sink, mut stream) = ws.split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let (notif_tx, notif_rx) = mpsc::unbounded_channel::<Notification>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        // Writer task
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

    #[cfg(unix)]
    pub async fn connect_daemon_socket(path: &Path) -> Result<Arc<Self>> {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .with_context(|| format!("connect daemon socket {}", path.display()))?;
        let (reader, mut writer) = stream.into_split();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let (notif_tx, notif_rx) = mpsc::unbounded_channel::<Notification>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if writer.write_all(frame.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
            }
            let _ = writer.shutdown().await;
        });

        let pending_r = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(text)) => dispatch_frame(text, &pending_r, &notif_tx).await,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(%e, "daemon socket read failed");
                        break;
                    }
                }
            }
        });

        Ok(Self::new(out_tx, pending, notif_rx))
    }

    #[cfg(not(unix))]
    pub async fn connect_daemon_socket(_path: &Path) -> Result<Arc<Self>> {
        Err(anyhow!(
            "joi daemon IPC is only supported on Unix platforms"
        ))
    }

    fn new(
        out_tx: mpsc::UnboundedSender<String>,
        pending: Pending,
        notif_rx: mpsc::UnboundedReceiver<Notification>,
    ) -> Arc<Self> {
        Arc::new(Self {
            out_tx,
            pending,
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            notifications: Mutex::new(notif_rx),
        })
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
            .map_err(|_| anyhow!("rpc writer closed"))?;
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

    pub async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R> {
        let value = serde_json::to_value(params)?;
        let result = self.call_raw(method, Some(value)).await?;
        Ok(serde_json::from_value(result)?)
    }

    pub async fn initialize(&self) -> Result<()> {
        let _: Value = self
            .call_raw(
                method::INITIALIZE,
                Some(json!({
                    "protocolVersion": proto::PROTOCOL_VERSION,
                    "clientInfo": { "name": "joi-cli", "title": "Joi CLI", "version": env!("CARGO_PKG_VERSION") },
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
        self.open_connection_as(actor_id, "human", display_name)
            .await
    }

    /// Open a connection bound to `actor_id` with an explicit `actor_kind`
    /// (`"human"` or `"agent"`). The v1 `joi agent serve` worker uses this to
    /// register one connection per managed agent so the server's actor-inbox
    /// delivery routes hands_off_to events to the right WS.
    pub async fn open_connection_as(
        &self,
        actor_id: &str,
        actor_kind: &str,
        display_name: Option<&str>,
    ) -> Result<Value> {
        self.call_raw(
            method::CONNECTION_OPEN,
            Some(json!({
                "actorId": actor_id,
                "actorKind": actor_kind,
                "displayName": display_name.unwrap_or(actor_id),
            })),
        )
        .await
    }
}

fn id_to_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => "_".into(),
    }
}

async fn dispatch_frame(
    text: String,
    pending: &Pending,
    notif_tx: &mpsc::UnboundedSender<Notification>,
) {
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
            let _ = notif_tx.send(n);
        }
        RpcEnvelope::Request(_) => {
            // Server doesn't send requests in v0.
        }
    }
}
