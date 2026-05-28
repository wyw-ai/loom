use std::path::{Path, PathBuf};

use agent_runtime::provider::{providers_dir, DetectedProvider, ProviderRegistry};
use anyhow::{anyhow, Context, Result};
use proto::methods::{AgentProviderRef, ProviderManifest, ProviderModeSpec};
use serde_json::{json, Map, Value};

use crate::{config, render};

pub fn validate(path: PathBuf) -> Result<()> {
    let manifest = resolve_manifest_file(&path)?;
    if render::is_json() {
        render::print_json(&json!({
            "ok": true,
            "provider": provider_summary(&manifest, false, None, None),
        }));
    } else {
        println!("provider {} OK", manifest.id);
    }
    Ok(())
}

pub fn add(path: PathBuf, replace: bool) -> Result<()> {
    let (manifest, raw) = read_and_resolve_manifest_file(&path)?;
    let registry = ProviderRegistry::load(&config::config_dir()).map_err(|err| anyhow!(err))?;
    let dir = providers_dir(&config::config_dir());
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create provider dir {}", dir.display()))?;
    let target = dir.join(format!("{}.json", manifest.id));
    let target_exists = target.exists();
    if registry.get(&manifest.id).is_some() && !(replace && target_exists) {
        return Err(anyhow!(
            "provider `{}` already exists; use --replace only for existing local provider manifests, or use a new id with extends for local variants",
            manifest.id
        ));
    }
    if target_exists && !replace {
        return Err(anyhow!(
            "local provider `{}` already exists at {}; pass --replace to overwrite",
            manifest.id,
            target.display()
        ));
    }
    let text = serde_json::to_string_pretty(&raw).context("serialize provider manifest")?;
    std::fs::write(&target, text)
        .with_context(|| format!("write provider manifest {}", target.display()))?;
    if render::is_json() {
        render::print_json(&json!({
            "ok": true,
            "replaced": target_exists,
            "path": target.display().to_string(),
            "provider": provider_summary(&manifest, false, None, None),
        }));
    } else {
        let verb = if target_exists { "replaced" } else { "added" };
        println!("{verb} provider {} -> {}", manifest.id, target.display());
    }
    Ok(())
}

pub fn list() -> Result<()> {
    let config_dir = config::config_dir();
    let registry = ProviderRegistry::load(&config_dir).map_err(|err| anyhow!(err))?;
    let detected = registry
        .detect_with_path(std::env::var_os("PATH").unwrap_or_default())
        .map_err(|err| anyhow!(err))?;
    let rows = registry
        .manifests()
        .map(|manifest| {
            let detected_provider = detected.iter().find(|provider| provider.id == manifest.id);
            provider_summary(
                manifest,
                detected_provider.is_some(),
                detected_provider.map(|provider| provider.command.as_str()),
                Some(&config_dir),
            )
        })
        .collect::<Vec<_>>();
    if render::is_json() {
        render::print_json(&json!({ "providers": rows }));
    } else if rows.is_empty() {
        println!("No providers registered.");
    } else {
        for row in rows {
            let detected = if row["detected"].as_bool() == Some(true) {
                "detected"
            } else {
                "missing"
            };
            println!(
                "{}\t{}\t{}\t{}",
                row["id"].as_str().unwrap_or_default(),
                row["displayName"].as_str().unwrap_or_default(),
                detected,
                row["command"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn show(provider_id: String) -> Result<()> {
    let config_dir = config::config_dir();
    let registry = ProviderRegistry::load(&config_dir).map_err(|err| anyhow!(err))?;
    let manifest = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow!("provider `{provider_id}` not found"))?;
    let detected = registry
        .detect_with_path(std::env::var_os("PATH").unwrap_or_default())
        .map_err(|err| anyhow!(err))?
        .into_iter()
        .find(|provider| provider.id == provider_id);
    let detail = provider_detail(&registry, &config_dir, manifest, detected.as_ref())?;
    if render::is_json() {
        render::print_json(&detail);
    } else {
        print_provider_detail(&detail);
    }
    Ok(())
}

pub fn remove(provider_id: String) -> Result<()> {
    let path = providers_dir(&config::config_dir()).join(format!("{provider_id}.json"));
    if !path.exists() {
        return Err(anyhow!(
            "local provider `{provider_id}` not found at {}",
            path.display()
        ));
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("remove provider manifest {}", path.display()))?;
    if render::is_json() {
        render::print_json(&json!({ "ok": true, "removed": provider_id }));
    } else {
        println!("removed provider {provider_id}");
    }
    Ok(())
}

pub fn doctor(provider_id: String) -> Result<()> {
    let config_dir = config::config_dir();
    let registry = ProviderRegistry::load(&config_dir).map_err(|err| anyhow!(err))?;
    let manifest = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow!("provider `{provider_id}` not found"))?;
    let detected = registry
        .detect_with_path(std::env::var_os("PATH").unwrap_or_default())
        .map_err(|err| anyhow!(err))?
        .into_iter()
        .find(|provider| provider.id == provider_id);
    if render::is_json() {
        let detail = provider_detail(&registry, &config_dir, manifest, detected.as_ref())?;
        render::print_json(&json!({
            "ok": detected.is_some(),
            "checks": provider_doctor_checks(manifest, detected.as_ref()),
            "provider": detail,
        }));
    } else {
        let checks = provider_doctor_checks(manifest, detected.as_ref());
        if detected.is_some() {
            println!("provider {} OK", manifest.id);
        } else {
            println!("provider {} has issues", manifest.id);
        }
        for check in checks {
            let status = if check["ok"].as_bool() == Some(true) {
                "ok"
            } else {
                "fail"
            };
            println!(
                "[{status}] {}",
                check["message"].as_str().unwrap_or_default()
            );
        }
        let detail = provider_detail(&registry, &config_dir, manifest, detected.as_ref())?;
        print_provider_detail(&detail);
    }
    Ok(())
}

fn read_and_resolve_manifest_file(path: &Path) -> Result<(ProviderManifest, serde_json::Value)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let raw = serde_json::from_str::<serde_json::Value>(&text)
        .with_context(|| format!("parse provider manifest {}", path.display()))?;
    let registry = ProviderRegistry::load(&config::config_dir()).map_err(|err| anyhow!(err))?;
    let manifest = registry
        .resolve_manifest_value(raw.clone())
        .map_err(|err| anyhow!(err))?;
    Ok((manifest, raw))
}

fn resolve_manifest_file(path: &Path) -> Result<ProviderManifest> {
    read_and_resolve_manifest_file(path).map(|(manifest, _)| manifest)
}

fn provider_summary(
    manifest: &ProviderManifest,
    detected: bool,
    command: Option<&str>,
    config_dir: Option<&Path>,
) -> serde_json::Value {
    json!({
        "id": manifest.id.clone(),
        "displayName": if manifest.display_name.trim().is_empty() {
            manifest.id.as_str()
        } else {
            manifest.display_name.as_str()
        },
        "source": provider_source_value(config_dir, manifest),
        "detected": detected,
        "command": command.unwrap_or(""),
        "defaultMode": default_mode_name(manifest),
        "modes": manifest.modes.keys().cloned().collect::<Vec<_>>(),
        "models": manifest.models.clone(),
    })
}

fn provider_detail(
    registry: &ProviderRegistry,
    config_dir: &Path,
    manifest: &ProviderManifest,
    detected: Option<&DetectedProvider>,
) -> Result<Value> {
    let mut modes = Map::new();
    let default_mode = default_mode_name(manifest).to_string();
    for (name, mode) in &manifest.modes {
        modes.insert(
            name.clone(),
            provider_mode_detail(registry, manifest, name, mode, detected.is_some())?,
        );
    }
    Ok(json!({
        "id": manifest.id,
        "displayName": display_name(manifest),
        "source": provider_source_value(Some(config_dir), manifest),
        "detected": detected.map(|provider| {
            json!({
                "available": true,
                "command": provider.command,
                "transportKind": provider.transport_kind,
                "args": provider.args,
                "env": provider.env,
            })
        }).unwrap_or_else(|| {
            json!({
                "available": false,
                "candidates": manifest.detect.candidates,
            })
        }),
        "detect": manifest.detect,
        "defaultMode": default_mode,
        "models": manifest.models,
        "modes": modes,
    }))
}

fn provider_mode_detail(
    registry: &ProviderRegistry,
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
    detected: bool,
) -> Result<Value> {
    let runtime_plan = if detected {
        let provider_ref = AgentProviderRef {
            id: manifest.id.clone(),
            mode: Some(mode_name.to_string()),
            model: manifest
                .models
                .as_ref()
                .and_then(|models| models.default.clone()),
            reasoning_effort: None,
        };
        match registry.resolve_runtime_plan(&provider_ref) {
            Ok(plan) => serde_json::to_value(plan).context("serialize runtime plan")?,
            Err(err) => json!({ "error": err }),
        }
    } else {
        Value::Null
    };
    Ok(json!({
        "transport": mode.transport,
        "command": mode.command,
        "args": mode.args,
        "env": mode.env,
        "stdin": mode.stdin,
        "prompt": mode.prompt,
        "stdout": mode.stdout,
        "session": mode.session,
        "timeoutMs": mode.timeout_ms,
        "idleTimeoutMs": mode.idle_timeout_ms,
        "runtimePlan": runtime_plan,
    }))
}

fn provider_doctor_checks(
    manifest: &ProviderManifest,
    detected: Option<&DetectedProvider>,
) -> Vec<Value> {
    let mut checks = vec![json!({
        "id": "manifest.valid",
        "ok": true,
        "message": "manifest loaded and validated",
    })];
    checks.push(json!({
        "id": "command.detected",
        "ok": detected.is_some(),
        "message": detected
            .map(|provider| format!("command detected: {}", provider.command))
            .unwrap_or_else(|| format!(
                "no command detected; candidates: {}",
                manifest.detect.candidates.join(", ")
            )),
    }));
    checks.push(json!({
        "id": "mode.default",
        "ok": manifest.modes.contains_key(default_mode_name(manifest)),
        "message": format!("default mode: {}", default_mode_name(manifest)),
    }));
    checks
}

fn print_provider_detail(detail: &Value) {
    println!("provider  = {}", detail["id"].as_str().unwrap_or_default());
    println!(
        "name      = {}",
        detail["displayName"].as_str().unwrap_or_default()
    );
    println!(
        "source    = {}",
        detail["source"]["kind"].as_str().unwrap_or_default()
    );
    if let Some(path) = detail["source"]["path"].as_str() {
        println!("path      = {path}");
    }
    println!(
        "detected  = {}",
        if detail["detected"]["available"].as_bool() == Some(true) {
            "yes"
        } else {
            "no"
        }
    );
    if let Some(command) = detail["detected"]["command"].as_str() {
        println!("command   = {command}");
    } else if let Some(candidates) = detail["detected"]["candidates"].as_array() {
        let candidates = candidates
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        println!("candidates = {candidates}");
    }
    println!(
        "default   = {}",
        detail["defaultMode"].as_str().unwrap_or_default()
    );
    if !detail["models"].is_null() {
        println!("models    = {}", compact_json(&detail["models"]));
    }
    if let Some(modes) = detail["modes"].as_object() {
        for (name, mode) in modes {
            println!();
            println!("[mode {name}]");
            println!(
                "transport = {}",
                mode["transport"].as_str().unwrap_or_default()
            );
            println!(
                "command   = {}",
                mode["command"].as_str().unwrap_or_default()
            );
            println!("args      = {}", compact_json(&mode["args"]));
            if !mode["env"].as_object().is_none_or(Map::is_empty) {
                println!("env       = {}", compact_json(&mode["env"]));
            }
            if !mode["stdin"].is_null() {
                println!("stdin     = {}", compact_json(&mode["stdin"]));
            }
            println!("prompt    = {}", compact_json(&mode["prompt"]));
            println!("stdout    = {}", compact_json(&mode["stdout"]));
            println!("session   = {}", compact_json(&mode["session"]));
            if !mode["runtimePlan"].is_null() {
                println!("runtime   = {}", compact_json(&mode["runtimePlan"]));
            }
        }
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<json error>".into())
}

fn display_name(manifest: &ProviderManifest) -> &str {
    if manifest.display_name.trim().is_empty() {
        manifest.id.as_str()
    } else {
        manifest.display_name.as_str()
    }
}

fn default_mode_name(manifest: &ProviderManifest) -> &str {
    if manifest.modes.contains_key("print") {
        "print"
    } else {
        manifest
            .modes
            .keys()
            .next()
            .map(String::as_str)
            .unwrap_or("print")
    }
}

fn provider_source_value(config_dir: Option<&Path>, manifest: &ProviderManifest) -> Value {
    let Some(config_dir) = config_dir else {
        return json!({ "kind": "file" });
    };
    let path = providers_dir(config_dir).join(format!("{}.json", manifest.id));
    if path.exists() {
        json!({ "kind": "local", "path": path.display().to_string() })
    } else {
        json!({ "kind": "builtin" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-cli-provider-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("create temp config dir");
        path
    }

    #[test]
    fn provider_detail_exposes_source_default_mode_and_decoder_capture() {
        let config_dir = temp_config_dir("detail");
        let registry = ProviderRegistry::load(&config_dir).expect("registry");
        let manifest = registry.get("qoder").expect("qoder manifest");

        let detail = provider_detail(&registry, &config_dir, manifest, None).expect("detail");

        assert_eq!(detail["source"]["kind"], "builtin");
        assert_eq!(detail["defaultMode"], "print");
        assert_eq!(
            detail["modes"]["print"]["stdout"]["capture"]["session"]["path"],
            "$.session_id"
        );
        assert!(detail["modes"]["print"]["runtimePlan"].is_null());
        std::fs::remove_dir_all(config_dir).ok();
    }

    #[test]
    fn doctor_checks_report_missing_command_candidates() {
        let config_dir = temp_config_dir("doctor");
        let registry = ProviderRegistry::load(&config_dir).expect("registry");
        let manifest = registry.get("claude").expect("claude manifest");

        let checks = provider_doctor_checks(manifest, None);

        assert!(checks.iter().any(|check| {
            check["id"] == "manifest.valid" && check["ok"].as_bool() == Some(true)
        }));
        assert!(checks.iter().any(|check| {
            check["id"] == "command.detected"
                && check["ok"].as_bool() == Some(false)
                && check["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("candidates: claude"))
        }));
        std::fs::remove_dir_all(config_dir).ok();
    }
}
