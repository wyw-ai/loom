use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
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
        if let Some(path) = file_rpc_url_path(url) {
            return Self::connect_file_rpc(path).await;
        }
        if let Some(path) = unix_url_path(url) {
            return Self::connect_unix_url(path).await;
        }
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

    #[cfg(unix)]
    async fn connect_unix_url(path: &str) -> Result<Arc<Self>> {
        if path.is_empty() {
            return Err(anyhow!("unix server URL is missing a socket path"));
        }
        Self::connect_daemon_socket(Path::new(path)).await
    }

    #[cfg(not(unix))]
    async fn connect_unix_url(_path: &str) -> Result<Arc<Self>> {
        Err(anyhow!(
            "unix server URLs are only supported on Unix platforms"
        ))
    }

    #[cfg(not(unix))]
    pub async fn connect_daemon_socket(_path: &Path) -> Result<Arc<Self>> {
        Err(anyhow!(
            "joi daemon IPC is only supported on Unix platforms"
        ))
    }

    async fn connect_file_rpc(path: &str) -> Result<Arc<Self>> {
        if path.is_empty() {
            return Err(anyhow!("file-rpc server URL is missing a directory path"));
        }
        let root = PathBuf::from(path);
        let client_id = format!(
            "conn_file_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let client_dir = root.join("clients").join(client_id);
        let in_dir = client_dir.join("in");
        let out_dir = client_dir.join("out");
        std::fs::create_dir_all(&in_dir)
            .with_context(|| format!("create file-rpc input dir {}", in_dir.display()))?;
        std::fs::create_dir_all(&out_dir)
            .with_context(|| format!("create file-rpc output dir {}", out_dir.display()))?;

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let (notif_tx, notif_rx) = mpsc::unbounded_channel::<Notification>();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(async move {
            let mut seq = 0_u64;
            while let Some(frame) = out_rx.recv().await {
                seq = seq.saturating_add(1);
                if write_frame_file(&in_dir, seq, &frame).is_err() {
                    break;
                }
            }
        });

        let pending_r = pending.clone();
        tokio::spawn(async move {
            let mut seen = BTreeSet::new();
            loop {
                if let Ok(entries) = std::fs::read_dir(&out_dir) {
                    let mut files = entries
                        .filter_map(Result::ok)
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
                        if !seen.insert(name) {
                            continue;
                        }
                        if let Ok(text) = std::fs::read_to_string(&file) {
                            dispatch_frame(text, &pending_r, &notif_tx).await;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });

        Ok(Self::new(out_tx, pending, notif_rx))
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
    /// (`"human"` or `"agent"`). The daemon-managed agent worker uses this to
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

    pub async fn open_observer_connection_as(
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
                "claimInbox": false,
            })),
        )
        .await
    }
}

fn unix_url_path(url: &str) -> Option<&str> {
    url.strip_prefix("unix://")
        .or_else(|| url.strip_prefix("unix:"))
}

fn file_rpc_url_path(url: &str) -> Option<&str> {
    url.strip_prefix("file-rpc://")
        .or_else(|| url.strip_prefix("file-rpc:"))
}

fn write_frame_file(dir: &Path, seq: u64, frame: &str) -> std::io::Result<()> {
    let final_path = dir.join(format!("{seq:020}.json"));
    let tmp_path = dir.join(format!("{seq:020}.json.tmp"));
    std::fs::write(&tmp_path, frame)?;
    std::fs::rename(tmp_path, final_path)
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
