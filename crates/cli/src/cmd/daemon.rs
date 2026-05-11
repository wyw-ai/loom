//! `joi daemon` — machine-scoped agent host.
//!
//! The daemon is the spec-less successor to `joi agent serve`: it reads the
//! desktop machine config, auto-detects supported local agent CLIs, synthesizes
//! runtime `AgentSpec`s in memory, and then reuses the existing agent worker
//! implementation.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use agent_runtime::discovery::{
    detect_agent_cli_providers, provider_specs_from_agent_definitions, AgentDefinition,
    DetectedAgentProvider,
};
use anyhow::{anyhow, Context, Result};
use proto::methods::AgentSpec;
use serde::Deserialize;
use serde_json::json;
use tokio::time::{sleep, Duration};

use crate::cmd::agent_serve::{self, MachineHostSpec};
use crate::cmd::service;
use crate::daemon_ipc;
use crate::{config, render};

const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_secs(3);

pub async fn run(
    machine_id: Option<String>,
    data_root: Option<PathBuf>,
    allow_actors: Vec<String>,
    list_providers: bool,
    services_dir: Option<PathBuf>,
    allow_services: Vec<String>,
    no_services: bool,
    socket_path: Option<PathBuf>,
    server_url: String,
) -> Result<()> {
    let providers = detect_agent_cli_providers();
    if list_providers {
        print_providers(&providers);
        return Ok(());
    }

    let cfg = load_desktop_config().unwrap_or_default();
    let machine = select_machine(&cfg, machine_id.as_deref())?;
    let selected_machine_id = machine.id.clone();
    let data_root = data_root.unwrap_or_else(|| machine_data_root(&machine));
    std::fs::create_dir_all(&data_root)
        .with_context(|| format!("create data root {}", data_root.display()))?;
    std::env::set_var("JOI_AGENT_DATA_ROOT", &data_root);

    let machine_host = MachineHostSpec {
        machine_id: machine.id.clone(),
        actor_id: machine_connection_actor_id(&machine),
        display_name: machine.name.clone(),
    };

    let socket_path = socket_path
        .or_else(daemon_ipc::env_socket_path)
        .unwrap_or_else(daemon_ipc::default_socket_path);
    let proxy_handle = daemon_ipc::start_proxy(socket_path.clone(), server_url.clone()).await?;
    std::env::set_var(daemon_ipc::ENV_DAEMON_SOCKET, &socket_path);
    if let Err(err) = daemon_ipc::write_discovery(&socket_path, &server_url) {
        eprintln!("joi daemon: warning: failed to write daemon discovery: {err:#}");
    }

    if no_services {
        eprintln!("joi daemon: service host disabled by --no-services");
    } else {
        spawn_service_host(services_dir, server_url.clone(), allow_services);
    }

    let machine_host_handle =
        agent_serve::spawn_machine_host_loop(machine_host, server_url.clone());
    let mut running_agents = HashMap::new();
    let mut warned_missing = HashSet::new();

    eprintln!(
        "joi daemon: machine={} providers={} data={} reload={}s",
        machine.id,
        providers.len(),
        data_root.display(),
        CONFIG_RELOAD_INTERVAL.as_secs()
    );
    eprintln!("joi daemon: socket={}", socket_path.display());
    eprintln!("joi daemon: ready (ctrl-c to stop)");

    loop {
        match load_machine_specs(
            Some(&selected_machine_id),
            &allow_actors,
            &mut warned_missing,
        ) {
            Ok(snapshot) => {
                reconcile_agents(&mut running_agents, snapshot.specs, &server_url, &data_root)
            }
            Err(e) => eprintln!("joi daemon: reload failed: {e:#}"),
        }

        tokio::select! {
            _ = shutdown_signal() => break,
            _ = sleep(CONFIG_RELOAD_INTERVAL) => {}
        }
    }

    eprintln!("\njoi daemon: shutting down");
    proxy_handle.abort();
    machine_host_handle.abort();
    for (_, running) in running_agents {
        running.handle.abort();
    }
    daemon_ipc::remove_discovery_for(&socket_path);
    daemon_ipc::cleanup_socket(&socket_path).await;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => Some(signal),
                Err(err) => {
                    tracing::warn!(error = %err, "failed to install SIGTERM handler");
                    None
                }
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                if let Some(signal) = sigterm.as_mut() {
                    let _ = signal.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

struct MachineSpecs {
    specs: Vec<AgentSpec>,
}

struct RunningAgent {
    fingerprint: String,
    handle: tokio::task::JoinHandle<()>,
}

fn load_machine_specs(
    machine_id: Option<&str>,
    allow_actors: &[String],
    warned_missing: &mut HashSet<String>,
) -> Result<MachineSpecs> {
    let cfg = load_desktop_config().unwrap_or_default();
    let machine = select_machine(&cfg, machine_id)?;
    let providers = detect_agent_cli_providers();
    let definitions = machine
        .agents
        .iter()
        .map(machine_agent_definition)
        .collect::<Vec<_>>();
    warn_missing_providers_once(&definitions, &providers, warned_missing);

    let provider_specs = provider_specs_from_agent_definitions(&providers, &definitions);
    let mut specs = provider_specs
        .into_iter()
        .flat_map(|provider| provider.into_agent_specs())
        .collect::<Vec<_>>();

    if !allow_actors.is_empty() {
        let allow = allow_actors
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        specs.retain(|spec| allow.contains(spec.actor.id.as_str()));
    }

    Ok(MachineSpecs { specs })
}

fn reconcile_agents(
    running: &mut HashMap<String, RunningAgent>,
    specs: Vec<AgentSpec>,
    server_url: &str,
    data_root: &PathBuf,
) {
    let mut desired = HashMap::new();
    for spec in specs {
        desired.insert(spec.actor.id.clone(), (spec_fingerprint(&spec), spec));
    }

    let stale = running
        .keys()
        .filter(|actor_id| !desired.contains_key(*actor_id))
        .cloned()
        .collect::<Vec<_>>();
    for actor_id in stale {
        if let Some(agent) = running.remove(&actor_id) {
            agent.handle.abort();
            eprintln!("[{actor_id}] stopped: removed from machine config");
        }
    }

    for (actor_id, (fingerprint, spec)) in desired {
        if running
            .get(&actor_id)
            .map(|agent| agent.fingerprint.as_str() == fingerprint.as_str())
            .unwrap_or(false)
        {
            continue;
        }
        if let Some(agent) = running.remove(&actor_id) {
            agent.handle.abort();
            eprintln!("[{actor_id}] restarting: machine config changed");
        } else {
            eprintln!("[{actor_id}] starting from machine config");
        }
        let handle =
            agent_serve::spawn_agent_worker_loop(spec, server_url.to_string(), data_root.clone());
        running.insert(
            actor_id,
            RunningAgent {
                fingerprint,
                handle,
            },
        );
    }
}

fn spec_fingerprint(spec: &AgentSpec) -> String {
    serde_json::to_string(spec).unwrap_or_else(|_| format!("{spec:?}"))
}

fn spawn_service_host(
    services_dir: Option<PathBuf>,
    server_url: String,
    allow_services: Vec<String>,
) {
    tokio::spawn(async move {
        if let Err(e) = service::serve(services_dir, server_url, allow_services).await {
            eprintln!("joi daemon: service host exited with error: {e:#}");
        }
    });
}

fn print_providers(providers: &[DetectedAgentProvider]) {
    if render::is_json() {
        render::print_json(&json!({ "providers": providers }));
        return;
    }
    if providers.is_empty() {
        println!("No supported agent CLIs found on PATH.");
        return;
    }
    for provider in providers {
        let args = if provider.args.is_empty() {
            String::new()
        } else {
            format!(" {}", provider.args.join(" "))
        };
        println!(
            "{}\t{}\t{}{}",
            provider.id, provider.display_name, provider.command, args
        );
    }
}

fn warn_missing_providers_once(
    definitions: &[AgentDefinition],
    providers: &[DetectedAgentProvider],
    warned: &mut HashSet<String>,
) {
    let provider_ids = providers
        .iter()
        .map(|p| p.id.as_str())
        .collect::<HashSet<_>>();
    for definition in definitions {
        if !provider_ids.contains(definition.provider_id.as_str()) {
            let key = format!("{}:{}", definition.actor_id, definition.provider_id);
            if !warned.insert(key) {
                continue;
            }
            eprintln!(
                "joi daemon: skipping {} because provider `{}` is not available on PATH",
                definition.actor_id, definition.provider_id
            );
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct DesktopConfig {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    workspaces: Vec<WorkspaceConfig>,
    #[serde(default)]
    machines: Vec<MachineConfig>,
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
    id: String,
    name: String,
    #[serde(default)]
    data_root: String,
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
    let cfg = toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(cfg)
}

fn select_machine(cfg: &DesktopConfig, requested: Option<&str>) -> Result<MachineConfig> {
    if let Some(id) = requested.filter(|id| !id.trim().is_empty()) {
        return cfg
            .machines
            .iter()
            .find(|machine| machine.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown machine id: {id}"));
    }

    let active = active_workspace_id(cfg);
    cfg.machines
        .iter()
        .find(|machine| machine.workspace_id.as_deref() == active)
        .or_else(|| cfg.machines.first())
        .cloned()
        .or_else(|| {
            Some(MachineConfig {
                workspace_id: active.map(ToString::to_string),
                id: "local".into(),
                name: "Local Machine".into(),
                data_root: default_agent_data_root_expr(),
                agents: Vec::new(),
            })
        })
        .ok_or_else(|| anyhow!("no machine configured"))
}

fn active_workspace_id(cfg: &DesktopConfig) -> Option<&str> {
    cfg.active
        .as_deref()
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .or_else(|| cfg.workspaces.first())
        .map(|workspace| workspace.id.as_str())
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

fn machine_data_root(machine: &MachineConfig) -> PathBuf {
    if machine.data_root.trim().is_empty() {
        expand_home(&default_agent_data_root_expr())
    } else {
        expand_home(&machine.data_root)
    }
}

fn machine_connection_actor_id(machine: &MachineConfig) -> String {
    format!("actor_service_{}", machine.id)
}

fn default_agent_data_root_expr() -> String {
    "~/.agentx".into()
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
