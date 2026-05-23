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

    /// Data directory (SQLite store + artifacts)
    #[arg(long, default_value = "./data", env = "LOOM_DATA_DIR")]
    data_dir: PathBuf,

    /// Unix socket to bind for local JSON-line RPC instead of TCP WebSocket.
    #[arg(long, env = "LOOM_UNIX_SOCKET")]
    unix_socket: Option<PathBuf>,

    /// Directory to use for local file-based JSON RPC instead of sockets.
    #[arg(long, env = "LOOM_FILE_RPC")]
    file_rpc: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    std::fs::create_dir_all(&args.data_dir)?;

    let journal = Journal::open_sqlite(args.data_dir.join("loom.sqlite3"))?;
    let store = Store::open(journal)?;
    let subscriptions = Subscriptions::new();
    let artifacts = Arc::new(ArtifactStore::new(
        args.data_dir.join("artifacts"),
        args.data_dir.join("workspaces"),
    )?);
    let scope_skills = Arc::new(ScopeSkills::new(
        args.data_dir.join("workspaces"),
        args.data_dir.join("agents"),
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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}
