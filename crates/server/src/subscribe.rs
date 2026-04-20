use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;
use proto::types::ScopeRef;
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct Connection {
    pub id: String,
    pub actor_id: Option<String>,
    pub tx: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct Inner {
    connections: HashMap<String, Connection>,
    /// scope -> set of connection IDs subscribed
    subs: HashMap<ScopeRef, HashSet<String>>,
    /// connection id -> set of scopes
    conn_scopes: HashMap<String, HashSet<ScopeRef>>,
    /// actor id -> connection id (last one wins; v0 is single-connection-per-actor)
    actor_conn: HashMap<String, String>,
}

#[derive(Default)]
pub struct Subscriptions {
    inner: RwLock<Inner>,
}

impl Subscriptions {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(Inner::default()),
        })
    }

    pub fn add_connection(&self, conn: Connection) {
        let mut inner = self.inner.write();
        if let Some(actor) = &conn.actor_id {
            inner.actor_conn.insert(actor.clone(), conn.id.clone());
        }
        inner.connections.insert(conn.id.clone(), conn);
    }

    pub fn bind_actor(&self, connection_id: &str, actor_id: String) {
        let mut inner = self.inner.write();
        if let Some(c) = inner.connections.get_mut(connection_id) {
            c.actor_id = Some(actor_id.clone());
        }
        inner.actor_conn.insert(actor_id, connection_id.into());
    }

    pub fn remove_connection(&self, connection_id: &str) {
        let mut inner = self.inner.write();
        if let Some(c) = inner.connections.remove(connection_id) {
            if let Some(actor) = &c.actor_id {
                if inner.actor_conn.get(actor).map(|s| s.as_str()) == Some(connection_id) {
                    inner.actor_conn.remove(actor);
                }
            }
        }
        if let Some(scopes) = inner.conn_scopes.remove(connection_id) {
            for s in scopes {
                if let Some(set) = inner.subs.get_mut(&s) {
                    set.remove(connection_id);
                }
            }
        }
    }

    pub fn subscribe(&self, connection_id: &str, scope: ScopeRef) -> bool {
        let mut inner = self.inner.write();
        if !inner.connections.contains_key(connection_id) {
            return false;
        }
        inner
            .subs
            .entry(scope.clone())
            .or_default()
            .insert(connection_id.into());
        inner
            .conn_scopes
            .entry(connection_id.into())
            .or_default()
            .insert(scope);
        true
    }

    pub fn unsubscribe(&self, connection_id: &str, scope: &ScopeRef) -> bool {
        let mut inner = self.inner.write();
        let removed_a = inner
            .subs
            .get_mut(scope)
            .map(|set| set.remove(connection_id))
            .unwrap_or(false);
        let removed_b = inner
            .conn_scopes
            .get_mut(connection_id)
            .map(|set| set.remove(scope))
            .unwrap_or(false);
        removed_a || removed_b
    }

    pub fn connection_for_actor(&self, actor_id: &str) -> Option<Connection> {
        let inner = self.inner.read();
        inner
            .actor_conn
            .get(actor_id)
            .and_then(|id| inner.connections.get(id))
            .cloned()
    }

    pub fn actor_for_connection(&self, connection_id: &str) -> Option<String> {
        let inner = self.inner.read();
        inner
            .connections
            .get(connection_id)
            .and_then(|c| c.actor_id.clone())
    }

    /// Send a JSON-RPC notification to all connections subscribed to scope.
    pub fn broadcast_to_scope(&self, scope: &ScopeRef, method: &str, payload: Value) {
        let frame = match serde_json::to_string(&proto::Notification::new(method, Some(payload))) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, "failed to serialize stream/update notification");
                return;
            }
        };
        let inner = self.inner.read();
        let Some(set) = inner.subs.get(scope) else {
            return;
        };
        for id in set {
            if let Some(c) = inner.connections.get(id) {
                let _ = c.tx.send(frame.clone());
            }
        }
    }

    /// Send a JSON-RPC notification (or any text frame) to a single connection.
    pub fn send_to_connection(&self, connection_id: &str, frame: String) -> bool {
        let inner = self.inner.read();
        if let Some(c) = inner.connections.get(connection_id) {
            c.tx.send(frame).is_ok()
        } else {
            false
        }
    }
}
