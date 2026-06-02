use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::methods::{AgentInfo, AgentListResult, AgentSpec};

use crate::{config, render};

pub fn list() -> Result<()> {
    let mut agents = load_specs_at(&default_specs_dir())?
        .into_iter()
        .map(|spec| AgentInfo {
            spec,
            status: "registered".into(),
            pid: None,
            session_id: None,
        })
        .collect::<Vec<_>>();

    agents.sort_by(|a, b| a.spec.actor.id.cmp(&b.spec.actor.id));
    let res = AgentListResult { agents };
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.agents.is_empty() {
        println!("(no daemon-configured agents)");
        return Ok(());
    }
    for a in res.agents {
        let provider = a.spec.provider_ref.id.as_str();
        println!(
            "{}\t{}\tstatus={}\tprovider={}",
            a.spec.actor.id, a.spec.actor.display_name, a.status, provider,
        );
    }
    Ok(())
}

/// Bump the per-actor reload marker so a running `loom-daemon`
/// host re-reads the AgentSpec + bundle and respawns the worker.
pub fn reload(actor_id: String) -> Result<()> {
    let data_root = super::agent_serve::default_data_root_pub();
    let path = super::reload::agent_marker_path(&data_root, &actor_id);
    let epoch = super::reload::bump(&path)?;
    if crate::render::is_json() {
        crate::render::print_json(&serde_json::json!({
            "actor_id": actor_id,
            "marker": path.display().to_string(),
            "epoch_ms": epoch,
        }));
    } else {
        println!(
            "reload requested  actor={actor_id}  epoch_ms={epoch}\n  marker={}",
            path.display()
        );
        println!("(host will respawn on next poll cycle; if no compatible host is running this is a no-op)");
    }
    Ok(())
}

pub(crate) fn default_specs_dir() -> PathBuf {
    if let Ok(s) = std::env::var("LOOM_AGENT_SPECS") {
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    config::config_dir().join("agents")
}

pub(crate) fn load_specs_at(dir: &Path) -> Result<Vec<AgentSpec>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let target = if file_type.is_dir() {
            let nested = path.join("spec.json");
            if !nested.exists() {
                continue;
            }
            nested
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            path
        } else {
            continue;
        };
        let text = std::fs::read_to_string(&target)
            .with_context(|| format!("read agent spec {}", target.display()))?;
        let spec: AgentSpec = serde_json::from_str(&text)
            .with_context(|| format!("parse agent spec {}", target.display()))?;
        out.push(spec);
    }
    out.sort_by(|a, b| a.actor.id.cmp(&b.actor.id));
    Ok(out)
}
