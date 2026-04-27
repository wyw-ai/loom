use std::sync::Arc;

use tokio::sync::Mutex;

use crate::ws::Client;

/// Shared state the Tauri app owns for the lifetime of the process.
///
/// `inner` is `None` before `connect` succeeds. Every IPC command that hits
/// the server goes through `with_client` so we surface a single "not connected"
/// error shape to the front-end instead of each command inventing its own.
pub struct AppState {
    pub inner: Arc<Mutex<Option<Arc<Client>>>>,
}

impl AppState {
    pub async fn client(&self) -> Result<Arc<Client>, String> {
        let guard = self.inner.lock().await;
        guard
            .clone()
            .ok_or_else(|| "not connected — call connect() first".to_string())
    }

    pub async fn set(&self, client: Option<Arc<Client>>) {
        *self.inner.lock().await = client;
    }
}
