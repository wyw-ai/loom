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

/// Load every `*.json` under `dir` and parse as `ServiceSpec`. Malformed
/// files are logged and skipped (matches `agent_serve::load_specs`
/// behavior — one bad spec must not block the rest of the fleet).
pub(crate) fn load_specs(dir: &Path) -> Result<Vec<ServiceSpec>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("read spec {}", path.display()))?;
        match serde_json::from_str::<ServiceSpec>(&text) {
            Ok(spec) => out.push(spec),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "skipping malformed ServiceSpec",
                );
            }
        }
    }
    Ok(out)
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
    let mut host = ServiceHost::new(server_url, data_root);
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
    let spec: ServiceSpec = serde_json::from_str(&text)
        .with_context(|| format!("parse {} as ServiceSpec", path.display()))?;
    spec.validate()
        .with_context(|| format!("validate ServiceSpec `{}`", spec.id))?;
    println!(
        "ok: ServiceSpec `{}` (kind={}, actor={})",
        spec.id, spec.kind, spec.actor.id
    );
    Ok(())
}
