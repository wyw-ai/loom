use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_runtime::discovery::{
    apply_provider_overrides, detect_agent_cli_providers, provider_specs_from_agent_definitions,
    AgentDefinition, AgentProviderOverride,
};
use anyhow::{Context, Result};
use proto::methods::{AgentInfo, AgentListResult, AgentSpec};
use serde::Deserialize;

use crate::{config, render};

pub fn list() -> Result<()> {
    let specs_dir = default_specs_dir();
    let mut seen = HashSet::new();
    let mut agents = load_specs_at(&specs_dir)?
        .into_iter()
        .map(|spec| {
            seen.insert(spec.actor.id.clone());
            AgentInfo {
                spec,
                status: "registered".into(),
                pid: None,
                session_id: None,
            }
        })
        .collect::<Vec<_>>();

    let cfg = load_desktop_config().unwrap_or_default();
    let detected_providers = detect_agent_cli_providers();
    let provider_overrides = cfg
        .machines
        .iter()
        .filter(|machine| machine_belongs_to_active_context(machine, &cfg))
        .flat_map(|machine| machine.providers.iter().cloned())
        .collect::<Vec<_>>();
    let providers = apply_provider_overrides(detected_providers, &provider_overrides);
    let provider_ids = providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect::<HashSet<_>>();
    let definitions = cfg
        .machines
        .iter()
        .filter(|machine| machine_belongs_to_active_context(machine, &cfg))
        .flat_map(|machine| {
            machine
                .agents
                .iter()
                .filter(|agent| provider_ids.contains(agent.provider_id.as_str()))
                .map(machine_agent_definition)
        })
        .collect::<Vec<_>>();
    agents.extend(
        provider_specs_from_agent_definitions(&providers, &definitions)
            .into_iter()
            .flat_map(|provider| provider.into_agent_specs())
            .filter(|spec| seen.insert(spec.actor.id.clone()))
            .map(|spec| AgentInfo {
                spec,
                status: "registered".into(),
                pid: None,
                session_id: None,
            }),
    );
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
        let provider = a
            .spec
            .provider_ref
            .as_ref()
            .map(|provider_ref| provider_ref.id.as_str())
            .unwrap_or("-");
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

#[derive(Debug, Clone, Default, Deserialize)]
struct DesktopConfig {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    account: Option<HumanAccount>,
    #[serde(default)]
    workspaces: Vec<WorkspaceConfig>,
    #[serde(default)]
    machines: Vec<MachineConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HumanAccount {
    #[serde(default)]
    actor_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceConfig {
    id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineConfig {
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    owner_actor_id: Option<String>,
    #[serde(default)]
    providers: Vec<AgentProviderOverride>,
    #[serde(default)]
    agents: Vec<MachineAgentConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineAgentConfig {
    provider_id: String,
    actor_id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    reasoning_effort: String,
    #[serde(default)]
    autostart: bool,
    #[serde(default)]
    avatar_url: String,
}

fn load_desktop_config() -> Result<DesktopConfig> {
    let path = config::config_dir().join("desktop.toml");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read desktop config {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn machine_belongs_to_active_context(machine: &MachineConfig, cfg: &DesktopConfig) -> bool {
    machine.workspace_id.as_deref() == active_workspace_id(cfg)
        && machine.owner_actor_id.as_deref() == active_account_actor_id(cfg)
}

fn active_workspace_id(cfg: &DesktopConfig) -> Option<&str> {
    cfg.active
        .as_deref()
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .or_else(|| cfg.workspaces.first())
        .map(|workspace| workspace.id.as_str())
}

fn active_account_actor_id(cfg: &DesktopConfig) -> Option<&str> {
    cfg.account
        .as_ref()
        .map(|account| account.actor_id.trim())
        .filter(|actor_id| !actor_id.is_empty())
}

fn machine_agent_definition(agent: &MachineAgentConfig) -> AgentDefinition {
    AgentDefinition {
        provider_id: agent.provider_id.clone(),
        actor_id: agent.actor_id.clone(),
        display_name: agent.name.clone(),
        description: non_empty(agent.description.trim()),
        model: non_empty(agent.model.trim()),
        reasoning_effort: non_empty(agent.reasoning_effort.trim()),
        autostart: agent.autostart,
        avatar_url: non_empty(agent.avatar_url.trim()),
    }
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
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
