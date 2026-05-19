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

use crate::config;
use crate::service::scheduler::SchedulerPlugin;
use crate::service::{state, ServiceHost};

pub(crate) fn default_specs_dir() -> PathBuf {
    if let Ok(s) = std::env::var("JOI_SERVICE_SPECS") {
        return PathBuf::from(s);
    }
    config::service_specs_dir()
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
    let specs =
        load_specs(&dir).with_context(|| format!("load ServiceSpecs from {}", dir.display()))?;
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
        Some(s) => {
            serde_json::from_str(&s).with_context(|| format!("--params must be valid JSON: {s}"))?
        }
        None => serde_json::json!({}),
    };
    if let Some(schema) = spec.params_schema.as_ref() {
        validate_params(&params_value, schema).with_context(|| {
            format!(
                "--params failed ServiceSpec.params_schema for `{}`",
                spec.id
            )
        })?;
    }
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
        println!(
            "no active thread-bound instances under {}",
            data_root.display()
        );
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

/// Validate `--params` JSON against a `ServiceSpec.params_schema` value.
///
/// Supports the subset that ServiceSpec uses in this repo:
///   - top-level must be an object
///   - schema.required: [str]   -> every name must be present
///   - schema.properties.<k>.type: "string"|"number"|"integer"|"boolean"|"array"|"object"
///     -> per-key shallow type check (only when the key is present)
///
/// Anything outside that subset is ignored — we deliberately don't pull
/// in a full JSON-schema crate for a 5-field guard.
fn validate_params(params: &serde_json::Value, schema: &serde_json::Value) -> Result<()> {
    let obj = params
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("--params must be a JSON object"))?;

    if let Some(req) = schema.get("required").and_then(|v| v.as_array()) {
        let missing: Vec<String> = req
            .iter()
            .filter_map(|n| n.as_str().map(|s| s.to_string()))
            .filter(|n| !obj.contains_key(n))
            .collect();
        if !missing.is_empty() {
            anyhow::bail!("missing required params: {}", missing.join(", "));
        }
    }
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        for (key, decl) in props {
            let Some(value) = obj.get(key) else { continue };
            let Some(want) = decl.get("type").and_then(|v| v.as_str()) else {
                continue;
            };
            let ok = match want {
                "string" => value.is_string(),
                "number" => value.is_f64() || value.is_i64() || value.is_u64(),
                "integer" => value.is_i64() || value.is_u64(),
                "boolean" => value.is_boolean(),
                "array" => value.is_array(),
                "object" => value.is_object(),
                _ => true,
            };
            if !ok {
                anyhow::bail!(
                    "param `{key}` has wrong type: expected {want}, got {}",
                    type_label(value)
                );
            }
        }
    }
    Ok(())
}

fn type_label(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod params_schema_tests {
    use super::validate_params;
    use serde_json::json;

    #[test]
    fn ok_when_required_present_and_types_match() {
        let schema = json!({
            "required": ["mr_url"],
            "properties": { "mr_url": { "type": "string" } }
        });
        validate_params(&json!({"mr_url": "https://x/y/z"}), &schema).unwrap();
    }

    #[test]
    fn err_when_required_missing() {
        let schema = json!({"required": ["mr_url"]});
        let err = validate_params(&json!({}), &schema)
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing required"), "{err}");
        assert!(err.contains("mr_url"), "{err}");
    }

    #[test]
    fn err_when_type_mismatch() {
        let schema = json!({
            "properties": { "mr_url": { "type": "string" } }
        });
        let err = validate_params(&json!({"mr_url": 42}), &schema)
            .unwrap_err()
            .to_string();
        assert!(err.contains("wrong type"), "{err}");
        assert!(err.contains("mr_url"), "{err}");
    }

    #[test]
    fn ignores_unknown_keywords() {
        let schema = json!({
            "title": "ignored",
            "additionalProperties": false,
            "required": ["a"],
            "properties": {"a": {"type": "string"}}
        });
        validate_params(&json!({"a": "x"}), &schema).unwrap();
    }

    #[test]
    fn err_when_params_not_object() {
        let schema = json!({});
        let err = validate_params(&json!([1, 2]), &schema)
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be a JSON object"), "{err}");
    }
}

#[cfg(test)]
mod service_spec_loader_tests {
    use super::load_specs;
    use std::path::PathBuf;

    #[test]
    fn load_real_data_services_preserves_top_level_params_schema() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/services");
        let specs = load_specs(&dir).expect("load data/services specs");
        let bug_fix_loop = specs
            .iter()
            .find(|spec| spec.id == "a1-bug-fix-loop")
            .expect("a1-bug-fix-loop spec");

        assert!(
            bug_fix_loop
                .params_schema
                .as_ref()
                .and_then(|schema| schema.get("required"))
                .is_some(),
            "top-level params_schema must survive ServiceSpec deserialization"
        );
    }
}
