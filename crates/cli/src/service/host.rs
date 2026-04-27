// Public S1 surface; first plugin landed in S2. Silence the
// "unused-yet" warnings until then.
#![allow(dead_code)]

//! `ServiceHost` supervises a fleet of [`ServicePlugin`] instances
//! loaded from on-disk [`ServiceSpec`] files. One spec → one task →
//! one WS connection bound to the spec's actor.
//!
//! Mirrors `crate::cmd::agent_serve` at the structural level: spec
//! loading, per-spec connect, per-spec supervision. Different at the
//! per-spec level: agents use the [`agent-runtime`] adapter to drive a
//! subprocess, services run a custom plugin loop.
//!
//! S1 deliberately ships **no built-in plugins**. The first one (`am`,
//! §7) lands in S2 along with its migration test. The host is still
//! useful in S1 for end-to-end wiring tests: register a no-op plugin,
//! call `serve`, prove the connection lifecycle works.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use proto::methods::ServiceSpec;
use tokio::sync::watch;

use crate::client::Client;

use super::plugin::{ServiceContext, ServicePlugin, ShutdownSignal};
use super::runtime::ServiceRuntime;

pub struct ServiceHost {
    plugins: HashMap<String, Arc<dyn ServicePlugin>>,
    server_url: String,
    data_root: PathBuf,
}

impl ServiceHost {
    pub fn new(server_url: String, data_root: PathBuf) -> Self {
        Self {
            plugins: HashMap::new(),
            server_url,
            data_root,
        }
    }

    /// Register a plugin under its `kind()`. Calling this twice with
    /// the same kind silently overwrites the previous registration —
    /// makes test harnesses easier and the host has no concept of
    /// "second plugin claiming the same kind" yet.
    pub fn register(&mut self, plugin: Arc<dyn ServicePlugin>) {
        self.plugins.insert(plugin.kind().to_string(), plugin);
    }

    /// Run until SIGINT/SIGTERM (or until every plugin task returns).
    /// Each spec runs in its own task; one plugin's failure does not
    /// stop the others (S1: error logged, no restart).
    ///
    /// Specs are validated up front; an invalid spec returns Err
    /// immediately and no tasks start.
    pub async fn serve(self, specs: Vec<ServiceSpec>) -> Result<()> {
        for spec in &specs {
            spec.validate()
                .with_context(|| format!("invalid ServiceSpec `{}`", spec.id))?;
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let _signal_task = {
            let tx = shutdown_tx.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    tracing::info!("ctrl-c received; signaling service host shutdown");
                    let _ = tx.send(true);
                }
            })
        };
        let mut handles = Vec::new();
        for spec in specs {
            let kind = spec.kind.clone();
            let Some(plugin) = self.plugins.get(&kind).cloned() else {
                tracing::warn!(
                    spec_id = %spec.id,
                    %kind,
                    "no plugin registered for kind; skipping spec",
                );
                continue;
            };
            if !spec.autostart {
                tracing::info!(spec_id = %spec.id, "autostart=false; skipping spec");
                continue;
            }
            let server_url = self.server_url.clone();
            let data_root = self.data_root.clone();
            let shutdown_rx = shutdown_rx.clone();
            let spec_id = spec.id.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = run_one_spec(spec, plugin, server_url, data_root, shutdown_rx).await
                {
                    tracing::error!(spec_id = %spec_id, error = ?e, "plugin task failed");
                }
            }));
        }
        for h in handles {
            let _ = h.await;
        }
        Ok(())
    }
}

async fn run_one_spec(
    spec: ServiceSpec,
    plugin: Arc<dyn ServicePlugin>,
    server_url: String,
    data_root: PathBuf,
    shutdown: ShutdownSignal,
) -> Result<()> {
    let actor_id = spec.actor.id.clone();
    let display = spec.actor.display_name.clone();
    let display_opt = if display.is_empty() {
        None
    } else {
        Some(display.as_str())
    };

    // Per §9.4 the second open_connection from a `joi service serve`
    // restart preempts the previous (now-dead) binding — the server's
    // `bind_actor` extends the agent preempt rule to Service kind.
    let client = Client::connect(&server_url)
        .await
        .with_context(|| format!("ws connect {server_url}"))?;
    client.initialize().await.context("rpc initialize")?;
    client
        .open_connection_as(&actor_id, "service", display_opt)
        .await
        .with_context(|| format!("connection/open as {actor_id}"))?;

    let runtime = ServiceRuntime::start(spec.id.clone(), actor_id.clone(), client, &data_root)?;
    runtime
        .actor_upsert(spec.actor.clone())
        .await
        .with_context(|| format!("actor/upsert for {actor_id}"))?;

    plugin
        .run(ServiceContext {
            spec,
            runtime,
            shutdown,
        })
        .await
}
