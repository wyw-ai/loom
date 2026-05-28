use std::path::{Path, PathBuf};

use agent_runtime::provider::{providers_dir, ProviderRegistry};
use anyhow::{anyhow, Context, Result};
use proto::methods::ProviderManifest;
use serde_json::json;

use crate::{config, render};

pub fn validate(path: PathBuf) -> Result<()> {
    let manifest = resolve_manifest_file(&path)?;
    if render::is_json() {
        render::print_json(&json!({
            "ok": true,
            "provider": provider_summary(&manifest, false, None),
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
            "provider": provider_summary(&manifest, false, None),
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
    let registry = ProviderRegistry::load(&config::config_dir()).map_err(|err| anyhow!(err))?;
    let manifest = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow!("provider `{provider_id}` not found"))?;
    if render::is_json() {
        render::print_json(manifest);
    } else {
        println!("{}", serde_json::to_string_pretty(manifest)?);
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
        render::print_json(&json!({
            "ok": detected.is_some(),
            "provider": provider_summary(
                manifest,
                detected.is_some(),
                detected.as_ref().map(|provider| provider.command.as_str()),
            ),
        }));
    } else if let Some(provider) = detected {
        println!("provider {} OK", provider.id);
        println!("command  = {}", provider.command);
        println!("mode     = print");
        println!("args     = {}", provider.args.join(" "));
    } else {
        println!(
            "provider {} is valid but no command was detected",
            manifest.id
        );
        println!("candidates = {}", manifest.detect.candidates.join(", "));
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
) -> serde_json::Value {
    json!({
        "id": manifest.id.clone(),
        "displayName": if manifest.display_name.trim().is_empty() {
            manifest.id.as_str()
        } else {
            manifest.display_name.as_str()
        },
        "detected": detected,
        "command": command.unwrap_or(""),
        "modes": manifest.modes.keys().cloned().collect::<Vec<_>>(),
        "models": manifest.models.clone(),
    })
}
