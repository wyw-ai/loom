//! `joi daemon` — machine-scoped agent host.
//!
//! The daemon is the machine-scoped agent host: it reads the desktop machine
//! config, auto-detects supported local agent CLIs, synthesizes runtime
//! `AgentSpec`s in memory, and then runs the shared agent worker implementation.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_runtime::discovery::{
    apply_provider_overrides, detect_agent_cli_providers, provider_specs_from_agent_definitions,
    AgentDefinition, AgentProviderOverride, DetectedAgentProvider,
};
use anyhow::{anyhow, Context, Result};
use proto::methods::AgentSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
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
    no_ipc: bool,
    server_url: String,
) -> Result<()> {
    let detected_providers = detect_agent_cli_providers();
    if list_providers {
        print_providers(&detected_providers);
        return Ok(());
    }

    let cfg = load_desktop_config().unwrap_or_default();
    let machine = select_machine(&cfg, machine_id.as_deref())?;
    let providers = apply_provider_overrides(detected_providers, &machine.providers);
    let selected_machine_id = machine.id.clone();
    let data_root = data_root.unwrap_or_else(|| machine_data_root(&machine));
    std::fs::create_dir_all(&data_root)
        .with_context(|| format!("create data root {}", data_root.display()))?;
    std::env::set_var("JOI_AGENT_DATA_ROOT", &data_root);

    let mut inventory_revision = 1u64;
    let mut inventory_fingerprint = machine_inventory_fingerprint(&machine, &data_root, &providers);
    let machine_inventory = Arc::new(Mutex::new(machine_inventory_meta(
        &machine,
        &data_root,
        &providers,
        inventory_revision,
    )));
    let (machine_command_tx, mut machine_command_rx) = mpsc::unbounded_channel();
    let machine_host = MachineHostSpec {
        machine_id: machine.id.clone(),
        actor_id: machine_connection_actor_id(&machine),
        display_name: machine.name.clone(),
        metadata: machine_inventory.clone(),
        command_tx: Some(machine_command_tx),
    };

    let (socket_path, proxy_handle) = if no_ipc {
        (None, None)
    } else {
        let socket_path = socket_path
            .or_else(daemon_ipc::env_socket_path)
            .unwrap_or_else(daemon_ipc::default_socket_path);
        let proxy_handle = daemon_ipc::start_proxy(socket_path.clone(), server_url.clone()).await?;
        std::env::set_var(daemon_ipc::ENV_DAEMON_SOCKET, &socket_path);
        if let Err(err) = daemon_ipc::write_discovery(&socket_path, &server_url) {
            eprintln!("joi daemon: warning: failed to write daemon discovery: {err:#}");
        }
        (Some(socket_path), Some(proxy_handle))
    };

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
    if let Some(socket_path) = socket_path.as_ref() {
        eprintln!("joi daemon: socket={}", socket_path.display());
    } else {
        eprintln!("joi daemon: socket disabled");
    }
    eprintln!("joi daemon: ready (ctrl-c to stop)");

    loop {
        match refresh_machine_runtime(
            &selected_machine_id,
            &allow_actors,
            &data_root,
            &server_url,
            &machine_inventory,
            &mut inventory_revision,
            &mut inventory_fingerprint,
            &mut running_agents,
            &mut warned_missing,
        ) {
            Ok(()) => {}
            Err(e) => eprintln!("joi daemon: reload failed: {e:#}"),
        }

        tokio::select! {
            _ = shutdown_signal() => break,
            maybe_command = machine_command_rx.recv() => {
                let Some(command) = maybe_command else {
                    eprintln!("joi daemon: machine command channel closed");
                    continue;
                };
                let result = handle_machine_command(&selected_machine_id, &data_root, command.payload);
                if result.get("ok").and_then(Value::as_bool) == Some(true) {
                    if let Err(err) = refresh_machine_runtime(
                        &selected_machine_id,
                        &allow_actors,
                        &data_root,
                        &server_url,
                        &machine_inventory,
                        &mut inventory_revision,
                        &mut inventory_fingerprint,
                        &mut running_agents,
                        &mut warned_missing,
                    ) {
                        let fallback = machine_command_error_from_result(
                            &result,
                            format!("machine command applied but runtime refresh failed: {err:#}"),
                        );
                        let _ = command.reply.send(fallback);
                        continue;
                    }
                }
                let _ = command.reply.send(result);
            }
            _ = sleep(CONFIG_RELOAD_INTERVAL) => {}
        }
    }

    eprintln!("\njoi daemon: shutting down");
    if let Some(proxy_handle) = proxy_handle {
        proxy_handle.abort();
    }
    machine_host_handle.abort();
    for (_, running) in running_agents {
        running.handle.abort();
    }
    if let Some(socket_path) = socket_path {
        daemon_ipc::remove_discovery_for(&socket_path);
        daemon_ipc::cleanup_socket(&socket_path).await;
    }
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
    machine: MachineConfig,
    providers: Vec<DetectedAgentProvider>,
    specs: Vec<AgentSpec>,
}

struct RunningAgent {
    fingerprint: String,
    handle: tokio::task::JoinHandle<()>,
}

fn refresh_machine_runtime(
    selected_machine_id: &str,
    allow_actors: &[String],
    data_root: &PathBuf,
    server_url: &str,
    machine_inventory: &Arc<Mutex<Value>>,
    inventory_revision: &mut u64,
    inventory_fingerprint: &mut String,
    running_agents: &mut HashMap<String, RunningAgent>,
    warned_missing: &mut HashSet<String>,
) -> Result<()> {
    let snapshot = load_machine_specs(Some(selected_machine_id), allow_actors, warned_missing)?;
    let next_fingerprint =
        machine_inventory_fingerprint(&snapshot.machine, data_root, &snapshot.providers);
    if next_fingerprint != *inventory_fingerprint {
        *inventory_revision = inventory_revision.saturating_add(1);
        *inventory_fingerprint = next_fingerprint;
    }
    *machine_inventory.lock().unwrap() = machine_inventory_meta(
        &snapshot.machine,
        data_root,
        &snapshot.providers,
        *inventory_revision,
    );
    reconcile_agents(running_agents, snapshot.specs, server_url, data_root);
    Ok(())
}

fn load_machine_specs(
    machine_id: Option<&str>,
    allow_actors: &[String],
    warned_missing: &mut HashSet<String>,
) -> Result<MachineSpecs> {
    let cfg = load_desktop_config().unwrap_or_default();
    let machine = select_machine(&cfg, machine_id)?;
    let providers = apply_provider_overrides(detect_agent_cli_providers(), &machine.providers);
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

    Ok(MachineSpecs {
        machine,
        providers,
        specs,
    })
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

fn handle_machine_command(selected_machine_id: &str, data_root: &PathBuf, payload: Value) -> Value {
    let command_id = payload
        .get("commandId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let machine_id = payload
        .get("machineId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let machine_actor_id = payload
        .get("machineActorId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let result_prefix = || {
        json!({
            "commandId": command_id,
            "machineId": machine_id,
            "machineActorId": machine_actor_id,
        })
    };
    if machine_id != selected_machine_id {
        return machine_command_error(result_prefix(), "command targets a different machine");
    }
    let result = match apply_machine_command(selected_machine_id, data_root, &payload) {
        Ok(output) => {
            let mut result = result_prefix();
            result["ok"] = json!(true);
            result["output"] = output;
            result
        }
        Err(err) => machine_command_error(result_prefix(), format!("{err:#}")),
    };
    result
}

fn machine_command_error_from_result(result: &Value, error: impl Into<String>) -> Value {
    machine_command_error(
        json!({
            "commandId": result.get("commandId").cloned().unwrap_or(Value::Null),
            "machineId": result.get("machineId").cloned().unwrap_or(Value::Null),
            "machineActorId": result.get("machineActorId").cloned().unwrap_or(Value::Null),
        }),
        error,
    )
}

fn machine_command_error(mut result: Value, error: impl Into<String>) -> Value {
    let message = error.into();
    let code = if message.starts_with("profile_conflict:") {
        "profile_conflict"
    } else {
        "machine_command_failed"
    };
    result["ok"] = json!(false);
    result["error"] = json!(message.clone());
    result["structuredError"] = json!({
        "code": code,
        "message": message,
        "retryable": false,
    });
    result
}

fn apply_machine_command(
    selected_machine_id: &str,
    data_root: &PathBuf,
    payload: &Value,
) -> Result<Value> {
    let command = payload
        .get("command")
        .ok_or_else(|| anyhow!("missing command payload"))?;
    let op = command
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("machine command op is required"))?;
    let mut cfg = load_desktop_config()?;
    let machine_index = cfg
        .machines
        .iter()
        .position(|machine| machine.id == selected_machine_id)
        .ok_or_else(|| anyhow!("unknown machine id: {selected_machine_id}"))?;

    match op {
        "agent.create" => {
            let agent = agent_config_from_command(command, selected_machine_id)?;
            if cfg.machines[machine_index]
                .agents
                .iter()
                .any(|existing| existing.actor_id == agent.actor_id)
            {
                return Err(anyhow!("agent actor already exists: {}", agent.actor_id));
            }
            let providers = apply_provider_overrides(
                detect_agent_cli_providers(),
                &cfg.machines[machine_index].providers,
            );
            if !providers
                .iter()
                .any(|provider| provider.id == agent.provider_id)
            {
                return Err(anyhow!(
                    "provider `{}` is not available on the remote machine",
                    agent.provider_id
                ));
            }
            cfg.machines[machine_index].agents.push(agent.clone());
            save_desktop_config(&cfg)?;
            Ok(json!({ "agent": agent }))
        }
        "agent.remove" => {
            let actor_id = required_str(command, "actorId")?;
            let machine = &mut cfg.machines[machine_index];
            let before = machine.agents.len();
            machine.agents.retain(|agent| agent.actor_id != actor_id);
            if machine.agents.len() == before {
                return Err(anyhow!("daemon-configured agent not found: {actor_id}"));
            }
            save_desktop_config(&cfg)?;
            Ok(json!({ "actorId": actor_id }))
        }
        "agent.update" => {
            let actor_id = required_str(command, "actorId")?;
            let agent_index = cfg.machines[machine_index]
                .agents
                .iter()
                .position(|agent| agent.actor_id == actor_id)
                .ok_or_else(|| anyhow!("daemon-configured agent not found: {actor_id}"))?;
            let provider_id = optional_trimmed_str(command, "providerId");
            if let Some(value) = provider_id.as_deref().filter(|value| !value.is_empty()) {
                let providers = apply_provider_overrides(
                    detect_agent_cli_providers(),
                    &cfg.machines[machine_index].providers,
                );
                if !providers.iter().any(|provider| provider.id == value) {
                    return Err(anyhow!(
                        "provider `{}` is not available on the remote machine",
                        value
                    ));
                }
            }
            let agent = &mut cfg.machines[machine_index].agents[agent_index];
            if let Some(value) = optional_trimmed_str(command, "displayName") {
                if !value.is_empty() {
                    agent.name = value;
                }
            }
            if let Some(value) = optional_trimmed_str(command, "description") {
                agent.description = value;
            }
            if let Some(value) = provider_id {
                if !value.is_empty() {
                    agent.provider_id = value;
                }
            }
            if let Some(value) = optional_trimmed_str(command, "model") {
                agent.model = value;
            }
            if let Some(value) = optional_trimmed_str(command, "reasoningEffort") {
                agent.reasoning_effort = value;
            }
            if let Some(value) = command.get("autostart").and_then(Value::as_bool) {
                agent.autostart = value;
            }
            let updated = agent.clone();
            save_desktop_config(&cfg)?;
            Ok(json!({ "agent": updated }))
        }
        "agent.profile.read" => {
            let actor_id = required_str(command, "actorId")?;
            let file = required_str(command, "file")?;
            let path = resolve_machine_agent_profile_file(
                &cfg.machines[machine_index],
                data_root,
                actor_id,
                file,
            )?;
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let sha256 = sha256_text(&text);
            Ok(json!({
                "path": path.display().to_string(),
                "text": text,
                "sha256": sha256,
            }))
        }
        "agent.profile.write" => {
            let actor_id = required_str(command, "actorId")?;
            let file = required_str(command, "file")?;
            let text = command
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("text is required"))?;
            let base_sha256 = command
                .get("baseSha256")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("profile.write requires baseSha256"))?;
            let path = resolve_machine_agent_profile_file(
                &cfg.machines[machine_index],
                data_root,
                actor_id,
                file,
            )?;
            let current_text = std::fs::read_to_string(&path).unwrap_or_default();
            let current_sha256 = sha256_text(&current_text);
            if current_sha256 != base_sha256 {
                return Err(anyhow!(
                    "profile_conflict: current profile hash {current_sha256} does not match base {base_sha256}"
                ));
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create directory {}", parent.display()))?;
            }
            std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
            Ok(json!({
                "path": path.display().to_string(),
                "text": text,
                "sha256": sha256_text(text),
            }))
        }
        other => Err(anyhow!("unsupported machine command op: {other}")),
    }
}

fn agent_config_from_command(command: &Value, machine_id: &str) -> Result<MachineAgentConfig> {
    let name = required_str(command, "name")?.trim().to_string();
    if name.is_empty() {
        return Err(anyhow!("agent name is required"));
    }
    let actor_id = command
        .get("actorId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("actor_agent_{}_{}", slugify(&name), machine_id));
    Ok(MachineAgentConfig {
        provider_id: required_str(command, "providerId")?.to_string(),
        actor_id,
        name,
        description: command
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        model: command
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        reasoning_effort: command
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        autostart: command
            .get("autostart")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn resolve_machine_agent_profile_file(
    machine: &MachineConfig,
    data_root: &PathBuf,
    actor_id: &str,
    file: &str,
) -> Result<PathBuf> {
    if !machine
        .agents
        .iter()
        .any(|agent| agent.actor_id == actor_id)
    {
        return Err(anyhow!("unknown agent actor id: {actor_id}"));
    }
    let file_name = match file.trim() {
        "identity" => "identity.md",
        "soul" => "soul.md",
        other => return Err(anyhow!("unknown profile file: {other}")),
    };
    Ok(data_root
        .join("agents")
        .join(actor_id)
        .join("profile")
        .join(file_name))
}

fn required_str<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{field} is required"))
}

fn optional_trimmed_str(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .map(ToString::to_string)
}

fn sha256_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn slugify(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "agent".into()
    } else {
        slug.into()
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HumanAccount {
    #[serde(default)]
    actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceConfig {
    id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachineConfig {
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    owner_actor_id: Option<String>,
    id: String,
    name: String,
    #[serde(default = "default_machine_kind")]
    kind: String,
    #[serde(default)]
    data_root: String,
    #[serde(default)]
    providers: Vec<AgentProviderOverride>,
    #[serde(default)]
    agents: Vec<MachineAgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn machine_inventory_meta(
    machine: &MachineConfig,
    data_root: &PathBuf,
    providers: &[DetectedAgentProvider],
    revision: u64,
) -> serde_json::Value {
    json!({
        "role": "machine",
        "machineId": &machine.id,
        "inventoryVersion": 2,
        "source": "daemon",
        "revision": revision,
        "observedAt": chrono::Utc::now().to_rfc3339(),
        "workspaceId": &machine.workspace_id,
        "ownerActorId": &machine.owner_actor_id,
        "name": &machine.name,
        "kind": &machine.kind,
        "dataRoot": data_root.display().to_string(),
        "configDir": config::config_dir().display().to_string(),
        "capabilities": [
            "inventory.read",
            "connection.status",
            "machine.command",
            "agent.create",
            "agent.remove",
            "agent.profile.read",
            "agent.profile.write"
        ],
        "providers": providers,
        "agents": &machine.agents,
    })
}

fn machine_inventory_fingerprint(
    machine: &MachineConfig,
    data_root: &PathBuf,
    providers: &[DetectedAgentProvider],
) -> String {
    serde_json::to_string(&json!({
        "machineId": &machine.id,
        "workspaceId": &machine.workspace_id,
        "ownerActorId": &machine.owner_actor_id,
        "name": &machine.name,
        "kind": &machine.kind,
        "dataRoot": data_root.display().to_string(),
        "configDir": config::config_dir().display().to_string(),
        "providers": providers,
        "agents": &machine.agents,
    }))
    .unwrap_or_default()
}

fn default_machine_kind() -> String {
    "local".into()
}

fn load_desktop_config() -> Result<DesktopConfig> {
    let path = config::config_dir().join("desktop.toml");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read desktop config {}", path.display()))?;
    let cfg = toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(cfg)
}

fn save_desktop_config(cfg: &DesktopConfig) -> Result<()> {
    let path = config::config_dir().join("desktop.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, text).with_context(|| format!("write desktop config {}", path.display()))
}

fn select_machine(cfg: &DesktopConfig, requested: Option<&str>) -> Result<MachineConfig> {
    let active = active_workspace_id(cfg);
    let owner = active_account_actor_id(cfg);

    if let Some(id) = requested.filter(|id| !id.trim().is_empty()) {
        let machine = cfg
            .machines
            .iter()
            .find(|machine| machine.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown machine id: {id}"));
        let machine = machine?;
        if !machine_belongs_to_owner(&machine, owner) {
            return Err(anyhow!(
                "machine id {id} does not belong to the active account"
            ));
        }
        return Ok(machine);
    }

    if let Some(machine) = cfg
        .machines
        .iter()
        .find(|machine| machine_belongs_to_workspace_and_owner(machine, active, owner))
    {
        return Ok(machine.clone());
    }

    if owner.is_some() {
        return Err(anyhow!(
            "no machine configured for the active account; open the GUI Computers page to create one"
        ));
    }

    cfg.machines
        .iter()
        .find(|machine| {
            machine.workspace_id.as_deref() == active && machine.owner_actor_id.is_none()
        })
        .or_else(|| {
            cfg.machines
                .iter()
                .find(|machine| machine.owner_actor_id.is_none())
        })
        .cloned()
        .or_else(|| {
            Some(MachineConfig {
                workspace_id: active.map(ToString::to_string),
                owner_actor_id: None,
                id: "local".into(),
                name: "Local Machine".into(),
                kind: default_machine_kind(),
                data_root: default_agent_data_root_expr(),
                providers: Vec::new(),
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

fn active_account_actor_id(cfg: &DesktopConfig) -> Option<&str> {
    cfg.account
        .as_ref()
        .map(|account| account.actor_id.trim())
        .filter(|actor_id| !actor_id.is_empty())
}

fn machine_belongs_to_workspace_and_owner(
    machine: &MachineConfig,
    workspace_id: Option<&str>,
    owner_actor_id: Option<&str>,
) -> bool {
    machine.workspace_id.as_deref() == workspace_id
        && machine_belongs_to_owner(machine, owner_actor_id)
}

fn machine_belongs_to_owner(machine: &MachineConfig, owner_actor_id: Option<&str>) -> bool {
    machine.owner_actor_id.as_deref() == owner_actor_id
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_owner(owner: &str, machines: Vec<MachineConfig>) -> DesktopConfig {
        DesktopConfig {
            active: Some("ws_main".into()),
            account: Some(HumanAccount {
                actor_id: owner.into(),
            }),
            workspaces: vec![WorkspaceConfig {
                id: "ws_main".into(),
            }],
            machines,
        }
    }

    fn machine(id: &str, owner: Option<&str>) -> MachineConfig {
        MachineConfig {
            workspace_id: Some("ws_main".into()),
            owner_actor_id: owner.map(ToString::to_string),
            id: id.into(),
            name: id.into(),
            kind: default_machine_kind(),
            data_root: String::new(),
            providers: Vec::new(),
            agents: Vec::new(),
        }
    }

    #[test]
    fn select_machine_uses_active_account_owner() {
        let cfg = cfg_with_owner(
            "actor_human_1",
            vec![
                machine("machine_other", Some("actor_human_2")),
                machine("machine_mine", Some("actor_human_1")),
            ],
        );

        let selected = select_machine(&cfg, None).expect("select machine");

        assert_eq!(selected.id, "machine_mine");
    }

    #[test]
    fn select_machine_rejects_requested_machine_for_other_owner() {
        let cfg = cfg_with_owner(
            "actor_human_1",
            vec![machine("machine_other", Some("actor_human_2"))],
        );

        let err = select_machine(&cfg, Some("machine_other")).expect_err("owner mismatch");

        assert!(err.to_string().contains("active account"));
    }
}
