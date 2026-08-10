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
use proto::methods::{method, AuthLoginResult, InitializeResult};
use proto::{Notification, Request, Response, RpcEnvelope};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Response>>>>;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);
const INBOUND_TIMEOUT: Duration = Duration::from_secs(60);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Client {
    out_tx: mpsc::Sender<String>,
    pending: Pending,
    next_id: Arc<std::sync::atomic::AtomicU64>,
    pub notifications: Mutex<Option<mpsc::Receiver<Notification>>>,
    disconnect_reason: Arc<Mutex<Option<String>>>,
}

impl Client {
    pub async fn connect(url: &str) -> Result<Arc<Self>> {
        Self::connect_with_timing(url, KEEPALIVE_INTERVAL, INBOUND_TIMEOUT).await
    }

    #[cfg(test)]
    async fn connect_with_keepalive(url: &str, keepalive_interval: Duration) -> Result<Arc<Self>> {
        Self::connect_with_timing(url, keepalive_interval, INBOUND_TIMEOUT).await
    }

    async fn connect_with_timing(
        url: &str,
        keepalive_interval: Duration,
        inbound_timeout: Duration,
    ) -> Result<Arc<Self>> {
        let (ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
            .await
            .map_err(|_| anyhow!("ws connect {url} timed out after 10s"))?
            .with_context(|| format!("ws connect {}", url))?;
        let (mut sink, mut stream) = ws.split();
        let (out_tx, mut out_rx) = mpsc::channel::<String>(4096);
        let (notif_tx, notif_rx) = mpsc::channel::<Notification>(1024);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let disconnect_reason = Arc::new(Mutex::new(None));
        let last_inbound = Arc::new(Mutex::new(Instant::now()));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let writer_disconnect_reason = disconnect_reason.clone();
        let writer_pending = pending.clone();
        let writer_shutdown_tx = shutdown_tx.clone();
        let mut writer_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            // Client-originated traffic is required as well as the server's
            // downstream ping. Some NATs and reverse proxies expire an idle
            // upstream flow even while server-to-client frames are arriving.
            let start = tokio::time::Instant::now() + keepalive_interval;
            let mut ping_ticker = tokio::time::interval_at(start, keepalive_interval);
            let terminal_reason = loop {
                tokio::select! {
                    biased;
                    changed = writer_shutdown_rx.changed() => {
                        if changed.is_err() || *writer_shutdown_rx.borrow() {
                            break None;
                        }
                    }
                    frame = out_rx.recv() => {
                        match frame {
                            Some(frame) => {
                                if let Err(err) = sink.send(Message::Text(frame)).await {
                                    break Some(format!("WebSocket write failed: {err}"));
                                }
                            }
                            None => break Some("WebSocket client writer closed".into()),
                        }
                    }
                    _ = ping_ticker.tick() => {
                        if let Err(err) = sink.send(Message::Ping(Vec::new())).await {
                            break Some(format!("WebSocket keepalive failed: {err}"));
                        }
                    }
                }
            };
            if let Some(reason) = terminal_reason {
                signal_disconnect(
                    &writer_disconnect_reason,
                    &writer_pending,
                    &writer_shutdown_tx,
                    reason,
                )
                .await;
            }
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, sink.close()).await;
        });

        let pending_r = pending.clone();
        let reader_disconnect_reason = disconnect_reason.clone();
        let reader_shutdown_tx = shutdown_tx.clone();
        let mut reader_shutdown_rx = shutdown_rx.clone();
        let reader_last_inbound = last_inbound.clone();
        tokio::spawn(async move {
            let reason = loop {
                let msg = tokio::select! {
                    biased;
                    changed = reader_shutdown_rx.changed() => {
                        if changed.is_err() || *reader_shutdown_rx.borrow() {
                            break None;
                        }
                        continue;
                    }
                    msg = stream.next() => msg,
                };
                let msg = match msg {
                    Some(Ok(msg)) => msg,
                    Some(Err(err)) => break Some(format!("WebSocket read failed: {err}")),
                    None => break Some("WebSocket stream ended".to_string()),
                };
                *reader_last_inbound.lock().await = Instant::now();
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Binary(b) => match String::from_utf8(b) {
                        Ok(s) => s,
                        Err(_) => continue,
                    },
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                    Message::Close(frame) => break Some(close_reason(frame)),
                };
                dispatch_frame(text, &pending_r, &notif_tx).await;
            };
            if let Some(reason) = reason {
                signal_disconnect(
                    &reader_disconnect_reason,
                    &pending_r,
                    &reader_shutdown_tx,
                    reason,
                )
                .await;
            }
        });

        let watchdog_disconnect_reason = disconnect_reason.clone();
        let watchdog_pending = pending.clone();
        let watchdog_shutdown_tx = shutdown_tx.clone();
        let mut watchdog_shutdown_rx = shutdown_rx;
        tokio::spawn(async move {
            loop {
                let deadline = *last_inbound.lock().await + inbound_timeout;
                tokio::select! {
                    biased;
                    changed = watchdog_shutdown_rx.changed() => {
                        if changed.is_err() || *watchdog_shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        let elapsed = Instant::now().saturating_duration_since(
                            *last_inbound.lock().await,
                        );
                        if elapsed < inbound_timeout {
                            continue;
                        }
                        signal_disconnect(
                            &watchdog_disconnect_reason,
                            &watchdog_pending,
                            &watchdog_shutdown_tx,
                            format!(
                                "WebSocket inbound watchdog timed out after {}s",
                                inbound_timeout.as_secs_f64(),
                            ),
                        )
                        .await;
                        break;
                    }
                }
            }
        });

        Ok(Self::new(out_tx, pending, notif_rx, disconnect_reason))
    }

    fn new(
        out_tx: mpsc::Sender<String>,
        pending: Pending,
        notif_rx: mpsc::Receiver<Notification>,
        disconnect_reason: Arc<Mutex<Option<String>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            out_tx,
            pending,
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            notifications: Mutex::new(Some(notif_rx)),
            disconnect_reason,
        })
    }

    #[cfg(test)]
    pub(crate) fn test_client() -> Arc<Self> {
        let (out_tx, _out_rx) = mpsc::channel(1);
        let (_notif_tx, notif_rx) = mpsc::channel(1);
        Self::new(
            out_tx,
            Arc::new(Mutex::new(HashMap::new())),
            notif_rx,
            Arc::new(Mutex::new(None)),
        )
    }

    pub async fn take_notifications(&self) -> Option<mpsc::Receiver<Notification>> {
        self.notifications.lock().await.take()
    }

    pub async fn disconnect_reason(&self) -> Option<String> {
        self.disconnect_reason.lock().await.clone()
    }

    pub async fn call_raw(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id_value = json!(id);
        let key = id_to_key(&id_value);
        let req = Request::new(id_value, method, params);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(key.clone(), tx);
        let frame = serde_json::to_string(&req)?;
        if self.out_tx.send(frame).await.is_err() {
            self.pending.lock().await.remove(&key);
            return Err(anyhow!("ws writer closed"));
        }
        let resp = match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => return Err(anyhow!("rpc `{}` channel closed", method)),
            Err(_) => {
                self.pending.lock().await.remove(&key);
                return Err(anyhow!("rpc `{}` timed out", method));
            }
        };
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

    pub async fn initialize(
        &self,
        client_name: &str,
        client_version: &str,
    ) -> Result<InitializeResult> {
        let value = self
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
        Ok(serde_json::from_value(value)?)
    }

    pub async fn authenticate(&self, password: &str) -> Result<AuthLoginResult> {
        let value = self
            .call_raw(method::AUTH_LOGIN, Some(json!({ "password": password })))
            .await?;
        Ok(serde_json::from_value(value)?)
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

async fn record_disconnect_reason(reason: &Mutex<Option<String>>, value: String) {
    let mut current = reason.lock().await;
    if current.is_none() {
        *current = Some(value);
    }
}

async fn signal_disconnect(
    disconnect_reason: &Mutex<Option<String>>,
    pending: &Pending,
    shutdown_tx: &watch::Sender<bool>,
    reason: String,
) {
    record_disconnect_reason(disconnect_reason, reason).await;
    // Dropping the senders wakes every in-flight RPC immediately. The shared
    // shutdown signal also terminates the peer I/O task so notification
    // forwarding closes and the UI can begin reconnecting.
    pending.lock().await.clear();
    let _ = shutdown_tx.send(true);
}

fn close_reason(
    frame: Option<tokio_tungstenite::tungstenite::protocol::CloseFrame<'static>>,
) -> String {
    match frame {
        Some(frame) if frame.reason.is_empty() => {
            format!("server closed WebSocket (code {})", u16::from(frame.code))
        }
        Some(frame) => format!(
            "server closed WebSocket (code {}): {}",
            u16::from(frame.code),
            frame.reason
        ),
        None => "server closed WebSocket without a close frame".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;

    #[tokio::test]
    async fn sends_client_keepalive_ping_while_rpc_is_idle() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = accept_async(stream).await.expect("websocket accept");
            loop {
                match socket.next().await {
                    Some(Ok(Message::Ping(payload))) => return payload,
                    Some(Ok(_)) => continue,
                    Some(Err(err)) => panic!("server websocket read failed: {err}"),
                    None => panic!("client closed before keepalive"),
                }
            }
        });

        let url = format!("ws://{address}");
        let _client = Client::connect_with_keepalive(&url, Duration::from_millis(20))
            .await
            .expect("client connect");

        let payload = tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .expect("keepalive deadline")
            .expect("server task");
        assert!(payload.is_empty());
    }

    #[tokio::test]
    async fn transport_close_fails_pending_rpc_and_preserves_close_reason() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = accept_async(stream).await.expect("websocket accept");
            let request = socket
                .next()
                .await
                .expect("request frame")
                .expect("request");
            assert!(matches!(request, Message::Text(_)));
            socket
                .close(Some(CloseFrame {
                    code: CloseCode::Away,
                    reason: "maintenance".into(),
                }))
                .await
                .expect("close websocket");
        });

        let url = format!("ws://{address}");
        let client = Client::connect_with_keepalive(&url, Duration::from_secs(60))
            .await
            .expect("client connect");
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            client.call_raw("test/pending", None),
        )
        .await
        .expect("pending RPC should fail immediately")
        .expect_err("RPC must fail when transport closes");
        assert!(error.to_string().contains("channel closed"));

        server.await.expect("server task");
        let reason = client.disconnect_reason().await.expect("disconnect reason");
        assert!(reason.contains("1001"));
        assert!(reason.contains("maintenance"));
    }

    #[tokio::test]
    async fn inbound_watchdog_closes_blackholed_transport_and_fails_pending_rpc() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let _socket = accept_async(stream).await.expect("websocket accept");
            // Keep TCP open without reading or sending frames. Client writes
            // continue to succeed, so only an inbound deadline can detect it.
            tokio::time::sleep(Duration::from_millis(250)).await;
        });

        let url = format!("ws://{address}");
        let client =
            Client::connect_with_timing(&url, Duration::from_millis(20), Duration::from_millis(80))
                .await
                .expect("client connect");
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            client.call_raw("test/blackhole", None),
        )
        .await
        .expect("watchdog deadline")
        .expect_err("watchdog must fail the pending RPC");
        assert!(error.to_string().contains("channel closed"));

        let reason = client.disconnect_reason().await.expect("disconnect reason");
        assert!(reason.contains("inbound watchdog timed out"));
        server.await.expect("server task");
    }
}
