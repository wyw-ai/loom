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
    /// actor id -> kind, learned at `bind_actor`. An actor's kind is stable, so
    /// this is keyed by actor (not connection) and never needs per-connection
    /// upkeep. Used to suppress duplicate agent/service wakes.
    actor_kind: HashMap<String, ActorKind>,
}

impl Inner {
    /// True when `conn` is an agent/service worker connection that is NOT the
    /// canonical inbox owner for its actor. Such a connection belongs to a
    /// stale or duplicate runtime; delivering scope wakes to it would make two
    /// workers drive the same actor concurrently (duplicate turns, conflicting
    /// coordination). Human connections are never suppressed — a person may run
    /// several clients that all want live updates.
    fn is_noncanonical_agent_worker(&self, conn: &Connection) -> bool {
        let Some(actor) = conn.actor_id.as_deref() else {
            return false;
        };
        if !matches!(
            self.actor_kind.get(actor),
            Some(ActorKind::Agent) | Some(ActorKind::Service)
        ) {
            return false;
        }
        self.actor_conn
            .get(actor)
            .is_some_and(|owner| owner != &conn.id)
    }
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
    /// becomes the actor-inbox owner depends on `claim_inbox` and
    /// `actor_kind`:
    ///
    /// * `claim_inbox = false` — never take the inbox. Observer connections
    ///   use this to authorize read-only RPCs as an actor without stealing
    ///   runtime delivery.
    ///
    /// * `Agent` / `Service` — always takes over. Long-lived host processes
    ///   (`loom-daemon`, `loom service serve`) claiming an actor means
    ///   "I am the runtime for this actor"; a restart after a crash needs
    ///   to win even if the previous WS hasn't been reaped yet (the old
    ///   conn's TCP close detection on the server side may lag the new
    ///   process's `connection/open` by tens of ms). See
    ///   `docs/service-plugin-system-design.md` §9.4.
    /// * `Human` — only take if no other live connection holds the slot.
    ///   Stops short-lived `loom` subcommands shelled from inside an agent
    ///   or service tool call (which inherit `LOOM_ACTOR` pointing at the
    ///   long-lived actor and dial `connection/open` on every invocation,
    ///   defaulting to `kind = Human`) from yanking the runtime out of
    ///   the routing table when their connection later closes.
    ///
    /// Stale entries (binding points at a conn no longer in `connections`)
    /// are evicted unconditionally, so a fresh host process after a clean
    /// shutdown also takes over.
    pub fn bind_actor(
        &self,
        connection_id: &str,
        actor_id: String,
        actor_kind: ActorKind,
        claim_inbox: bool,
    ) {
        let mut inner = self.inner.write();
        if let Some(c) = inner.connections.get_mut(connection_id) {
            c.actor_id = Some(actor_id.clone());
        }
        // Never downgrade a previously-learned Agent/Service kind to Human.
        // Short-lived CLI subcommands shelled from inside an agent turn default
        // to actor_kind=Human and must not overwrite the stable long-lived kind;
        // doing so would break is_noncanonical_agent_worker / wake suppression.
        if !matches!(
            (inner.actor_kind.get(&actor_id), actor_kind),
            (
                Some(ActorKind::Agent | ActorKind::Service),
                ActorKind::Human
            )
        ) {
            inner.actor_kind.insert(actor_id.clone(), actor_kind);
        }
        if !claim_inbox {
            tracing::debug!(
                actor = %actor_id,
                connection = %connection_id,
                kind = ?actor_kind,
                "actor connection bound as observer; inbox owner unchanged",
            );
            return;
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
            Some(prev) if matches!(actor_kind, ActorKind::Agent | ActorKind::Service) => {
                tracing::info!(
                    actor = %actor_id,
                    prev_conn = %prev,
                    new_conn = %connection_id,
                    kind = ?actor_kind,
                    "long-lived host connection preempting existing actor_conn binding \
                     (agent/service serve restart with old WS still in connections table)",
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

    /// True when `connection_id` is a stale/duplicate agent worker connection
    /// (an agent/service connection that is no longer its actor's canonical
    /// inbox owner). Scope-wake fan-out must skip such connections so only one
    /// runtime drives each agent even when two daemons are connected. Returns
    /// false for unknown connections and for human connections.
    pub fn is_suppressed_wake_target(&self, connection_id: &str) -> bool {
        let inner = self.inner.read();
        inner
            .connections
            .get(connection_id)
            .is_some_and(|c| inner.is_noncanonical_agent_worker(c))
    }

    pub fn connected_actor_ids(&self, actor_ids: &[String]) -> Vec<String> {
        let inner = self.inner.read();
        let mut out = if actor_ids.is_empty() {
            inner
                .actor_conn
                .iter()
                .filter_map(|(actor_id, conn_id)| {
                    inner
                        .connections
                        .contains_key(conn_id)
                        .then(|| actor_id.clone())
                })
                .collect::<Vec<_>>()
        } else {
            actor_ids
                .iter()
                .filter(|actor_id| {
                    inner
                        .actor_conn
                        .get(*actor_id)
                        .is_some_and(|conn_id| inner.connections.contains_key(conn_id))
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        out.sort();
        out.dedup();
        out
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
                if inner.is_noncanonical_agent_worker(c) {
                    continue;
                }
                let _ = c.tx.send(frame.clone());
            }
        }
    }

    /// Send a JSON-RPC notification to every connected client, regardless
    /// of scope subscriptions. Used for global announcements like
    /// `channel.created` for public channels.
    pub fn broadcast_to_all(&self, method: &str, payload: Value) {
        let frame = match serde_json::to_string(&proto::Notification::new(method, Some(payload))) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, "failed to serialize broadcast_to_all notification");
                return;
            }
        };
        let inner = self.inner.read();
        for c in inner.connections.values() {
            let _ = c.tx.send(frame.clone());
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

    /// Send a JSON-RPC notification to every live connection currently bound
    /// to `actor_id`. This is for human-facing inbox events, where multiple
    /// GUI/TUI clients for the same person should all learn about invites and
    /// cross-scope requests. Agent/service delivery must keep using
    /// `send_to_actor` so a restarted runtime's old connection cannot receive
    /// duplicate work.
    pub fn send_to_actor_connections(&self, actor_id: &str, method: &str, payload: Value) -> usize {
        let frame = match serde_json::to_string(&proto::Notification::new(method, Some(payload))) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%e, %method, "failed to serialize actor notification");
                return 0;
            }
        };
        let inner = self.inner.read();
        let mut delivered = 0;
        for c in inner
            .connections
            .values()
            .filter(|c| c.actor_id.as_deref() == Some(actor_id))
        {
            if c.tx.send(frame.clone()).is_ok() {
                delivered += 1;
            } else {
                tracing::warn!(
                    actor = %actor_id,
                    conn = %c.id,
                    %method,
                    "actor-inbox send dropped: writer channel closed (WS dead?)",
                );
            }
        }
        if delivered == 0 {
            tracing::warn!(
                actor = %actor_id,
                %method,
                "actor-inbox send dropped: no connection bound to actor",
            );
        }
        delivered
    }

    #[cfg(test)]
    fn inbox_owner(&self, actor_id: &str) -> Option<String> {
        self.inner.read().actor_conn.get(actor_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn(id: &str) -> Connection {
        let (tx, _rx) = mpsc::unbounded_channel();
        Connection {
            id: id.into(),
            actor_id: None,
            tx,
        }
    }

    #[test]
    fn service_kind_preempts_existing_live_binding() {
        // §9.4: a long-lived `loom service serve` restart must take over the
        // actor-inbox even if the previous WS hasn't been reaped yet — same
        // contract as `loom-daemon`. Without preempt, the new host can't
        // receive any actor-inbox push until the old conn TCP-times out.
        let subs = Subscriptions::new();
        subs.add_connection(make_conn("conn_old"));
        subs.add_connection(make_conn("conn_new"));

        subs.bind_actor("conn_old", "svc_am_bridge".into(), ActorKind::Service, true);
        assert_eq!(
            subs.inbox_owner("svc_am_bridge").as_deref(),
            Some("conn_old")
        );

        subs.bind_actor("conn_new", "svc_am_bridge".into(), ActorKind::Service, true);
        assert_eq!(
            subs.inbox_owner("svc_am_bridge").as_deref(),
            Some("conn_new"),
            "service kind must preempt the previous live binding",
        );
    }

    #[test]
    fn human_bind_does_not_downgrade_agent_kind_for_wake_suppression() {
        // Regression: a short-lived CLI command shelled from inside an agent turn
        // calls bind_actor with actor_kind=Human.  Before the fix this overwrote
        // the agent's actor_kind entry, so is_noncanonical_agent_worker returned
        // false for the stale conn and it started receiving scope-wake messages
        // again (duplicate-runtime risk).
        let subs = Subscriptions::new();
        let (tx_stale, mut rx_stale) = mpsc::unbounded_channel();
        let (tx_live, mut rx_live) = mpsc::unbounded_channel();
        let (tx_shell, _rx_shell) = mpsc::unbounded_channel();

        subs.add_connection(Connection {
            id: "conn_stale".into(),
            actor_id: None,
            tx: tx_stale,
        });
        subs.add_connection(Connection {
            id: "conn_live".into(),
            actor_id: None,
            tx: tx_live,
        });
        subs.add_connection(Connection {
            id: "conn_shell".into(),
            actor_id: None,
            tx: tx_shell,
        });

        // Long-lived daemon binds first, then restarts and the new conn takes over.
        subs.bind_actor("conn_stale", "actor_agent".into(), ActorKind::Agent, true);
        subs.bind_actor("conn_live", "actor_agent".into(), ActorKind::Agent, true);

        // Simulate a CLI subcommand shelled from inside the agent turn (kind=Human).
        subs.bind_actor("conn_shell", "actor_agent".into(), ActorKind::Human, true);

        // conn_shell must not have stolen the inbox.
        assert_eq!(
            subs.inbox_owner("actor_agent").as_deref(),
            Some("conn_live"),
            "Human bind must not preempt the live agent connection",
        );

        // The stale agent worker must still be suppressed even after the Human bind.
        let scope = ScopeRef {
            kind: proto::types::ScopeKind::Channel,
            id: "chan_test".into(),
        };
        for c in ["conn_stale", "conn_live", "conn_shell"] {
            assert!(subs.subscribe(c, scope.clone()));
        }
        subs.broadcast_to_scope(
            &scope,
            "stream/update",
            serde_json::json!({ "kind": "message.created" }),
        );

        assert!(
            rx_live.try_recv().is_ok(),
            "canonical agent worker must be woken"
        );
        assert!(
            rx_stale.try_recv().is_err(),
            "stale agent worker must NOT be woken after a Human re-bind"
        );
    }

    #[test]
    fn human_kind_does_not_preempt_live_binding() {
        // A short-lived `loom --as svc_xxx message send` defaults to actor_kind=Human
        // (see client::open_connection). It must NOT yank the long-lived
        // service host out of the routing table, otherwise its eventual
        // disconnect would leave the actor with no inbox owner at all.
        let subs = Subscriptions::new();
        subs.add_connection(make_conn("conn_host"));
        subs.add_connection(make_conn("conn_shell"));

        subs.bind_actor(
            "conn_host",
            "svc_am_bridge".into(),
            ActorKind::Service,
            true,
        );
        subs.bind_actor("conn_shell", "svc_am_bridge".into(), ActorKind::Human, true);

        assert_eq!(
            subs.inbox_owner("svc_am_bridge").as_deref(),
            Some("conn_host"),
            "human-kind shell must not preempt the long-lived service host",
        );
    }

    #[test]
    fn observer_connection_never_preempts_live_binding() {
        let subs = Subscriptions::new();
        subs.add_connection(make_conn("conn_host"));
        subs.add_connection(make_conn("conn_observer"));

        subs.bind_actor(
            "conn_host",
            "svc_mr_detector".into(),
            ActorKind::Service,
            true,
        );
        subs.bind_actor(
            "conn_observer",
            "svc_mr_detector".into(),
            ActorKind::Service,
            false,
        );

        assert_eq!(
            subs.inbox_owner("svc_mr_detector").as_deref(),
            Some("conn_host"),
            "observer must share actor identity without stealing actor-inbox delivery",
        );
    }

    #[test]
    fn stale_binding_is_evicted_for_any_kind() {
        // The "stale entry" branch fires before kind matching, so even a
        // human-kind connection takes over when the prior binding points at
        // a connection that is no longer in the table.
        let subs = Subscriptions::new();
        subs.add_connection(make_conn("conn_old"));
        subs.bind_actor("conn_old", "svc_am_bridge".into(), ActorKind::Service, true);
        subs.remove_connection("conn_old");

        subs.add_connection(make_conn("conn_new"));
        subs.bind_actor("conn_new", "svc_am_bridge".into(), ActorKind::Human, true);
        assert_eq!(
            subs.inbox_owner("svc_am_bridge").as_deref(),
            Some("conn_new")
        );
    }

    #[test]
    fn actor_connections_sends_to_every_bound_connection() {
        let subs = Subscriptions::new();
        let (tx_a, mut rx_a) = mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();
        let (tx_other, mut rx_other) = mpsc::unbounded_channel();

        subs.add_connection(Connection {
            id: "conn_a".into(),
            actor_id: None,
            tx: tx_a,
        });
        subs.add_connection(Connection {
            id: "conn_b".into(),
            actor_id: None,
            tx: tx_b,
        });
        subs.add_connection(Connection {
            id: "conn_other".into(),
            actor_id: None,
            tx: tx_other,
        });
        subs.bind_actor("conn_a", "actor_alice".into(), ActorKind::Human, true);
        subs.bind_actor("conn_b", "actor_alice".into(), ActorKind::Human, true);
        subs.bind_actor("conn_other", "actor_bob".into(), ActorKind::Human, true);

        let delivered = subs.send_to_actor_connections(
            "actor_alice",
            "stream/update",
            serde_json::json!({ "kind": "channel.invited" }),
        );

        assert_eq!(delivered, 2);
        assert!(rx_a.try_recv().is_ok());
        assert!(rx_b.try_recv().is_ok());
        assert!(rx_other.try_recv().is_err());
    }

    #[test]
    fn scope_wake_skips_noncanonical_agent_workers_but_not_humans() {
        // Two daemons can each hold a worker connection for the same agent
        // actor (e.g. a stale daemon overlapping a fresh one). Only the
        // canonical inbox owner should be woken by a scope broadcast, so the
        // agent is driven by exactly one runtime. Humans, by contrast, may run
        // several clients that all want the live update.
        let subs = Subscriptions::new();
        let (tx_stale, mut rx_stale) = mpsc::unbounded_channel();
        let (tx_live, mut rx_live) = mpsc::unbounded_channel();
        let (tx_human1, mut rx_human1) = mpsc::unbounded_channel();
        let (tx_human2, mut rx_human2) = mpsc::unbounded_channel();

        subs.add_connection(Connection {
            id: "conn_stale".into(),
            actor_id: None,
            tx: tx_stale,
        });
        subs.add_connection(Connection {
            id: "conn_live".into(),
            actor_id: None,
            tx: tx_live,
        });
        subs.add_connection(Connection {
            id: "conn_human1".into(),
            actor_id: None,
            tx: tx_human1,
        });
        subs.add_connection(Connection {
            id: "conn_human2".into(),
            actor_id: None,
            tx: tx_human2,
        });

        // The second agent bind preempts: conn_live becomes canonical, conn_stale
        // keeps the actor identity but is no longer the inbox owner.
        subs.bind_actor("conn_stale", "actor_agent".into(), ActorKind::Agent, true);
        subs.bind_actor("conn_live", "actor_agent".into(), ActorKind::Agent, true);
        // Two human clients of the same person; the second never preempts the
        // inbox but must still receive live updates.
        subs.bind_actor("conn_human1", "actor_human".into(), ActorKind::Human, true);
        subs.bind_actor("conn_human2", "actor_human".into(), ActorKind::Human, true);

        let scope = ScopeRef {
            kind: proto::types::ScopeKind::Channel,
            id: "chan_demo".into(),
        };
        for c in ["conn_stale", "conn_live", "conn_human1", "conn_human2"] {
            assert!(subs.subscribe(c, scope.clone()));
        }

        subs.broadcast_to_scope(
            &scope,
            "stream/update",
            serde_json::json!({ "kind": "message.created" }),
        );

        assert!(
            rx_live.try_recv().is_ok(),
            "canonical agent worker must be woken",
        );
        assert!(
            rx_stale.try_recv().is_err(),
            "stale/duplicate agent worker must NOT be woken",
        );
        assert!(
            rx_human1.try_recv().is_ok(),
            "human client must receive live updates",
        );
        assert!(
            rx_human2.try_recv().is_ok(),
            "a second human client must also receive live updates",
        );
    }
}
