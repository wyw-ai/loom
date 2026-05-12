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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use proto::methods::{ServiceLifecycle, ServiceSpec};
use tokio::sync::watch;
use tokio::task::JoinHandle;
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
            let server_url = self.server_url.clone();
            let data_root = self.data_root.clone();
            let shutdown_rx = shutdown_rx.clone();
            let spec_id = spec.id.clone();
            let specs_dir = self.specs_dir.clone();
            match spec.lifecycle {
                ServiceLifecycle::ChannelSingleton => {
                    if !spec.autostart {
                        tracing::info!(
                            spec_id = %spec.id,
                            "autostart=false; skipping channel-singleton spec",
                        );
                        continue;
                    }
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
                ServiceLifecycle::ThreadBound => {
                    // Thread-bound specs ignore `autostart` at the
                    // host level — the watcher always runs so that a
                    // later `joi service start --in <thread>` writes
                    // a request file and is picked up. `autostart`
                    // semantics for thread-bound are reserved for a
                    // future "auto-spawn one instance per existing
                    // thread" mode.
                    handles.push(tokio::spawn(async move {
                        if let Err(e) = supervise_instances(
                            spec,
                            plugin,
                            server_url,
                            data_root,
                            shutdown_rx,
                            specs_dir,
                        )
                        .await
                        {
                            tracing::error!(
                                spec_id = %spec_id,
                                error = ?e,
                                "instance watcher failed",
                            );
                        }
                    }));
                }
            }
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
        return run_one_spec(initial_spec, plugin, server_url, data_root, shutdown, None).await;
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
        let spec_path = resolve_spec_path(Some(&dir), &spec_id);
        let worker = tokio::spawn(async move {
            run_one_spec(
                spec_for_run,
                plugin_task,
                server_clone,
                data_clone,
                shutdown_clone,
                spec_path,
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
    let nested = dir.join(spec_id).join("spec.json");
    let flat = dir.join(format!("{spec_id}.json"));
    let path = if nested.exists() {
        nested
    } else if flat.exists() {
        flat
    } else {
        return Ok(None);
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read spec {}", path.display()))?;
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

/// Resolve a spec's source path under `specs_dir`. Tries the legacy
/// flat layout `<dir>/<id>.json` first, then the recursive layout
/// `<dir>/<id>/spec.json`. Returns `None` when neither exists or
/// `specs_dir` itself is `None`.
fn resolve_spec_path(specs_dir: Option<&Path>, spec_id: &str) -> Option<PathBuf> {
    let dir = specs_dir?;
    let flat = dir.join(format!("{spec_id}.json"));
    if flat.exists() {
        return Some(flat);
    }
    let nested = dir.join(spec_id).join("spec.json");
    if nested.exists() {
        return Some(nested);
    }
    None
}

async fn run_one_spec(
    spec: ServiceSpec,
    plugin: Arc<dyn ServicePlugin>,
    server_url: String,
    data_root: PathBuf,
    shutdown: ShutdownSignal,
    spec_path: Option<PathBuf>,
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
            spec_path,
            runtime,
            shutdown,
            instance: None,
        })
        .await
}

/// Per-instance plugin task for a `lifecycle = thread_bound` spec.
/// Mirrors [`run_one_spec`] but binds the [`ServiceRuntime`] to a
/// thread-scoped state dir and carries the [`InstanceRequest`] into
/// the plugin's [`ServiceContext`].
async fn run_one_instance(
    spec: ServiceSpec,
    plugin: Arc<dyn ServicePlugin>,
    server_url: String,
    data_root: PathBuf,
    shutdown: ShutdownSignal,
    request: super::instance::InstanceRequest,
    spec_path: Option<PathBuf>,
) -> Result<()> {
    let actor_id = spec.actor.id.clone();
    let instance_id = request.scope.id.clone();
    let display = spec.actor.display_name.clone();
    let display_opt = if display.is_empty() {
        None
    } else {
        Some(display.as_str())
    };

    let client = Client::connect(&server_url)
        .await
        .with_context(|| format!("ws connect {server_url}"))?;
    client.initialize().await.context("rpc initialize")?;
    client
        .open_connection_as(&actor_id, "service", display_opt)
        .await
        .with_context(|| format!("connection/open as {actor_id}"))?;

    let runtime = ServiceRuntime::start_instance(
        spec.id.clone(),
        actor_id.clone(),
        instance_id,
        client,
        &data_root,
    )?;
    runtime
        .actor_upsert(spec.actor.clone())
        .await
        .with_context(|| format!("actor/upsert for {actor_id}"))?;

    plugin
        .run(ServiceContext {
            spec,
            spec_path,
            runtime,
            shutdown,
            instance: Some(request),
        })
        .await
}

/// Watcher loop for `lifecycle = thread_bound` specs (§4.7.3).
///
/// Polls `<data_root>/services/<spec.id>/instances/` once per second:
///
/// * For each `<thread_id>/request.json` not in `active`, reads the
///   request and spawns [`run_one_instance`] with a [`JoinHandle`]
///   stored under the thread id.
/// * For each `active` entry whose plugin task has **finished**
///   (e.g. the scheduler observed `service.self_complete` and
///   self-aborted, or the plugin returned naturally), reaps the
///   instance: deletes the `request.json` if still present and drops
///   it from `active`.
/// * For each `active` entry whose request file has disappeared
///   without the task finishing (`joi service stop` removed it
///   externally), aborts the task and drops it.
/// * **If `bind.auto_stop_on` contains `"thread.closed"`**: each
///   tick lists threads visible to the spec's service actor; any
///   active instance whose `instance_id` (= bound thread id) is no
///   longer in that list is treated as closed → reap (delete
///   request.json + drop the join handle), per §4.7.3.
/// * On `shutdown` notification, aborts every active task and exits.
///
/// The watcher itself is panic-free: per-instance failures are
/// logged and do not unwind the loop. Returning `Ok(())` is the only
/// non-panic outcome.
async fn supervise_instances(
    spec: ServiceSpec,
    plugin: Arc<dyn ServicePlugin>,
    server_url: String,
    data_root: PathBuf,
    mut shutdown: ShutdownSignal,
    specs_dir: Option<PathBuf>,
) -> Result<()> {
    let mut active: HashMap<String, JoinHandle<()>> = HashMap::new();
    let spec_id = spec.id.clone();
    let actor_id = spec.actor.id.clone();
    let watch_thread_closed = spec
        .bind
        .as_ref()
        .map(|b| b.auto_stop_on.iter().any(|e| e == "thread.closed"))
        .unwrap_or(false);

    // Long-lived client used only for thread/list visibility checks
    // when auto_stop_on contains "thread.closed". Lazy-initialised so
    // specs without that opt-in don't pay the connection cost.
    let mut visibility: Option<Arc<crate::client::Client>> = None;

    tracing::info!(
        spec_id = %spec_id,
        watch_thread_closed,
        "thread-bound instance watcher started"
    );

    loop {
        if *shutdown.borrow() {
            break;
        }

        // 1. Reap instances whose plugin task has finished naturally
        //    (scheduler self_complete, plugin returned). Delete
        //    request.json so subsequent ticks treat the instance as
        //    stopped and don't re-spawn it.
        let to_reap: Vec<String> = active
            .iter()
            .filter(|(_, h)| h.is_finished())
            .map(|(k, _)| k.clone())
            .collect();
        for instance_id in to_reap {
            active.remove(&instance_id);
            match super::instance::delete_request(&data_root, &spec_id, &instance_id) {
                Ok(true) => tracing::info!(
                    spec_id = %spec_id,
                    instance_id = %instance_id,
                    "instance task finished; reaped request.json",
                ),
                Ok(false) => tracing::debug!(
                    spec_id = %spec_id,
                    instance_id = %instance_id,
                    "instance task finished; request.json already gone",
                ),
                Err(e) => tracing::warn!(
                    spec_id = %spec_id,
                    instance_id = %instance_id,
                    error = ?e,
                    "instance task finished but request.json delete failed",
                ),
            }
        }

        // 1b. If `auto_stop_on` contains `thread.closed`, ask the
        //     server which threads still exist and reap any active
        //     instance whose bound thread id is no longer visible.
        //     Skips silently on transient RPC errors so the watcher
        //     stays panic-free.
        if watch_thread_closed && !active.is_empty() {
            let visible =
                match thread_visibility_check(&mut visibility, &server_url, &actor_id, &spec_id)
                    .await
                {
                    Ok(v) => Some(v),
                    Err(e) => {
                        tracing::warn!(
                            spec_id = %spec_id,
                            error = ?e,
                            "thread visibility check failed; deferring auto_stop_on=thread.closed",
                        );
                        None
                    }
                };
            if let Some(visible_set) = visible {
                let closed: Vec<String> = active
                    .keys()
                    .filter(|k| !visible_set.contains(*k))
                    .cloned()
                    .collect();
                for instance_id in closed {
                    if let Some(handle) = active.remove(&instance_id) {
                        tracing::info!(
                            spec_id = %spec_id,
                            instance_id = %instance_id,
                            "bound thread no longer visible; reaping (auto_stop_on=thread.closed)",
                        );
                        handle.abort();
                        match super::instance::delete_request(&data_root, &spec_id, &instance_id) {
                            Ok(_) => {}
                            Err(e) => tracing::warn!(
                                spec_id = %spec_id,
                                instance_id = %instance_id,
                                error = ?e,
                                "delete_request after thread.closed failed",
                            ),
                        }
                    }
                }
            }
        }

        // Diff filesystem state vs in-memory active map.
        let listed = match super::instance::list_instances(&data_root, &spec_id) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(spec_id = %spec_id, error = ?e, "list_instances failed");
                Vec::new()
            }
        };
        let listed_set: HashSet<String> = listed.iter().cloned().collect();

        // 2. Stop instances whose request files have been removed
        //    externally (`joi service stop`).
        let to_drop: Vec<String> = active
            .keys()
            .filter(|k| !listed_set.contains(*k))
            .cloned()
            .collect();
        for instance_id in to_drop {
            if let Some(handle) = active.remove(&instance_id) {
                tracing::info!(
                    spec_id = %spec_id,
                    instance_id = %instance_id,
                    "request file gone; aborting instance task",
                );
                handle.abort();
            }
        }

        // Spawn instances that appeared since the last tick.
        for instance_id in listed {
            if active.contains_key(&instance_id) {
                continue;
            }
            let request = match super::instance::read_request(&data_root, &spec_id, &instance_id) {
                Ok(Some(r)) => r,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(
                        spec_id = %spec_id,
                        instance_id = %instance_id,
                        error = ?e,
                        "read_request failed; skipping",
                    );
                    continue;
                }
            };
            tracing::info!(
                spec_id = %spec_id,
                instance_id = %instance_id,
                "spawning instance task",
            );
            let plugin_c = plugin.clone();
            let server_c = server_url.clone();
            let data_c = data_root.clone();
            let shutdown_c = shutdown.clone();
            let spec_c = spec.clone();
            let inst_for_log = instance_id.clone();
            let spec_for_log = spec_id.clone();
            let spec_path_c = resolve_spec_path(specs_dir.as_deref(), &spec_id);
            let join = tokio::spawn(async move {
                if let Err(e) = run_one_instance(
                    spec_c,
                    plugin_c,
                    server_c,
                    data_c,
                    shutdown_c,
                    request,
                    spec_path_c,
                )
                .await
                {
                    tracing::error!(
                        spec_id = %spec_for_log,
                        instance_id = %inst_for_log,
                        error = ?e,
                        "instance task failed",
                    );
                }
            });
            active.insert(instance_id, join);
        }

        // Wait either for next tick or shutdown.
        tokio::select! {
            _ = sleep(Duration::from_millis(1000)) => {}
            _ = shutdown.changed() => {}
        }
    }

    tracing::info!(spec_id = %spec_id, "thread-bound watcher shutting down");
    for (_, handle) in active.drain() {
        handle.abort();
    }
    Ok(())
}

/// Open (lazily) a watcher-side WS client and ask the server which
/// threads the spec's service actor can see. Returns the set of
/// thread ids; the caller treats any active instance whose
/// `instance_id` is missing from this set as "thread closed".
///
/// On the first call, opens a fresh connection as `actor_id` and
/// caches it in `slot`. Subsequent calls reuse the cached client. If
/// the cached client has gone bad (e.g. server restarted), the caller
/// sees the error, logs it, and we drop the slot so the next tick
/// reconnects.
async fn thread_visibility_check(
    slot: &mut Option<Arc<crate::client::Client>>,
    server_url: &str,
    actor_id: &str,
    spec_id: &str,
) -> Result<HashSet<String>> {
    use proto::methods::{method, ThreadListResult};
    use serde_json::json;

    if slot.is_none() {
        let client = crate::client::Client::connect(server_url)
            .await
            .with_context(|| format!("watcher ws connect {server_url}"))?;
        client
            .open_connection_as(actor_id, "service", None)
            .await
            .with_context(|| format!("watcher connection/open as {actor_id}"))?;
        *slot = Some(client);
    }
    let client = slot.as_ref().expect("just initialised");

    let res: ThreadListResult = match client.call(method::THREAD_LIST, json!({})).await {
        Ok(v) => v,
        Err(e) => {
            // Drop the (possibly broken) client so next tick reconnects.
            *slot = None;
            return Err(anyhow::anyhow!(
                "thread/list rpc failed for spec `{spec_id}`: {e}"
            ));
        }
    };
    Ok(res.threads.into_iter().map(|t| t.id).collect())
}
/// Pure helper: given the set of currently-listed instance ids and
/// the currently-active set, return `(to_spawn, to_drop)`. Extracted
/// so the watcher's diff logic is unit-testable without a tokio
/// runtime or filesystem.
#[cfg(test)]
fn diff_instances(listed: &[String], active: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let listed_set: HashSet<&str> = listed.iter().map(String::as_str).collect();
    let to_spawn: Vec<String> = listed
        .iter()
        .filter(|id| !active.contains(id.as_str()))
        .cloned()
        .collect();
    let to_drop: Vec<String> = active
        .iter()
        .filter(|id| !listed_set.contains(id.as_str()))
        .cloned()
        .collect();
    (to_spawn, to_drop)
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

    #[test]
    fn diff_instances_spawns_new_and_drops_gone() {
        let listed = vec!["t1".to_string(), "t2".to_string(), "t3".to_string()];
        let mut active: HashSet<String> = HashSet::new();
        active.insert("t2".to_string());
        active.insert("t9".to_string());

        let (mut to_spawn, mut to_drop) = diff_instances(&listed, &active);
        to_spawn.sort();
        to_drop.sort();

        assert_eq!(to_spawn, vec!["t1".to_string(), "t3".to_string()]);
        assert_eq!(to_drop, vec!["t9".to_string()]);
    }

    #[test]
    fn diff_instances_no_change_returns_empty() {
        let listed = vec!["t1".to_string(), "t2".to_string()];
        let mut active: HashSet<String> = HashSet::new();
        active.insert("t1".to_string());
        active.insert("t2".to_string());

        let (to_spawn, to_drop) = diff_instances(&listed, &active);
        assert!(to_spawn.is_empty());
        assert!(to_drop.is_empty());
    }
}
