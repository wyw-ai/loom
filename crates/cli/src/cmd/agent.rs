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
    machines: Vec<MachineConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineConfig {
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
