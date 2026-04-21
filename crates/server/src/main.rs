mod artifacts;
mod handlers;
mod journal;
mod runtime;
mod state;
mod store;
mod subscribe;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::routing::get;
use axum::Router;
use clap::Parser;

use crate::artifacts::ArtifactStore;
use crate::journal::Journal;
use crate::runtime::RuntimeManager;
use crate::state::AppState;
use crate::store::Store;
use crate::subscribe::Subscriptions;

#[derive(Debug, Parser)]
#[command(
    name = "joi-server",
    about = "Open Multi-Actor Collaboration Protocol v0 server"
)]
struct Args {
    /// Address to bind, e.g. 127.0.0.1:7878
    #[arg(long, default_value = "127.0.0.1:7878")]
    bind: String,

    /// Data directory (journal + artifacts + per-agent workspaces)
    #[arg(long, default_value = "./data", env = "JOI_DATA_DIR")]
    data_dir: PathBuf,

    /// Directory containing agent JSON specs
    #[arg(long, default_value = "./agents", env = "JOI_AGENTS_DIR")]
    agents_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    std::fs::create_dir_all(&args.data_dir)?;
    std::fs::create_dir_all(&args.agents_dir)?;

    let journal = Journal::open(args.data_dir.join("journal.jsonl"))?;
    let store = Store::open(journal)?;
    let subscriptions = Subscriptions::new();
    let artifacts = Arc::new(ArtifactStore::new(args.data_dir.join("artifacts"))?);
    let server_url = format!("ws://{}/rpc", args.bind);
    let runtime = RuntimeManager::new(
        args.data_dir.clone(),
        args.agents_dir.clone(),
        store.clone(),
        server_url,
    )?;

    let state = AppState {
        store: store.clone(),
        subscriptions,
        runtime: runtime.clone(),
        artifacts,
    };

    // Stream broadcaster (store events -> stream/update notifications).
    ws::spawn_stream_broadcaster(state.clone());
    // Runtime supervisor (store events -> ACP child wakeup). Set
    // `JOI_DISABLE_EMBEDDED_RUNTIME=1` to opt into the v1 model where an
    // external `joi agent serve` process drives agents instead.
    let disable_embedded = std::env::var("JOI_DISABLE_EMBEDDED_RUNTIME")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false);
    if disable_embedded {
        tracing::info!(
            "embedded agent runtime disabled (JOI_DISABLE_EMBEDDED_RUNTIME); \
             run `joi agent serve` to drive registered agents."
        );
    } else {
        runtime::wakeup::spawn_supervisor(runtime.clone(), store.clone());
    }

    let app = Router::new()
        .route("/rpc", get(ws::ws_upgrade))
        .with_state(state);

    let addr: std::net::SocketAddr = args.bind.parse()?;
    tracing::info!(%addr, "joi-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}
