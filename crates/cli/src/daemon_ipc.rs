use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use crate::config;

pub const ENV_DAEMON_SOCKET: &str = "JOI_DAEMON_SOCKET";
pub const ENV_NO_DAEMON: &str = "JOI_NO_DAEMON";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketSource {
    Env,
    Discovery,
}

#[derive(Debug, Clone)]
pub struct ResolvedSocket {
    pub path: PathBuf,
    pub source: SocketSource,
    pub server_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonDiscovery {
    pid: u32,
    socket: PathBuf,
    server_url: String,
}

pub fn daemon_disabled() -> bool {
    std::env::var(ENV_NO_DAEMON)
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

pub fn default_socket_path() -> PathBuf {
    config::config_dir().join("daemon").join("daemon.sock")
}

pub fn discovery_path() -> PathBuf {
    config::config_dir().join("daemon").join("discovery.json")
}

pub fn env_socket_path() -> Option<PathBuf> {
    std::env::var(ENV_DAEMON_SOCKET)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn resolve_socket() -> Option<ResolvedSocket> {
    if let Some(path) = env_socket_path() {
        let server_url = read_discovery()
            .filter(|discovery| discovery.socket == path)
            .map(|discovery| discovery.server_url);
        return Some(ResolvedSocket {
            path,
            source: SocketSource::Env,
            server_url,
        });
    }

    let discovery = read_discovery()?;
    if !discovery.socket.exists() {
        return None;
    }
    Some(ResolvedSocket {
        path: discovery.socket,
        source: SocketSource::Discovery,
        server_url: Some(discovery.server_url),
    })
}

fn read_discovery() -> Option<DaemonDiscovery> {
    let text = std::fs::read_to_string(discovery_path()).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_discovery(socket: &Path, server_url: &str) -> Result<()> {
    let path = discovery_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create daemon discovery dir {}", parent.display()))?;
    }
    let discovery = DaemonDiscovery {
        pid: std::process::id(),
        socket: socket.to_path_buf(),
        server_url: server_url.to_string(),
    };
    let text = serde_json::to_string_pretty(&discovery)?;
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))
}

pub fn remove_discovery_for(socket: &Path) {
    let path = discovery_path();
    let should_remove = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<DaemonDiscovery>(&text).ok())
        .map(|discovery| discovery.socket == socket)
        .unwrap_or(true);
    if should_remove {
        let _ = std::fs::remove_file(path);
    }
}

pub async fn start_proxy(socket: PathBuf, server_url: String) -> Result<JoinHandle<()>> {
    #[cfg(not(unix))]
    {
        let _ = (socket, server_url);
        bail!("joi daemon IPC is only supported on Unix platforms");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use tokio::net::UnixListener;

        prepare_socket_path(&socket).await?;
        let listener = UnixListener::bind(&socket)
            .with_context(|| format!("bind daemon socket {}", socket.display()))?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod daemon socket {}", socket.display()))?;

        Ok(tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let server_url = server_url.clone();
                        tokio::spawn(async move {
                            if let Err(err) = proxy_client(stream, server_url).await {
                                tracing::warn!(error = %err, "daemon IPC client disconnected");
                            }
                        });
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "daemon IPC accept failed");
                    }
                }
            }
        }))
    }
}

pub async fn cleanup_socket(socket: &Path) {
    let _ = std::fs::remove_file(socket);
}

#[cfg(unix)]
async fn prepare_socket_path(socket: &Path) -> Result<()> {
    use tokio::net::UnixStream;

    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create daemon socket dir {}", parent.display()))?;
    }

    if socket.exists() {
        if UnixStream::connect(socket).await.is_ok() {
            bail!("joi daemon socket is already active: {}", socket.display());
        }
        std::fs::remove_file(socket)
            .with_context(|| format!("remove stale daemon socket {}", socket.display()))?;
    }

    Ok(())
}

#[cfg(unix)]
async fn proxy_client(stream: tokio::net::UnixStream, server_url: String) -> Result<()> {
    if let Some(path) = unix_url_path(&server_url) {
        return proxy_client_to_unix(stream, path).await;
    }

    let (ws, _) = tokio_tungstenite::connect_async(&server_url)
        .await
        .with_context(|| format!("ws connect {}", server_url))?;
    let (mut ws_sink, mut ws_stream) = ws.split();
    let (local_reader, mut local_writer) = stream.into_split();
    let mut local_lines = BufReader::new(local_reader).lines();

    let local_to_ws = async {
        while let Some(line) = local_lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            ws_sink.send(Message::Text(line)).await?;
        }
        let _ = ws_sink.close().await;
        Ok::<(), anyhow::Error>(())
    };

    let ws_to_local = async {
        while let Some(message) = ws_stream.next().await {
            let message = message?;
            let text = match message {
                Message::Text(text) => text,
                Message::Binary(bytes) => String::from_utf8(bytes)
                    .map_err(|err| anyhow!("server sent non-utf8 frame: {}", err))?,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                Message::Close(_) => break,
            };
            local_writer.write_all(text.as_bytes()).await?;
            local_writer.write_all(b"\n").await?;
        }
        let _ = local_writer.shutdown().await;
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        result = local_to_ws => result,
        result = ws_to_local => result,
    }
}

#[cfg(unix)]
async fn proxy_client_to_unix(stream: tokio::net::UnixStream, path: &str) -> Result<()> {
    let server = tokio::net::UnixStream::connect(path)
        .await
        .with_context(|| format!("connect unix server socket {}", path))?;
    let (server_reader, mut server_writer) = server.into_split();
    let (local_reader, mut local_writer) = stream.into_split();
    let mut local_lines = BufReader::new(local_reader).lines();
    let mut server_lines = BufReader::new(server_reader).lines();

    let local_to_server = async {
        while let Some(line) = local_lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            server_writer.write_all(line.as_bytes()).await?;
            server_writer.write_all(b"\n").await?;
        }
        let _ = server_writer.shutdown().await;
        Ok::<(), anyhow::Error>(())
    };

    let server_to_local = async {
        while let Some(line) = server_lines.next_line().await? {
            local_writer.write_all(line.as_bytes()).await?;
            local_writer.write_all(b"\n").await?;
        }
        let _ = local_writer.shutdown().await;
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        result = local_to_server => result,
        result = server_to_local => result,
    }
}

#[cfg(unix)]
fn unix_url_path(url: &str) -> Option<&str> {
    url.strip_prefix("unix://")
        .or_else(|| url.strip_prefix("unix:"))
}
