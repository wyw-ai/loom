mod artifacts;
mod handlers;
mod journal;
mod machine_commands;
mod scope_skills;
mod state;
mod store;
mod subscribe;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use axum::routing::get;
use axum::Router;
use clap::Parser;

use crate::artifacts::ArtifactStore;
use crate::journal::Journal;
use crate::machine_commands::MachineCommandWaiters;
use crate::scope_skills::ScopeSkills;
use crate::state::AppState;
use crate::store::Store;
use crate::subscribe::Subscriptions;

#[derive(Debug, Parser)]
#[command(name = "loom-server", about = "Loom multi-actor collaboration server")]
struct Args {
    /// Address to bind, e.g. 127.0.0.1:7878
    #[arg(long, default_value = "127.0.0.1:7878")]
    bind: String,

    /// Data directory (SQLite store + artifacts). Defaults to the OS data
    /// directory under `loom/server`.
    #[arg(long, env = "LOOM_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Unix socket to bind for local JSON-line RPC instead of TCP WebSocket.
    #[arg(long, env = "LOOM_UNIX_SOCKET")]
    unix_socket: Option<PathBuf>,

    /// Directory to use for local file-based JSON RPC instead of sockets.
    #[arg(long, env = "LOOM_FILE_RPC")]
    file_rpc: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    windows_console::init();
    init_tracing();
    let args = Args::parse();
    let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
    std::fs::create_dir_all(&data_dir)?;

    let journal = Journal::open_sqlite(data_dir.join("loom.sqlite3"))?;
    let store = Store::open(journal)?;
    let subscriptions = Subscriptions::new();
    let artifacts = Arc::new(ArtifactStore::new(
        data_dir.join("artifacts"),
        data_dir.join("workspaces"),
    )?);
    let scope_skills = Arc::new(ScopeSkills::new(
        data_dir.join("workspaces"),
        data_dir.join("agents"),
    )?);
    let machine_commands = MachineCommandWaiters::new();
    scope_skills.reconcile(&store)?;

    let state = AppState {
        store: store.clone(),
        subscriptions,
        artifacts,
        scope_skills,
        machine_commands,
    };

    // Stream broadcaster (store events -> stream/update notifications).
    ws::spawn_stream_broadcaster(state.clone());
    spawn_reminder_worker(state.clone());

    if let Some(socket) = args.unix_socket {
        return serve_unix(state, socket).await;
    }
    if let Some(root) = args.file_rpc {
        return serve_file_rpc(state, root).await;
    }

    let app = Router::new()
        .route("/rpc", get(ws::ws_upgrade))
        .with_state(state);
    let addr: std::net::SocketAddr = args.bind.parse()?;
    tracing::info!(%addr, "loom-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|dir| dir.join("loom").join("server"))
        .unwrap_or_else(|| PathBuf::from(".loom").join("server-data"))
}

#[cfg(test)]
mod tests {
    use super::default_data_dir;

    #[test]
    fn default_data_dir_is_not_repo_data_dir() {
        let path = default_data_dir();
        assert!(path.ends_with("loom/server") || path.ends_with(".loom/server-data"));
        assert_ne!(path, std::path::PathBuf::from("./data"));
    }
}

async fn serve_file_rpc(state: AppState, root: PathBuf) -> Result<()> {
    ws::spawn_file_rpc(state, root.clone())
        .with_context(|| format!("start file-rpc transport {}", root.display()))?;
    tracing::info!(root = %root.display(), "loom-server listening on file-rpc directory");
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(unix)]
async fn serve_unix(state: AppState, socket: PathBuf) -> Result<()> {
    use std::os::unix::fs::FileTypeExt;

    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create unix socket dir {}", parent.display()))?;
    }
    if socket.exists() {
        let file_type = std::fs::symlink_metadata(&socket)
            .with_context(|| format!("stat unix socket {}", socket.display()))?
            .file_type();
        if !file_type.is_socket() {
            bail!("{} exists and is not a unix socket", socket.display());
        }
        std::fs::remove_file(&socket)
            .with_context(|| format!("remove stale unix socket {}", socket.display()))?;
    }

    let listener = tokio::net::UnixListener::bind(&socket)
        .with_context(|| format!("bind unix socket {}", socket.display()))?;
    tracing::info!(socket = %socket.display(), "loom-server listening on unix socket");
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(ws::handle_unix_socket(state.clone(), stream));
    }
}

#[cfg(not(unix))]
async fn serve_unix(_state: AppState, socket: PathBuf) -> Result<()> {
    let _ = socket;
    bail!("unix sockets are only supported on Unix platforms");
}

fn spawn_reminder_worker(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let fired = state.store.fire_due_reminders();
            for reminder in fired {
                tracing::debug!(reminder = %reminder.id, "reminder processed");
            }
        }
    });
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::prelude::*;
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // File log directory.  On Windows: %LOCALAPPDATA%\loom\logs\server.
    // On Unix: ~/.local/share/loom/logs/server.
    #[cfg(target_os = "windows")]
    let log_dir = {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(local).join("loom").join("logs").join("server")
    };
    #[cfg(not(target_os = "windows"))]
    let log_dir = {
        let home = dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        home.join("loom").join("logs").join("server")
    };
    let _ = std::fs::create_dir_all(&log_dir);

    // Use a fixed-name log file (not rolling) so loom-shell can always find it.
    let log_path = log_dir.join("loom-server.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("failed to open loom-server.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file);

    // Stderr layer for console/daemon manager output.
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr);

    // File layer for persistent log storage.
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
    // Keep _guard alive so non_blocking worker stays active.
    std::mem::forget(_guard);
    tracing::info!(path = %log_path.display(), "loom-server logging to file");
}
