use std::sync::Arc;

use tokio::sync::Mutex;

use crate::ws::Client;

#[derive(Clone)]
pub(crate) struct CurrentConnection {
    pub generation: u64,
    pub client: Arc<Client>,
}

#[derive(Default)]
pub(crate) struct ConnectionSlot {
    pub generation: u64,
    pub current: Option<CurrentConnection>,
}

/// Shared state the Tauri app owns for the lifetime of the process.
///
/// `inner` is `None` before `connect` succeeds. Every IPC command that hits
/// the server goes through `with_client` so we surface a single "not connected"
/// error shape to the front-end instead of each command inventing its own.
pub struct AppState {
    pub(crate) inner: Arc<Mutex<ConnectionSlot>>,
    /// Serializes connect/disconnect IPC commands. Without this gate, two
    /// overlapping reconnect attempts could install their clients out of
    /// order, or a late connect could undo an explicit disconnect.
    pub connection_gate: Arc<Mutex<()>>,
    /// Serializes read-modify-write access to the desktop profile. A connect
    /// attempt may spend tens of seconds on the network, so it must never
    /// publish the stale profile snapshot it started with over a newer account
    /// or workspace edit.
    pub config_gate: Arc<Mutex<()>>,
}

impl AppState {
    pub async fn client(&self) -> Result<Arc<Client>, String> {
        let guard = self.inner.lock().await;
        guard
            .current
            .as_ref()
            .map(|current| Arc::clone(&current.client))
            .ok_or_else(|| "not connected — call connect() first".to_string())
    }

    pub async fn try_client(&self) -> Option<Arc<Client>> {
        self.inner
            .lock()
            .await
            .current
            .as_ref()
            .map(|current| Arc::clone(&current.client))
    }

    /// Invalidates the current connection and reserves a new generation for a
    /// candidate connection. Late events and late candidates from every older
    /// generation can then be rejected deterministically.
    pub async fn begin_transition(&self) -> u64 {
        let mut slot = self.inner.lock().await;
        slot.generation = slot.generation.wrapping_add(1).max(1);
        slot.current = None;
        slot.generation
    }

    pub async fn invalidate(&self) -> u64 {
        self.begin_transition().await
    }

    pub async fn publish(&self, generation: u64, client: Arc<Client>) -> bool {
        let mut slot = self.inner.lock().await;
        if slot.generation != generation || slot.current.is_some() {
            return false;
        }
        slot.current = Some(CurrentConnection { generation, client });
        true
    }

    #[cfg(test)]
    pub(crate) async fn generation(&self) -> u64 {
        self.inner.lock().await.generation
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ConnectionSlot::default())),
            connection_gate: Arc::new(Mutex::new(())),
            config_gate: Arc::new(Mutex::new(())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_transition_advances_generation_and_invalidates_current_slot() {
        let state = AppState::default();
        let first = state.begin_transition().await;
        let second = state.begin_transition().await;
        assert!(second > first);
        assert_eq!(state.generation().await, second);
        assert!(state.try_client().await.is_none());
    }
}
