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

use anyhow::{Context, Result};
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
    loom_platform::console::init();
    agent_runtime::tracing_setup::init_file_tracing("server", "info");
    let startup_started = std::time::Instant::now();

    // Install a panic hook that logs panics to the tracing system.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else {
            "unknown panic payload".to_string()
        };
        tracing::error!(
            location = %location,
            payload = %payload,
            "loom-server PANIC"
        );
        default_hook(info);
    }));

    let args = Args::parse();
    let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
    tracing::info!(
        data_dir = %data_dir.display(),
        bind = %args.bind,
        unix_socket = %args.unix_socket.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        file_rpc = %args.file_rpc.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        "loom-server startup begin"
    );
    let stage_started = std::time::Instant::now();
    std::fs::create_dir_all(&data_dir)?;
    tracing::info!(
        elapsed_ms = stage_started.elapsed().as_millis(),
        "loom-server data directory ready"
    );

    let stage_started = std::time::Instant::now();
    tracing::info!("loom-server opening sqlite journal");
    let journal = Journal::open_sqlite(data_dir.join("loom.sqlite3"))?;
    tracing::info!(
        elapsed_ms = stage_started.elapsed().as_millis(),
        "loom-server sqlite journal ready"
    );
    let stage_started = std::time::Instant::now();
    tracing::info!("loom-server opening store");
    let store = Store::open(journal)?;
    tracing::info!(
        elapsed_ms = stage_started.elapsed().as_millis(),
        "loom-server store ready"
    );
    let subscriptions = Subscriptions::new();
    let stage_started = std::time::Instant::now();
    let artifacts = Arc::new(ArtifactStore::new(
        data_dir.join("artifacts"),
        data_dir.join("workspaces"),
    )?);
    tracing::info!(
        elapsed_ms = stage_started.elapsed().as_millis(),
        "loom-server artifact store ready"
    );
    let stage_started = std::time::Instant::now();
    let scope_skills = Arc::new(ScopeSkills::new(
        data_dir.join("workspaces"),
        data_dir.join("agents"),
    )?);
    let machine_commands = MachineCommandWaiters::new();
    scope_skills.reconcile(&store)?;
    tracing::info!(
        elapsed_ms = stage_started.elapsed().as_millis(),
        "loom-server scope skills ready"
    );

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
    tracing::info!(
        %addr,
        startup_elapsed_ms = startup_started.elapsed().as_millis(),
        "loom-server binding tcp listener"
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(
        %addr,
        startup_elapsed_ms = startup_started.elapsed().as_millis(),
        "loom-server listening"
    );
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

async fn serve_unix(state: AppState, socket: PathBuf) -> Result<()> {
    use loom_platform::ipc::{LocalListener, LocalSocketName};

    if let Some(parent) = socket.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create local socket dir {}", parent.display()))?;
        }
    }

    let name = LocalSocketName::from_path(socket.clone())
        .with_context(|| format!("build socket name from {}", socket.display()))?;
    let listener = LocalListener::bind(&name)
        .await
        .with_context(|| format!("bind local socket {}", name.display()))?;
    tracing::info!(socket = %name.display(), "loom-server listening on local socket");
    loop {
        let stream = listener
            .accept()
            .await
            .with_context(|| format!("accept local socket {}", name.display()))?;
        tokio::spawn(ws::handle_local_socket(state.clone(), stream));
    }
}

fn spawn_reminder_worker(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            // fire_due_reminders is infallible (returns Vec, not Result).
            // RwLock poisoning is handled by rc4 mutex-poisoning fixes.
            let fired = state.store.fire_due_reminders();
            for reminder in fired {
                tracing::debug!(reminder = %reminder.id, "reminder processed");
            }
        }
    });
}
