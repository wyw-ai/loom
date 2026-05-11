use std::collections::HashSet;

use agent_runtime::discovery::{
    detect_agent_cli_providers, provider_specs_from_agent_definitions, AgentDefinition,
};
use anyhow::{Context, Result};
use proto::methods::{AgentInfo, AgentListResult};
use serde::Deserialize;

use crate::{config, render};

pub fn list() -> Result<()> {
    let cfg = load_desktop_config().unwrap_or_default();
    let providers = detect_agent_cli_providers();
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
    let agents = provider_specs_from_agent_definitions(&providers, &definitions)
        .into_iter()
        .flat_map(|provider| provider.into_agent_specs())
        .map(|spec| AgentInfo {
            spec,
            status: "registered".into(),
            pid: None,
            session_id: None,
        })
        .collect::<Vec<_>>();
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
        println!(
            "{}\t{}\tstatus={}\tcommand={}",
            a.spec.actor.id, a.spec.actor.display_name, a.status, a.spec.transport.command,
        );
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
    }
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
