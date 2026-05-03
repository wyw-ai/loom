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
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use proto::methods::ServiceSpec;
use tokio::sync::watch;
use tokio::time::sleep;

use crate::client::Client;
use crate::cmd::reload;

use super::plugin::{ServiceContext, ServicePlugin, ShutdownSignal};
use super::runtime::ServiceRuntime;

const RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(1000);

pub struct ServiceHost {
    plugins: HashMap<String, Arc<dyn ServicePlugin>>,
    server_url: String,
    data_root: PathBuf,
    /// When set, per-spec supervisors poll
    /// `<data_root>/services/<id>/reload-epoch.json` and re-load the
    /// spec from this directory whenever the epoch advances. `None`
    /// preserves the legacy "load once, no reload" behavior used by
    /// existing tests.
    specs_dir: Option<PathBuf>,
}

impl ServiceHost {
    pub fn new(server_url: String, data_root: PathBuf) -> Self {
        Self {
            plugins: HashMap::new(),
            server_url,
            data_root,
            specs_dir: None,
        }
    }

    /// Enable hot-reload by telling the host where the on-disk spec
    /// files live. After this, a `joi service reload <id>` bump will
    /// cause the host to re-read `<dir>/<id>.json`, normalize+validate,
    /// abort the running plugin task, and respawn it with the fresh
    /// spec. See design §7.1.
    pub fn with_specs_dir(mut self, dir: PathBuf) -> Self {
        self.specs_dir = Some(dir);
        self
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
            let specs_dir = self.specs_dir.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = supervise_spec(
                    spec,
                    plugin,
                    server_url,
                    data_root,
                    shutdown_rx,
                    specs_dir,
                )
                .await
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

/// Supervisor wrapper around [`run_one_spec`]. When `specs_dir` is
/// `Some`, polls the per-service reload marker; on bump aborts the
/// inner task, re-reads the spec, and respawns. When `None`, runs the
/// inner task exactly once.
async fn supervise_spec(
    initial_spec: ServiceSpec,
    plugin: Arc<dyn ServicePlugin>,
    server_url: String,
    data_root: PathBuf,
    shutdown: ShutdownSignal,
    specs_dir: Option<PathBuf>,
) -> Result<()> {
    let Some(dir) = specs_dir else {
        return run_one_spec(initial_spec, plugin, server_url, data_root, shutdown).await;
    };

    let spec_id = initial_spec.id.clone();
    let marker = reload::service_marker_path(&data_root, &spec_id);
    let mut current_spec = initial_spec;

    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        let baseline_epoch = reload::read_epoch(&marker);

        let plugin_task = plugin.clone();
        let server_clone = server_url.clone();
        let data_clone = data_root.clone();
        let shutdown_clone = shutdown.clone();
        let spec_for_run = current_spec.clone();
        let worker = tokio::spawn(async move {
            run_one_spec(
                spec_for_run,
                plugin_task,
                server_clone,
                data_clone,
                shutdown_clone,
            )
            .await
        });

        let watcher_marker = marker.clone();
        let worker_abort = worker.abort_handle();
        let spec_for_watch = spec_id.clone();
        let watcher = tokio::spawn(async move {
            loop {
                sleep(RELOAD_POLL_INTERVAL).await;
                let cur = reload::read_epoch(&watcher_marker);
                if cur > baseline_epoch {
                    tracing::info!(
                        spec_id = %spec_for_watch,
                        epoch_ms = cur,
                        "reload requested; aborting plugin task",
                    );
                    worker_abort.abort();
                    return;
                }
            }
        });

        let join_result = worker.await;
        watcher.abort();
        let _ = watcher.await;

        match join_result {
            Ok(Ok(())) => {
                // Plugin returned cleanly (shutdown or self-stop). No
                // restart.
                return Ok(());
            }
            Ok(Err(e)) => {
                // Plugin reported an error. Surface it; supervisor
                // does not auto-restart on plugin error in S1.
                return Err(e);
            }
            Err(join_err) => {
                if !join_err.is_cancelled() {
                    return Err(anyhow::anyhow!(
                        "plugin task panicked for {spec_id}: {join_err}"
                    ));
                }
                // Cancelled by the reload watcher — re-read the spec
                // before respawning. On parse error, log and fall back
                // to the last good spec so a typo doesn't silently
                // drop a running plugin.
                match reload_one_spec(&dir, &spec_id) {
                    Ok(Some(s)) => current_spec = s,
                    Ok(None) => tracing::warn!(
                        spec_id = %spec_id,
                        dir = %dir.display(),
                        "spec file missing on reload; reusing last-known spec",
                    ),
                    Err(e) => tracing::warn!(
                        spec_id = %spec_id,
                        error = %e,
                        "failed to re-read spec on reload; reusing last-known spec",
                    ),
                }
            }
        }
    }
}

/// Re-read `<dir>/<spec_id>.json`, normalize and validate. Returns
/// `Ok(None)` if the file is absent (operator deleted the spec —
/// supervisor keeps running the cached one until host restart).
fn reload_one_spec(dir: &Path, spec_id: &str) -> Result<Option<ServiceSpec>> {
    let path = dir.join(format!("{spec_id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read spec {}", path.display()))?;
    let mut spec: ServiceSpec = serde_json::from_str(&text)
        .with_context(|| format!("parse ServiceSpec {}", path.display()))?;
    spec.normalize();
    spec.validate()
        .with_context(|| format!("validate ServiceSpec {}", path.display()))?;
    if spec.id != spec_id {
        anyhow::bail!(
            "spec id mismatch: file {} declares id `{}`, expected `{}`",
            path.display(),
            spec.id,
            spec_id
        );
    }
    Ok(Some(spec))
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

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::{ServiceLifecycle, ServiceSpec};
    use proto::types::{Actor, ActorKind};

    fn temp_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "joi-host-tests-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn minimal_spec(id: &str) -> ServiceSpec {
        ServiceSpec {
            id: id.to_string(),
            kind: "scheduler".to_string(),
            actor: Actor {
                id: format!("svc:{id}"),
                kind: ActorKind::Service,
                display_name: format!("svc {id}"),
                capabilities: None,
                _meta: None,
            },
            autostart: true,
            channel_id: None,
            target_agent: None,
            lifecycle: ServiceLifecycle::ChannelSingleton,
            bind: None,
            params_schema: None,
            config: serde_json::json!({}),
        }
    }

    fn write_spec(dir: &Path, spec: &ServiceSpec) {
        let path = dir.join(format!("{}.json", spec.id));
        let body = serde_json::to_string_pretty(spec).unwrap();
        std::fs::write(&path, body).unwrap();
    }

    #[test]
    fn reload_one_spec_reads_and_normalizes() {
        let dir = temp_dir("reload-ok");
        let mut spec = minimal_spec("svc-a");
        spec.config = serde_json::json!({
            "lifecycle": "thread_bound",
            "bind": { "scope": "thread", "auto_stop_on": ["service.self_complete"] },
        });
        spec.lifecycle = ServiceLifecycle::ChannelSingleton;
        spec.bind = None;
        write_spec(&dir, &spec);

        let reloaded = reload_one_spec(&dir, "svc-a").unwrap().unwrap();
        assert!(matches!(reloaded.lifecycle, ServiceLifecycle::ThreadBound));
        let bind = reloaded.bind.as_ref().expect("bind hoisted");
        assert_eq!(bind.scope, "thread");
        assert!(bind
            .auto_stop_on
            .iter()
            .any(|s| s == "service.self_complete"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reload_one_spec_missing_returns_none() {
        let dir = temp_dir("reload-missing");
        assert!(reload_one_spec(&dir, "ghost").unwrap().is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reload_one_spec_rejects_id_mismatch() {
        let dir = temp_dir("reload-mismatch");
        let spec = minimal_spec("declared-id");
        let path = dir.join("expected-id.json");
        std::fs::write(&path, serde_json::to_string(&spec).unwrap()).unwrap();
        let err = reload_one_spec(&dir, "expected-id").unwrap_err();
        assert!(err.to_string().contains("spec id mismatch"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reload_one_spec_invalid_json_errors() {
        let dir = temp_dir("reload-bad");
        std::fs::write(dir.join("svc-x.json"), "{ not json").unwrap();
        let err = reload_one_spec(&dir, "svc-x").unwrap_err();
        assert!(
            err.to_string().contains("parse ServiceSpec")
                || format!("{err:?}").contains("expected"),
            "unexpected error: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
