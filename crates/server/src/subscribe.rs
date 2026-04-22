use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::RwLock;
use proto::types::{ActorKind, ScopeRef};
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

    /// Bind this connection to `actor_id`. The connection always learns its
    /// actor identity (so it can send events as that actor); whether it also
    /// becomes the actor-inbox owner depends on `actor_kind`:
    ///
    /// * `Agent` — always takes over. `joi agent serve` claiming an actor
    ///   means "I am the runtime for this actor"; a restart after a crash
    ///   needs to win even if the previous WS hasn't been reaped yet (the
    ///   old conn's TCP close detection on the server side may lag the new
    ///   process's `connection/open` by tens of ms).
    /// * `Human` / `Service` — only take if no other live connection holds
    ///   the slot. Stops short-lived `joi` subcommands shelled from inside
    ///   an agent's tool call (which inherit `JOI_ACTOR` pointing at the
    ///   *agent*'s actor and dial `connection/open` on every invocation)
    ///   from yanking the long-lived agent runtime out of the routing
    ///   table when their connection later closes.
    ///
    /// Stale entries (binding points at a conn no longer in `connections`)
    /// are evicted unconditionally, so a fresh `agent serve` after a clean
    /// shutdown also takes over.
    pub fn bind_actor(&self, connection_id: &str, actor_id: String, actor_kind: ActorKind) {
        let mut inner = self.inner.write();
        if let Some(c) = inner.connections.get_mut(connection_id) {
            c.actor_id = Some(actor_id.clone());
        }
        let take_inbox = match inner.actor_conn.get(&actor_id) {
            None => true,
            Some(prev) if prev == connection_id => true,
            Some(prev) if !inner.connections.contains_key(prev) => {
                tracing::info!(
                    actor = %actor_id,
                    stale_conn = %prev,
                    new_conn = %connection_id,
                    "actor_conn taking over stale binding",
                );
                true
            }
            Some(prev) if matches!(actor_kind, ActorKind::Agent) => {
                tracing::info!(
                    actor = %actor_id,
                    prev_conn = %prev,
                    new_conn = %connection_id,
                    "agent connection preempting existing actor_conn binding (likely \
                     `agent serve` restart with old WS still in connections table)",
                );
                true
            }
            Some(prev) => {
                tracing::debug!(
                    actor = %actor_id,
                    holder = %prev,
                    secondary = %connection_id,
                    kind = ?actor_kind,
                    "actor_conn already owned; new connection shares identity but not inbox",
                );
                false
            }
        };
        if take_inbox {
            inner.actor_conn.insert(actor_id, connection_id.into());
        }
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

    pub fn actor_for_connection(&self, connection_id: &str) -> Option<String> {
        let inner = self.inner.read();
        inner
            .connections
            .get(connection_id)
            .and_then(|c| c.actor_id.clone())
    }

    /// Snapshot the set of connection ids currently subscribed to `scope`.
    /// Returned as a `Vec<String>` (not borrowed) so the caller can drop
    /// the read lock before doing per-connection work like ACL filtering.
    pub fn scope_subscribers(&self, scope: &ScopeRef) -> Vec<String> {
        let inner = self.inner.read();
        inner
            .subs
            .get(scope)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
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

    /// Send a JSON-RPC notification to the (at most one) connection currently
    /// bound to `actor_id`. Used for owner-only delivery of turn-private
    /// trace frames. Returns false if no connection is bound or the send
    /// channel is closed.
    pub fn send_to_actor(&self, actor_id: &str, method: &str, payload: Value) -> bool {
        let frame = match serde_json::to_string(&proto::Notification::new(method, Some(payload))) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, %method, "failed to serialize actor notification");
                return false;
            }
        };
        let inner = self.inner.read();
        let Some(conn_id) = inner.actor_conn.get(actor_id) else {
            tracing::warn!(
                actor = %actor_id,
                %method,
                "actor-inbox send dropped: no connection bound to actor",
            );
            return false;
        };
        let Some(c) = inner.connections.get(conn_id) else {
            tracing::warn!(
                actor = %actor_id,
                conn = %conn_id,
                %method,
                "actor-inbox send dropped: bound connection vanished from registry",
            );
            return false;
        };
        if c.tx.send(frame).is_err() {
            tracing::warn!(
                actor = %actor_id,
                conn = %conn_id,
                %method,
                "actor-inbox send dropped: writer channel closed (WS dead?)",
            );
            return false;
        }
        true
    }
}
