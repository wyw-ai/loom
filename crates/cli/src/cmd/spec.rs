//! `loom agent spec` / `loom service spec` / `loom agent bundle` — read-only
//! spec and bundle inspection. Lets a teaching agent (e.g. `teacher`) or
//! ops examine deployed AgentSpec / ServiceSpec / bundle layouts without
//! touching the runtime.
//!
//! These commands do not contact the loom-server. Specs and bundles are
//! local config artifacts owned by the operator running `loom-daemon`
//! on the same host.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use proto::methods::ServiceSpec;

use crate::render;

const REDACTED: &str = "***redacted***";

fn redact_value(value: &mut serde_json::Value) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let lower = k.to_ascii_lowercase();
                if lower.contains("token")
                    || lower.contains("secret")
                    || lower.contains("password")
                    || lower.contains("api_key")
                    || lower.contains("apikey")
                {
                    *v = Value::String(REDACTED.into());
                } else {
                    redact_value(v);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr {
                redact_value(v);
            }
        }
        _ => {}
    }
}

pub fn agent_list() -> Result<()> {
    let specs = super::agent::load_specs_at(&super::agent::default_specs_dir())?;
    if render::is_json() {
        let rows: Vec<_> = specs
            .iter()
            .map(|s| {
                serde_json::json!({
                    "actor_id": s.actor.id,
                    "display_name": s.actor.display_name,
                    "provider": s.provider_ref.id.as_str(),
                })
            })
            .collect();
        render::print_json(&serde_json::json!({"agents": rows}));
        return Ok(());
    }
    if specs.is_empty() {
        println!(
            "(no agent specs in {})",
            super::agent::default_specs_dir().display()
        );
        return Ok(());
    }
    println!("DIR: {}", super::agent::default_specs_dir().display());
    for s in specs {
        let provider = s.provider_ref.id.as_str();
        println!(
            "{}\t{}\tprovider={}",
            s.actor.id, s.actor.display_name, provider,
        );
    }
    Ok(())
}

pub fn agent_get(actor_id: String, raw: bool) -> Result<()> {
    let path = super::agent::default_specs_dir().join(format!("{actor_id}.json"));
    if !path.exists() {
        bail!("agent spec not found: {}", path.display());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if raw {
        print!("{text}");
        if !text.ends_with('\n') {
            println!();
        }
        return Ok(());
    }
    let mut value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    redact_value(&mut value);
    if render::is_json() {
        render::print_json(&value);
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}

pub fn service_list() -> Result<()> {
    let dir = super::service::default_specs_dir();
    let specs: Vec<ServiceSpec> = super::service::load_specs(&dir)?;
    if render::is_json() {
        let rows: Vec<_> = specs
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "kind": s.kind,
                    "actor": s.actor.id,
                    "display_name": s.actor.display_name,
                })
            })
            .collect();
        render::print_json(&serde_json::json!({"services": rows}));
        return Ok(());
    }
    if specs.is_empty() {
        println!("(no service specs in {})", dir.display());
        return Ok(());
    }
    println!("DIR: {}", dir.display());
    for s in specs {
        println!("{}\t{}\t{}", s.id, s.kind, s.actor.display_name);
    }
    Ok(())
}

pub fn service_get(service_id: String, raw: bool) -> Result<()> {
    let path = super::service::default_specs_dir().join(format!("{service_id}.json"));
    if !path.exists() {
        bail!("service spec not found: {}", path.display());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if raw {
        print!("{text}");
        if !text.ends_with('\n') {
            println!();
        }
        return Ok(());
    }
    let mut value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    redact_value(&mut value);
    if render::is_json() {
        render::print_json(&value);
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}

pub fn bundle_get(actor_id: String, file: Option<String>, list: bool) -> Result<()> {
    let bundle_dir = resolve_bundle_dir(&actor_id)?;
    if list {
        let mut entries: Vec<String> = walk(&bundle_dir, &bundle_dir)?;
        entries.sort();
        if render::is_json() {
            render::print_json(&serde_json::json!({
                "bundle_dir": bundle_dir.display().to_string(),
                "entries": entries,
            }));
        } else {
            println!("BUNDLE: {}", bundle_dir.display());
            for e in entries {
                println!("{e}");
            }
        }
        return Ok(());
    }
    if let Some(rel) = file {
        let safe = sanitize_relative(&rel)?;
        let target = bundle_dir.join(&safe);
        if !target.starts_with(&bundle_dir) {
            bail!("file {} escapes bundle dir", rel);
        }
        if !target.exists() {
            bail!("not found: {}", target.display());
        }
        let bytes = std::fs::read(&target).with_context(|| format!("read {}", target.display()))?;
        std::io::Write::write_all(&mut std::io::stdout(), &bytes)?;
        return Ok(());
    }
    if render::is_json() {
        render::print_json(&serde_json::json!({
            "bundle_dir": bundle_dir.display().to_string(),
        }));
    } else {
        println!("{}", bundle_dir.display());
    }
    Ok(())
}

fn resolve_bundle_dir(actor_id: &str) -> Result<PathBuf> {
    // Match `agent_serve.rs`'s default_data_root semantics so a teacher
    // sees the same bundle the runtime will pick on this host.
    let data_root = std::env::var_os("LOOM_AGENT_DATA_ROOT")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join("loom").join("agents")))
        .unwrap_or_else(|| PathBuf::from(".loom").join("agents-data"));
    let candidate = data_root
        .join("agents")
        .join(actor_id)
        .join("bundles")
        .join("current");
    if !candidate.exists() {
        bail!(
            "bundle not found at {} (agent never served? run `loom-daemon` first)",
            candidate.display()
        );
    }
    Ok(candidate)
}

fn walk(root: &PathBuf, base: &PathBuf) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path, base)?);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.display().to_string());
        }
    }
    Ok(out)
}

fn sanitize_relative(rel: &str) -> Result<PathBuf> {
    let p = PathBuf::from(rel);
    if p.is_absolute() {
        bail!("expected relative path, got absolute: {}", rel);
    }
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => bail!("`..` not allowed in bundle paths"),
            Component::Prefix(_) | Component::RootDir => {
                bail!("absolute components not allowed: {}", rel)
            }
            _ => {}
        }
    }
    Ok(p)
}
