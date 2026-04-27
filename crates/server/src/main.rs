mod artifacts;
mod handlers;
mod journal;
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

    /// Data directory (journal + artifacts)
    #[arg(long, default_value = "./data", env = "JOI_DATA_DIR")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    std::fs::create_dir_all(&args.data_dir)?;

    let journal = Journal::open(args.data_dir.join("journal.jsonl"))?;
    let store = Store::open(journal)?;
    let subscriptions = Subscriptions::new();
    let artifacts = Arc::new(ArtifactStore::new(
        args.data_dir.join("artifacts"),
        args.data_dir.join("workspaces"),
    )?);

    let state = AppState {
        store: store.clone(),
        subscriptions,
        artifacts,
    };

    // Stream broadcaster (store events -> stream/update notifications).
    ws::spawn_stream_broadcaster(state.clone());

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
