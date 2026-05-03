//! `joi service serve` and `joi service validate` — Stage 4 entry points
//! for the §6 service host. The first wires `ServiceHost` to spec files
//! on disk; the second is a one-shot validator for ops to sanity-check a
//! spec before deploying.
//!
//! Mirrors `crate::cmd::agent_serve` at the structural level (spec
//! loader + `--allow-*` filter + serve loop) but is much smaller because
//! the heavy lifting lives in `crate::service`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::methods::ServiceSpec;

use std::sync::Arc;

use crate::service::scheduler::SchedulerPlugin;
use crate::service::{state, ServiceHost};

pub(crate) fn default_specs_dir() -> PathBuf {
    if let Ok(s) = std::env::var("JOI_SERVICE_SPECS") {
        return PathBuf::from(s);
    }
    dirs::config_dir()
        .map(|p| p.join("joi").join("services"))
        .unwrap_or_else(|| {
            PathBuf::from(".")
                .join(".config")
                .join("joi")
                .join("services")
        })
}

/// Load every ServiceSpec under `dir`. Accepts two layouts:
///
/// * **Flat** — `<dir>/<id>.json` (legacy; used by `joi service register`
///   when ops drop a single file under `~/.config/joi/services/`).
/// * **Nested** — `<dir>/<id>/spec.json` (used by the workspace
///   `data/services/` tree so each spec can ship a `bundle/` sibling).
///
/// Malformed files are logged and skipped — one bad spec must not block
/// the rest of the fleet (matches `agent_serve::load_specs` behavior).
pub(crate) fn load_specs(dir: &Path) -> Result<Vec<ServiceSpec>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let nested = path.join("spec.json");
            if !nested.exists() {
                continue;
            }
            push_spec(&nested, &mut out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            push_spec(&path, &mut out);
        }
    }
    Ok(out)
}

fn push_spec(path: &Path, out: &mut Vec<ServiceSpec>) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "skipping unreadable ServiceSpec");
            return;
        }
    };
    match serde_json::from_str::<ServiceSpec>(&text) {
        Ok(mut spec) => {
            spec.normalize();
            out.push(spec);
        }
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "skipping malformed ServiceSpec",
            );
        }
    }
}

/// Run the service host: load every spec under `--specs` (default
/// `~/.config/joi/services/`), filter by `--allow-services` if given,
/// then hand to [`ServiceHost::serve`]. Blocks until Ctrl-C.
///
/// In S1 the host ships **no built-in plugins**, so any spec whose
/// `kind` doesn't match a registered plugin is logged-and-skipped (see
/// [`ServiceHost::serve`]). That keeps the wiring honest end-to-end —
/// the connection lifecycle, signal handling, and spec loader all run —
/// while leaving plugin code itself for S2.
pub async fn serve(
    specs_dir: Option<PathBuf>,
    server_url: String,
    allow_services: Vec<String>,
) -> Result<()> {
    let dir = specs_dir.unwrap_or_else(default_specs_dir);
    let mut specs =
        load_specs(&dir).with_context(|| format!("load ServiceSpecs from {}", dir.display()))?;
    if !allow_services.is_empty() {
        let allow: HashSet<&str> = allow_services.iter().map(String::as_str).collect();
        specs.retain(|s| allow.contains(s.id.as_str()));
    }
    if specs.is_empty() {
        eprintln!(
            "no ServiceSpec files matched under {} (allow_services={:?})",
            dir.display(),
            allow_services
        );
        return Ok(());
    }
    tracing::info!(
        spec_count = specs.len(),
        dir = %dir.display(),
        "starting service host",
    );
    let data_root = state::default_data_root();
    let mut host = ServiceHost::new(server_url, data_root).with_specs_dir(dir.clone());
    // S3: scheduler is the first long-process plugin under the host.
    // AM stays a short-lived `am-handler` subprocess (S2) and isn't
    // registered here.
    host.register(Arc::new(SchedulerPlugin::default()));
    host.serve(specs).await
}

/// `joi service reload <service_id>` — bump the per-service reload
/// marker so a running `joi service serve` host re-reads the
/// ServiceSpec and respawns the supervised plugin instance(s) for
/// that id. See design §7.1.
pub fn reload(service_id: String) -> Result<()> {
    let data_root = state::default_data_root();
    let path = super::reload::service_marker_path(&data_root, &service_id);
    let epoch = super::reload::bump(&path)?;
    if crate::render::is_json() {
        crate::render::print_json(&serde_json::json!({
            "service_id": service_id,
            "marker": path.display().to_string(),
            "epoch_ms": epoch,
        }));
    } else {
        println!(
            "reload requested  service={service_id}  epoch_ms={epoch}\n  marker={}",
            path.display()
        );
        println!("(host will respawn on next poll cycle; if no `joi service serve` is running this is a no-op)");
    }
    Ok(())
}

/// `joi service am-handler --service-id <id>` — per-message AM bridge
/// handler. Stage 4 wires the CLI; the orchestrator + plugin logic
/// lives in `crate::service::am::handler` (S2).
pub async fn am_handler(
    server_url: String,
    service_id: String,
    specs_dir: Option<PathBuf>,
    async_reply: Option<String>,
) -> Result<()> {
    let dir = specs_dir.unwrap_or_else(default_specs_dir);
    crate::service::am::run_handler(server_url, service_id, dir, async_reply).await
}

/// `joi service validate <path>` — read a single ServiceSpec JSON file,
/// run `ServiceSpec::validate()`, exit 0 on success, propagate the
/// error otherwise. Useful in CI / pre-deploy hooks.
pub fn validate(path: PathBuf) -> Result<()> {
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut spec: ServiceSpec = serde_json::from_str(&text)
        .with_context(|| format!("parse {} as ServiceSpec", path.display()))?;
    spec.normalize();
    spec.validate()
        .with_context(|| format!("validate ServiceSpec `{}`", spec.id))?;
    println!(
        "ok: ServiceSpec `{}` (kind={}, actor={})",
        spec.id, spec.kind, spec.actor.id
    );
    Ok(())
}

/// `joi service start --spec <id> --in <thread> [--params <json>] [--channel <id>]`.
///
/// Looks up the ServiceSpec, asserts `lifecycle = thread_bound`, and
/// writes a per-instance `request.json` under the host data root.
/// Idempotent — re-running with the same scope overwrites the params,
/// which the host watcher debounces by file mtime / content hash.
pub fn start(
    spec_id: String,
    thread: String,
    channel: Option<String>,
    params: Option<String>,
    specs_dir: Option<PathBuf>,
) -> Result<()> {
    use proto::methods::ServiceLifecycle;

    let dir = specs_dir.unwrap_or_else(default_specs_dir);
    let specs = load_specs(&dir)
        .with_context(|| format!("load ServiceSpecs from {}", dir.display()))?;
    let spec = specs
        .into_iter()
        .find(|s| s.id == spec_id)
        .with_context(|| format!("ServiceSpec `{spec_id}` not found under {}", dir.display()))?;
    if !matches!(spec.lifecycle, ServiceLifecycle::ThreadBound) {
        anyhow::bail!(
            "ServiceSpec `{}` has lifecycle = {:?}; only `thread_bound` specs accept `service start`",
            spec.id,
            spec.lifecycle
        );
    }
    let params_value: serde_json::Value = match params {
        Some(s) => serde_json::from_str(&s)
            .with_context(|| format!("--params must be valid JSON: {s}"))?,
        None => serde_json::json!({}),
    };
    let req = crate::service::instance::InstanceRequest {
        version: 1,
        spec_id: spec.id.clone(),
        scope: crate::service::instance::InstanceScope {
            kind: "thread".to_string(),
            id: thread.clone(),
            channel_id: channel,
        },
        params: params_value,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    let data_root = state::default_data_root();
    let path = crate::service::instance::write_request(&data_root, &req)?;
    if crate::render::is_json() {
        crate::render::print_json(&serde_json::json!({
            "spec_id": spec.id,
            "thread_id": thread,
            "request_path": path.display().to_string(),
        }));
    } else {
        println!(
            "ok: instance request written\n  spec={}\n  thread={}\n  request={}",
            spec.id,
            thread,
            path.display()
        );
        println!("(host will start the instance on next watcher tick; no-op if no `joi service serve` is running)");
    }
    Ok(())
}

/// `joi service stop --spec <id> --in <thread>` — remove the instance
/// request file. Safe to call on an instance that's already stopped.
pub fn stop(spec_id: String, thread: String) -> Result<()> {
    let data_root = state::default_data_root();
    let removed = crate::service::instance::delete_request(&data_root, &spec_id, &thread)?;
    if crate::render::is_json() {
        crate::render::print_json(&serde_json::json!({
            "spec_id": spec_id,
            "thread_id": thread,
            "removed": removed,
        }));
    } else if removed {
        println!("ok: instance request removed  spec={spec_id}  thread={thread}");
    } else {
        println!("noop: no instance request for  spec={spec_id}  thread={thread}");
    }
    Ok(())
}

/// `joi service status [--spec <id>]` — list active instances. Without
/// a spec filter, walks every spec dir under the host data root.
pub fn status(spec_id: Option<String>) -> Result<()> {
    let data_root = state::default_data_root();
    let mut summary: Vec<(String, Vec<String>)> = Vec::new();
    if let Some(sid) = spec_id {
        let instances = crate::service::instance::list_instances(&data_root, &sid)?;
        summary.push((sid, instances));
    } else {
        let services_dir = data_root.join("services");
        if services_dir.exists() {
            for entry in fs::read_dir(&services_dir)
                .with_context(|| format!("read_dir {}", services_dir.display()))?
            {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                    continue;
                };
                let instances = crate::service::instance::list_instances(&data_root, &name)?;
                if !instances.is_empty() {
                    summary.push((name, instances));
                }
            }
        }
    }
    summary.sort_by(|a, b| a.0.cmp(&b.0));
    if crate::render::is_json() {
        let payload = serde_json::json!({
            "data_root": data_root.display().to_string(),
            "services": summary
                .iter()
                .map(|(s, ids)| serde_json::json!({"spec_id": s, "instances": ids}))
                .collect::<Vec<_>>(),
        });
        crate::render::print_json(&payload);
    } else if summary.is_empty() {
        println!("no active thread-bound instances under {}", data_root.display());
    } else {
        for (s, ids) in &summary {
            if ids.is_empty() {
                println!("{s}: (no active instances)");
            } else {
                println!("{s}:");
                for id in ids {
                    println!("  - {id}");
                }
            }
        }
    }
    Ok(())
}
