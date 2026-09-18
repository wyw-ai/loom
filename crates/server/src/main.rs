mod artifacts;
mod auth;
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
use crate::auth::ServerAuth;
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

    /// Shared password required by every client transport. Prefer
    /// --password-file so the secret is not visible in the process list.
    #[arg(
        long,
        env = "LOOM_SERVER_PASSWORD",
        hide_env_values = true,
        conflicts_with = "password_file"
    )]
    password: Option<String>,

    /// Read the shared server password from a UTF-8 file. One trailing CR/LF
    /// sequence is ignored so ordinary secret files work as expected.
    #[arg(long, env = "LOOM_SERVER_PASSWORD_FILE", conflicts_with = "password")]
    password_file: Option<PathBuf>,
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
    let password = resolve_server_password(args.password, args.password_file.as_deref())?;
    let auth = Arc::new(match password.as_deref() {
        Some(password) => ServerAuth::with_password(password).map_err(anyhow::Error::msg)?,
        None => ServerAuth::disabled(),
    });
    let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
    tracing::info!(
        data_dir = %data_dir.display(),
        bind = %args.bind,
        unix_socket = %args.unix_socket.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        file_rpc = %args.file_rpc.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        password_required = auth.required(),
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
        auth,
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

fn resolve_server_password(
    inline: Option<String>,
    password_file: Option<&std::path::Path>,
) -> Result<Option<String>> {
    let mut password = match (inline, password_file) {
        (Some(password), None) => password,
        (None, Some(path)) => std::fs::read_to_string(path)
            .with_context(|| format!("read server password file {}", path.display()))?,
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => anyhow::bail!("pass either --password or --password-file, not both"),
    };
    while matches!(password.chars().last(), Some('\r' | '\n')) {
        password.pop();
    }
    if password.is_empty() {
        anyhow::bail!("server password cannot be empty");
    }
    Ok(Some(password))
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|dir| dir.join("loom").join("server"))
        .unwrap_or_else(|| PathBuf::from(".loom").join("server-data"))
}

#[cfg(test)]
mod tests {
    use super::{default_data_dir, resolve_server_password};

    #[test]
    fn default_data_dir_is_not_repo_data_dir() {
        let path = default_data_dir();
        assert!(path.ends_with("loom/server") || path.ends_with(".loom/server-data"));
        assert_ne!(path, std::path::PathBuf::from("./data"));
    }

    #[test]
    fn password_file_trims_only_line_endings() {
        let root = std::env::temp_dir().join(format!(
            "loom-server-password-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&root, "  secret phrase  \r\n").expect("write password");
        let password = resolve_server_password(None, Some(&root)).expect("read password");
        let _ = std::fs::remove_file(&root);
        assert_eq!(password.as_deref(), Some("  secret phrase  "));
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
