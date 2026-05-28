use std::path::PathBuf;

use agent_runtime::provider::{providers_dir, validate_manifest, ProviderRegistry};
use anyhow::{anyhow, Context, Result};
use proto::methods::ProviderManifest;
use serde_json::json;

use crate::{config, render};

pub fn validate(path: PathBuf) -> Result<()> {
    let manifest = read_manifest(&path)?;
    validate_manifest(&manifest).map_err(|err| anyhow!(err))?;
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

pub fn add(path: PathBuf) -> Result<()> {
    let manifest = read_manifest(&path)?;
    validate_manifest(&manifest).map_err(|err| anyhow!(err))?;
    let dir = providers_dir(&config::config_dir());
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create provider dir {}", dir.display()))?;
    let target = dir.join(format!("{}.json", manifest.id));
    let text = serde_json::to_string_pretty(&manifest).context("serialize provider manifest")?;
    std::fs::write(&target, text)
        .with_context(|| format!("write provider manifest {}", target.display()))?;
    if render::is_json() {
        render::print_json(&json!({
            "ok": true,
            "path": target.display().to_string(),
            "provider": provider_summary(&manifest, false, None),
        }));
    } else {
        println!("added provider {} -> {}", manifest.id, target.display());
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
    validate_manifest(manifest).map_err(|err| anyhow!(err))?;
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

fn read_manifest(path: &PathBuf) -> Result<ProviderManifest> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("parse provider manifest {}", path.display()))
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
