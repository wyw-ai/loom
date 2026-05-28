//! `loom-daemon` — machine-scoped agent host.
//!
//! The daemon is the machine-scoped agent host: it reads the desktop machine
//! config, auto-detects supported local agent CLIs, synthesizes runtime
//! `AgentSpec`s in memory, and then runs the shared agent worker implementation.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_runtime::discovery::{
    apply_provider_overrides, detect_agent_cli_providers, provider_specs_from_agent_definitions,
    AgentDefinition, AgentProviderOverride, DetectedAgentProvider,
};
use anyhow::{anyhow, Context, Result};
use proto::methods::{method, AgentModelSpec, AgentProviderRef, AgentSpec, AgentTransport};
use proto::types::{Actor, ActorKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use crate::client::Client;
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

    let mut cfg = load_desktop_config_or_default_if_missing()?;
    let repaired_desktop_config = repair_desktop_config_for_daemon(&mut cfg, &server_url);
    let (machine, restored_machine_config) = select_machine_for_daemon(
        &mut cfg,
        machine_id.as_deref(),
        data_root.as_ref(),
        &server_url,
    )
    .await?;
    let providers = apply_provider_overrides(detected_providers, &machine.providers);
    let migrated_legacy_agents =
        migrate_legacy_machine_agents_to_specs(&mut cfg, &machine.id, &providers)?;
    if repaired_desktop_config || restored_machine_config || migrated_legacy_agents {
        save_desktop_config(&cfg)?;
    }
    let machine = cfg
        .machines
        .iter()
        .find(|candidate| candidate.id == machine.id)
        .cloned()
        .unwrap_or(machine);
    let selected_machine_id = machine.id.clone();
    let mut selected_machine = machine.clone();
    let data_root = data_root.unwrap_or_else(|| machine_data_root(&machine));
    std::fs::create_dir_all(&data_root)
        .with_context(|| format!("create data root {}", data_root.display()))?;
    std::env::set_var("LOOM_AGENT_DATA_ROOT", &data_root);

    let mut initial_specs = load_config_agent_specs()?;
    annotate_machine_agent_specs(&mut initial_specs, &machine);
    if !allow_actors.is_empty() {
        let allow = allow_actors
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        initial_specs.retain(|spec| allow.contains(spec.actor.id.as_str()));
    }
    let mut inventory_revision = 1u64;
    let mut inventory_fingerprint =
        machine_inventory_fingerprint(&machine, &data_root, &providers, &initial_specs);
    let machine_inventory = Arc::new(Mutex::new(machine_inventory_meta(
        &machine,
        &data_root,
        &providers,
        &initial_specs,
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
            eprintln!("loom-daemon: warning: failed to write daemon discovery: {err:#}");
        }
        (Some(socket_path), Some(proxy_handle))
    };

    if no_services {
        eprintln!("loom-daemon: service host disabled by --no-services");
    } else {
        spawn_service_host(services_dir, server_url.clone(), allow_services);
    }

    let machine_host_handle =
        agent_serve::spawn_machine_host_loop(machine_host, server_url.clone());
    let mut running_agents = HashMap::new();
    let mut warned_missing = HashSet::new();

    eprintln!(
        "loom-daemon: machine={} providers={} data={} reload={}s",
        machine.id,
        providers.len(),
        data_root.display(),
        CONFIG_RELOAD_INTERVAL.as_secs()
    );
    if let Some(socket_path) = socket_path.as_ref() {
        eprintln!("loom-daemon: socket={}", socket_path.display());
    } else {
        eprintln!("loom-daemon: socket disabled");
    }
    eprintln!("loom-daemon: ready (ctrl-c to stop)");

    loop {
        match refresh_machine_runtime(
            &selected_machine_id,
            &mut selected_machine,
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
            Err(e) => eprintln!("loom-daemon: reload failed: {e:#}"),
        }

        tokio::select! {
            _ = shutdown_signal() => break,
            maybe_command = machine_command_rx.recv() => {
                let Some(command) = maybe_command else {
                    eprintln!("loom-daemon: machine command channel closed");
                    continue;
                };
                let result = handle_machine_command(
                    &selected_machine_id,
                    &mut selected_machine,
                    &data_root,
                    command.payload,
                );
                if result.get("ok").and_then(Value::as_bool) == Some(true) {
                    if let Err(err) = refresh_machine_runtime(
                        &selected_machine_id,
                        &mut selected_machine,
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

    eprintln!("\nloom-daemon: shutting down");
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
    selected_machine: &mut MachineConfig,
    allow_actors: &[String],
    data_root: &PathBuf,
    server_url: &str,
    machine_inventory: &Arc<Mutex<Value>>,
    inventory_revision: &mut u64,
    inventory_fingerprint: &mut String,
    running_agents: &mut HashMap<String, RunningAgent>,
    warned_missing: &mut HashSet<String>,
) -> Result<()> {
    let snapshot = load_machine_specs(
        selected_machine_id,
        selected_machine,
        allow_actors,
        warned_missing,
        server_url,
    )?;
    *selected_machine = snapshot.machine.clone();
    let next_fingerprint = machine_inventory_fingerprint(
        &snapshot.machine,
        data_root,
        &snapshot.providers,
        &snapshot.specs,
    );
    if next_fingerprint != *inventory_fingerprint {
        *inventory_revision = inventory_revision.saturating_add(1);
        *inventory_fingerprint = next_fingerprint;
    }
    *machine_inventory.lock().unwrap() = machine_inventory_meta(
        &snapshot.machine,
        data_root,
        &snapshot.providers,
        &snapshot.specs,
        *inventory_revision,
    );
    reconcile_agents(running_agents, snapshot.specs, server_url, data_root);
    Ok(())
}

fn load_machine_specs(
    selected_machine_id: &str,
    selected_machine: &MachineConfig,
    allow_actors: &[String],
    warned_missing: &mut HashSet<String>,
    server_url: &str,
) -> Result<MachineSpecs> {
    let mut cfg = load_desktop_config_or_default_if_missing()?;
    let repaired = repair_desktop_config_for_daemon(&mut cfg, server_url);
    let was_missing = !cfg
        .machines
        .iter()
        .any(|machine| machine.id == selected_machine_id);
    let restored_context = if was_missing {
        if warned_missing.insert(format!("machine:{selected_machine_id}")) {
            eprintln!(
                "loom-daemon: selected machine {selected_machine_id} is missing from desktop.toml; restoring live runtime snapshot"
            );
        }
        restore_selected_machine_context(&mut cfg, selected_machine, server_url, true)
    } else {
        false
    };
    let (machine_index, restored) =
        ensure_selected_machine_config(&mut cfg, selected_machine_id, selected_machine)?;
    let providers = apply_provider_overrides(
        detect_agent_cli_providers(),
        &cfg.machines[machine_index].providers,
    );
    let migrated_legacy_agents =
        migrate_legacy_machine_agents_at_index_to_specs(&mut cfg, machine_index, &providers)?;
    if repaired || restored_context || restored || migrated_legacy_agents {
        save_desktop_config(&cfg)?;
    }
    let machine = cfg.machines[machine_index].clone();
    let mut specs = load_config_agent_specs()?;
    annotate_machine_agent_specs(&mut specs, &machine);

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

fn load_config_agent_specs() -> Result<Vec<AgentSpec>> {
    let dir = crate::config::config_dir().join("agents");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    agent_serve::load_specs(&dir).with_context(|| format!("load AgentSpecs from {}", dir.display()))
}

fn migrate_legacy_machine_agents_to_specs(
    cfg: &mut DesktopConfig,
    machine_id: &str,
    providers: &[DetectedAgentProvider],
) -> Result<bool> {
    let Some(machine_index) = cfg
        .machines
        .iter()
        .position(|machine| machine.id == machine_id)
    else {
        return Ok(false);
    };
    migrate_legacy_machine_agents_at_index_to_specs(cfg, machine_index, providers)
}

fn migrate_legacy_machine_agents_at_index_to_specs(
    cfg: &mut DesktopConfig,
    machine_index: usize,
    providers: &[DetectedAgentProvider],
) -> Result<bool> {
    let legacy_agents = cfg
        .machines
        .get(machine_index)
        .map(|machine| machine.agents.clone())
        .unwrap_or_default();
    if legacy_agents.is_empty() {
        return Ok(false);
    }

    for agent in &legacy_agents {
        if load_config_agent_spec(&agent.actor_id)?.is_some() {
            continue;
        }
        let spec = agent_spec_from_machine_agent_config(agent, providers);
        write_config_agent_spec(&spec)?;
    }
    cfg.machines[machine_index].agents.clear();
    Ok(true)
}

fn agent_specs_dir() -> PathBuf {
    crate::config::config_dir().join("agents")
}

fn agent_spec_path(actor_id: &str) -> PathBuf {
    agent_specs_dir().join(actor_id).join("spec.json")
}

fn flat_agent_spec_path(actor_id: &str) -> PathBuf {
    agent_specs_dir().join(format!("{actor_id}.json"))
}

fn load_config_agent_spec(actor_id: &str) -> Result<Option<AgentSpec>> {
    let nested = agent_spec_path(actor_id);
    let flat = flat_agent_spec_path(actor_id);
    let path = if nested.exists() {
        nested
    } else if flat.exists() {
        flat
    } else {
        return Ok(None);
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let spec: AgentSpec =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(spec))
}

fn write_config_agent_spec(spec: &AgentSpec) -> Result<PathBuf> {
    let path = agent_spec_path(&spec.actor.id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create agent spec dir {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(spec)?;
    std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn remove_config_agent_spec(actor_id: &str) -> Result<bool> {
    let mut removed = false;
    let nested = agent_spec_path(actor_id);
    if nested.exists() {
        std::fs::remove_file(&nested).with_context(|| format!("remove {}", nested.display()))?;
        removed = true;
        if let Some(parent) = nested.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
    let flat = flat_agent_spec_path(actor_id);
    if flat.exists() {
        std::fs::remove_file(&flat).with_context(|| format!("remove {}", flat.display()))?;
        removed = true;
    }
    Ok(removed)
}

fn agent_spec_from_command(
    command: &Value,
    machine_id: &str,
    providers: &[DetectedAgentProvider],
) -> Result<AgentSpec> {
    let name = required_str(command, "name")?.trim().to_string();
    if name.is_empty() {
        return Err(anyhow!("agent name is required"));
    }
    let provider_id = required_str(command, "providerId")?.to_string();
    let provider = providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| anyhow!("provider `{provider_id}` is not available on this machine"))?;
    let actor_id = command
        .get("actorId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("actor_agent_{}_{}", slugify(&name), machine_id));
    let description = command
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let model = command
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let reasoning_effort = command
        .get("reasoningEffort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let avatar_url = command
        .get("avatarUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let mut meta = BTreeMap::new();
    meta.insert("providerId".into(), json!(provider.id.clone()));
    meta.insert("providerName".into(), json!(provider.display_name.clone()));
    meta.insert(
        "transportKind".into(),
        json!(provider.transport_kind.clone()),
    );
    meta.insert("createdBy".into(), json!("loom-daemon"));
    if let Some(reasoning_effort) = reasoning_effort.as_ref() {
        meta.insert("reasoningEffort".into(), json!(reasoning_effort));
    }
    if let Some(description) = description.as_ref() {
        meta.insert("description".into(), json!(description));
    }
    if let Some(avatar_url) = avatar_url.as_ref() {
        meta.insert("avatarUrl".into(), json!(avatar_url));
    }

    Ok(AgentSpec {
        actor: Actor {
            id: actor_id,
            kind: ActorKind::Agent,
            display_name: name,
            capabilities: None,
            _meta: Some(meta),
        },
        provider_ref: Some(AgentProviderRef {
            id: provider.id.clone(),
            mode: Some("print".into()),
            model: model.clone(),
            reasoning_effort,
        }),
        transport: AgentTransport::default(),
        autostart: command
            .get("autostart")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        models: Some(AgentModelSpec {
            default: model.or_else(|| provider.default_model.clone()),
            choices: provider.model_choices.clone(),
        }),
        bundle: None,
        identity: None,
        memory: None,
        announcement: None,
        trigger: None,
        prompt_template: None,
    })
}

fn update_agent_spec_from_command(
    mut spec: AgentSpec,
    command: &Value,
    providers: &[DetectedAgentProvider],
) -> Result<AgentSpec> {
    let mut selected_provider = spec.provider_ref.as_ref().and_then(|provider_ref| {
        providers
            .iter()
            .find(|provider| provider.id == provider_ref.id)
    });
    if let Some(provider_id) =
        optional_trimmed_str(command, "providerId").filter(|value| !value.is_empty())
    {
        let provider = providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| anyhow!("provider `{provider_id}` is not available on this machine"))?;
        let provider_ref = spec
            .provider_ref
            .get_or_insert_with(AgentProviderRef::default);
        provider_ref.id = provider.id.clone();
        provider_ref.mode.get_or_insert_with(|| "print".into());
        selected_provider = Some(provider);
    }
    if let Some(display_name) =
        optional_trimmed_str(command, "displayName").filter(|value| !value.is_empty())
    {
        spec.actor.display_name = display_name;
    }
    if let Some(description) = optional_trimmed_str(command, "description") {
        let meta = spec.actor._meta.get_or_insert_with(Default::default);
        if description.trim().is_empty() {
            meta.remove("description");
        } else {
            meta.insert("description".into(), json!(description));
        }
    }
    if let Some(model) = optional_trimmed_str(command, "model") {
        let model = if model.trim().is_empty() {
            None
        } else {
            Some(model)
        };
        let provider_ref = spec
            .provider_ref
            .get_or_insert_with(AgentProviderRef::default);
        provider_ref.model = model.clone();
        if let Some(models) = spec.models.as_mut() {
            models.default =
                model.or_else(|| selected_provider.and_then(|p| p.default_model.clone()));
        }
    }
    if let Some(reasoning_effort) = optional_trimmed_str(command, "reasoningEffort") {
        let reasoning_effort = if reasoning_effort.trim().is_empty() {
            None
        } else {
            Some(reasoning_effort)
        };
        let provider_ref = spec
            .provider_ref
            .get_or_insert_with(AgentProviderRef::default);
        provider_ref.reasoning_effort = reasoning_effort.clone();
        let meta = spec.actor._meta.get_or_insert_with(Default::default);
        if let Some(reasoning_effort) = reasoning_effort {
            meta.insert("reasoningEffort".into(), json!(reasoning_effort));
        } else {
            meta.remove("reasoningEffort");
        }
    }
    if let Some(value) = command.get("autostart").and_then(Value::as_bool) {
        spec.autostart = value;
    }
    if let Some(avatar_url) = optional_trimmed_str(command, "avatarUrl") {
        let meta = spec.actor._meta.get_or_insert_with(Default::default);
        if avatar_url.trim().is_empty() {
            meta.remove("avatarUrl");
        } else {
            meta.insert("avatarUrl".into(), json!(avatar_url));
        }
    }
    if let Some(provider) = selected_provider {
        let meta = spec.actor._meta.get_or_insert_with(Default::default);
        meta.insert("providerId".into(), json!(provider.id.clone()));
        meta.insert("providerName".into(), json!(provider.display_name.clone()));
        meta.insert(
            "transportKind".into(),
            json!(provider.transport_kind.clone()),
        );
        spec.models = Some(AgentModelSpec {
            default: spec
                .provider_ref
                .as_ref()
                .and_then(|provider_ref| provider_ref.model.clone())
                .or_else(|| provider.default_model.clone()),
            choices: provider.model_choices.clone(),
        });
    }
    Ok(spec)
}

fn agent_spec_from_machine_agent_config(
    agent: &MachineAgentConfig,
    providers: &[DetectedAgentProvider],
) -> AgentSpec {
    let definition = machine_agent_definition(agent);
    if let Some(provider) = providers
        .iter()
        .find(|provider| provider.id == definition.provider_id)
    {
        if let Some(spec) =
            provider_specs_from_agent_definitions(&[provider.clone()], &[definition.clone()])
                .into_iter()
                .flat_map(|provider| provider.into_agent_specs())
                .next()
        {
            return spec;
        }
    }
    agent_spec_from_definition_without_detected_provider(&definition)
}

fn agent_spec_from_definition_without_detected_provider(definition: &AgentDefinition) -> AgentSpec {
    let mut meta = BTreeMap::new();
    meta.insert("providerId".into(), json!(definition.provider_id.clone()));
    meta.insert("providerName".into(), json!(definition.provider_id.clone()));
    meta.insert("createdBy".into(), json!("loom-daemon"));
    meta.insert("migratedFrom".into(), json!("machine.agents"));
    if let Some(reasoning_effort) = definition.reasoning_effort.as_ref() {
        meta.insert("reasoningEffort".into(), json!(reasoning_effort));
    }
    if let Some(description) = definition.description.as_ref() {
        meta.insert("description".into(), json!(description));
    }
    if let Some(avatar_url) = definition.avatar_url.as_ref() {
        meta.insert("avatarUrl".into(), json!(avatar_url));
    }

    AgentSpec {
        actor: Actor {
            id: definition.actor_id.clone(),
            kind: ActorKind::Agent,
            display_name: definition.display_name.clone(),
            capabilities: None,
            _meta: Some(meta),
        },
        provider_ref: Some(AgentProviderRef {
            id: definition.provider_id.clone(),
            mode: Some("print".into()),
            model: definition.model.clone(),
            reasoning_effort: definition.reasoning_effort.clone(),
        }),
        transport: AgentTransport::default(),
        autostart: definition.autostart,
        models: Some(AgentModelSpec {
            default: definition.model.clone(),
            choices: Vec::new(),
        }),
        bundle: None,
        identity: None,
        memory: None,
        announcement: None,
        trigger: None,
        prompt_template: None,
    }
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

fn annotate_machine_agent_specs(specs: &mut [AgentSpec], machine: &MachineConfig) {
    for spec in specs {
        let meta = spec.actor._meta.get_or_insert_with(Default::default);
        meta.insert("machineId".into(), json!(machine.id.clone()));
        if let Some(workspace_id) = machine.workspace_id.as_deref() {
            meta.insert("workspaceId".into(), json!(workspace_id));
        }
        if let Some(owner_actor_id) = machine.owner_actor_id.as_deref() {
            meta.insert("ownerActorId".into(), json!(owner_actor_id));
        }
    }
}

fn spawn_service_host(
    services_dir: Option<PathBuf>,
    server_url: String,
    allow_services: Vec<String>,
) {
    tokio::spawn(async move {
        if let Err(e) = service::serve(services_dir, server_url, allow_services).await {
            eprintln!("loom-daemon: service host exited with error: {e:#}");
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

fn handle_machine_command(
    selected_machine_id: &str,
    selected_machine: &mut MachineConfig,
    data_root: &PathBuf,
    payload: Value,
) -> Value {
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
    let result =
        match apply_machine_command(selected_machine_id, selected_machine, data_root, &payload) {
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
    selected_machine: &mut MachineConfig,
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
    let mut cfg = load_desktop_config_or_default_if_missing()?;
    let (machine_index, config_changed) =
        ensure_selected_machine_config(&mut cfg, selected_machine_id, selected_machine)?;
    if config_changed {
        save_desktop_config(&cfg)?;
        *selected_machine = cfg.machines[machine_index].clone();
    }

    match op {
        "agent.create" => {
            let providers = apply_provider_overrides(
                detect_agent_cli_providers(),
                &cfg.machines[machine_index].providers,
            );
            let spec = agent_spec_from_command(command, selected_machine_id, &providers)?;
            if cfg.machines[machine_index]
                .agents
                .iter()
                .any(|existing| existing.actor_id == spec.actor.id)
                || load_config_agent_spec(&spec.actor.id)?.is_some()
            {
                return Err(anyhow!("agent actor already exists: {}", spec.actor.id));
            }
            let path = write_config_agent_spec(&spec)?;
            Ok(json!({ "agentSpec": spec, "path": path.display().to_string() }))
        }
        "agent.remove" => {
            let actor_id = required_str(command, "actorId")?;
            let machine = &mut cfg.machines[machine_index];
            let before = machine.agents.len();
            machine.agents.retain(|agent| agent.actor_id != actor_id);
            let removed_spec = remove_config_agent_spec(actor_id)?;
            if machine.agents.len() == before && !removed_spec {
                return Err(anyhow!("daemon-configured agent not found: {actor_id}"));
            }
            if machine.agents.len() != before {
                save_desktop_config(&cfg)?;
            }
            *selected_machine = cfg.machines[machine_index].clone();
            Ok(json!({ "actorId": actor_id }))
        }
        "agent.update" => {
            let actor_id = required_str(command, "actorId")?;
            let providers = apply_provider_overrides(
                detect_agent_cli_providers(),
                &cfg.machines[machine_index].providers,
            );
            if let Some(spec) = load_config_agent_spec(actor_id)? {
                let spec = update_agent_spec_from_command(spec, command, &providers)?;
                let path = write_config_agent_spec(&spec)?;
                return Ok(json!({ "agentSpec": spec, "path": path.display().to_string() }));
            }
            let agent_index = cfg.machines[machine_index]
                .agents
                .iter()
                .position(|agent| agent.actor_id == actor_id)
                .ok_or_else(|| anyhow!("daemon-configured agent not found: {actor_id}"))?;
            let legacy = cfg.machines[machine_index].agents[agent_index].clone();
            let spec = agent_spec_from_machine_agent_config(&legacy, &providers);
            let spec = update_agent_spec_from_command(spec, command, &providers)?;
            let path = write_config_agent_spec(&spec)?;
            cfg.machines[machine_index].agents.remove(agent_index);
            save_desktop_config(&cfg)?;
            *selected_machine = cfg.machines[machine_index].clone();
            Ok(json!({
                "agentSpec": spec,
                "path": path.display().to_string(),
                "migratedFrom": "machine.agents"
            }))
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
            *selected_machine = cfg.machines[machine_index].clone();
            Ok(json!({
                "path": path.display().to_string(),
                "text": text,
                "sha256": sha256_text(text),
            }))
        }
        other => Err(anyhow!("unsupported machine command op: {other}")),
    }
}

fn ensure_selected_machine_config(
    cfg: &mut DesktopConfig,
    selected_machine_id: &str,
    selected_machine: &MachineConfig,
) -> Result<(usize, bool)> {
    if let Some(index) = cfg
        .machines
        .iter()
        .position(|machine| machine.id == selected_machine_id)
    {
        let changed = if selected_machine.id == selected_machine_id {
            fill_missing_machine_context(&mut cfg.machines[index], selected_machine)
        } else {
            false
        };
        return Ok((index, changed));
    }
    if selected_machine.id != selected_machine_id {
        return Err(anyhow!(
            "selected machine cache mismatch: expected {selected_machine_id}, got {}",
            selected_machine.id
        ));
    }
    cfg.machines.push(machine_host_config(selected_machine));
    Ok((cfg.machines.len() - 1, true))
}

fn machine_host_config(machine: &MachineConfig) -> MachineConfig {
    let mut machine = machine.clone();
    machine.agents.clear();
    machine
}

fn resolve_machine_agent_profile_file(
    _machine: &MachineConfig,
    data_root: &PathBuf,
    actor_id: &str,
    file: &str,
) -> Result<PathBuf> {
    if load_config_agent_spec(actor_id)?.is_none() {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HumanAccount {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    staff_id: String,
    #[serde(default)]
    nickname: String,
    #[serde(default)]
    real_name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    actor_id: String,
    #[serde(default)]
    avatar_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceConfig {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    server_url: String,
    #[serde(default)]
    actor_id: String,
    #[serde(default)]
    display_name: String,
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
    #[serde(default)]
    avatar_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerMachineInventory {
    role: String,
    source: String,
    inventory_version: u64,
    machine_id: String,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    owner_actor_id: Option<String>,
    name: String,
    kind: String,
    data_root: String,
}

impl ServerMachineInventory {
    fn is_valid(&self) -> bool {
        self.role == "machine"
            && self.source == "daemon"
            && self.inventory_version == 2
            && !self.machine_id.trim().is_empty()
            && !self.name.trim().is_empty()
            && !self.kind.trim().is_empty()
            && !self.data_root.trim().is_empty()
    }
}

fn machine_inventory_meta(
    machine: &MachineConfig,
    data_root: &PathBuf,
    providers: &[DetectedAgentProvider],
    specs: &[AgentSpec],
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
        "agentSpecs": specs,
    })
}

fn machine_inventory_fingerprint(
    machine: &MachineConfig,
    data_root: &PathBuf,
    providers: &[DetectedAgentProvider],
    specs: &[AgentSpec],
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
        "agentSpecs": specs,
    }))
    .unwrap_or_default()
}

fn default_machine_kind() -> String {
    "local".into()
}

fn load_desktop_config_or_default_if_missing() -> Result<DesktopConfig> {
    let path = config::config_dir().join("desktop.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("parse {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DesktopConfig::default()),
        Err(err) => Err(err).with_context(|| format!("read desktop config {}", path.display())),
    }
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

fn repair_desktop_config_for_daemon(cfg: &mut DesktopConfig, server_url: &str) -> bool {
    let mut changed = false;
    if cfg.active.is_none() {
        if let Some(workspace) = cfg.workspaces.first() {
            cfg.active = Some(workspace.id.clone());
            changed = true;
        }
    }

    let account_actor_id = active_account_actor_id(cfg).map(ToString::to_string);
    let account_display_name = cfg.account.as_ref().map(account_display_name);
    for workspace in &mut cfg.workspaces {
        if workspace.name.trim().is_empty() {
            workspace.name = "Local".into();
            changed = true;
        }
        if workspace.server_url.trim().is_empty() {
            workspace.server_url = server_url.to_string();
            changed = true;
        }
        if workspace.actor_id.trim().is_empty() {
            if let Some(actor_id) = account_actor_id.as_ref() {
                workspace.actor_id = actor_id.clone();
                changed = true;
            }
        }
        if workspace.display_name.trim().is_empty() {
            if let Some(display_name) = account_display_name.as_ref() {
                workspace.display_name = display_name.clone();
                changed = true;
            } else if !workspace.actor_id.trim().is_empty() {
                workspace.display_name = workspace.actor_id.clone();
                changed = true;
            }
        }
    }
    changed
}

fn restore_selected_machine_context(
    cfg: &mut DesktopConfig,
    selected_machine: &MachineConfig,
    server_url: &str,
    force_active: bool,
) -> bool {
    let mut changed = false;
    let workspace_id = selected_machine
        .workspace_id
        .as_deref()
        .and_then(trimmed_non_empty);
    let owner_actor_id = selected_machine
        .owner_actor_id
        .as_deref()
        .and_then(trimmed_non_empty);

    if let Some(owner_actor_id) = owner_actor_id {
        match cfg.account.as_mut() {
            Some(account) if account.actor_id.trim() == owner_actor_id => {
                changed |= account.fill_missing_from_actor_id(owner_actor_id);
            }
            _ => {
                cfg.account = Some(HumanAccount::from_actor_id(owner_actor_id));
                changed = true;
            }
        }
    }

    if let Some(workspace_id) = workspace_id {
        let workspace_owner = owner_actor_id
            .or_else(|| active_account_actor_id(cfg))
            .map(ToString::to_string)
            .unwrap_or_default();
        if let Some(workspace) = cfg
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == workspace_id)
        {
            changed |= workspace.fill_missing_context(server_url, &workspace_owner);
        } else {
            cfg.workspaces.push(WorkspaceConfig::from_context(
                workspace_id,
                server_url,
                &workspace_owner,
            ));
            changed = true;
        }

        if force_active
            && cfg.active.as_deref() != Some(workspace_id)
            && active_workspace_can_be_recovered(cfg)
        {
            cfg.active = Some(workspace_id.to_string());
            changed = true;
        }
    }

    if let Some(machine) = cfg
        .machines
        .iter_mut()
        .find(|machine| machine.id == selected_machine.id)
    {
        changed |= fill_missing_machine_context(machine, selected_machine);
    } else {
        cfg.machines.push(machine_host_config(selected_machine));
        changed = true;
    }

    changed
}

async fn select_machine_for_daemon(
    cfg: &mut DesktopConfig,
    requested: Option<&str>,
    data_root: Option<&PathBuf>,
    server_url: &str,
) -> Result<(MachineConfig, bool)> {
    let Some(id) = requested.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok((select_machine(cfg, None)?, false));
    };

    if let Some(machine) = select_requested_machine(cfg, id)? {
        return Ok((machine, false));
    }

    match recover_machine_from_server_inventory(
        server_url,
        cfg,
        id,
        data_root.map(PathBuf::as_path),
    )
    .await
    {
        Ok(Some(machine)) => {
            let restored = restore_selected_machine_context(cfg, &machine, server_url, true);
            return Ok((machine, restored));
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!(
                "loom-daemon: warning: failed to recover machine config from server: {err:#}"
            );
        }
    }

    let Some(data_root) = data_root else {
        return Err(anyhow!("unknown machine id: {id}"));
    };
    let machine = synthesize_requested_machine(cfg, id, data_root);
    let restored = restore_selected_machine_context(cfg, &machine, server_url, true);
    Ok((machine, restored))
}

fn select_machine(cfg: &DesktopConfig, requested: Option<&str>) -> Result<MachineConfig> {
    let active = active_workspace_id(cfg);
    let owner = active_account_actor_id(cfg);

    if let Some(id) = requested.filter(|id| !id.trim().is_empty()) {
        return select_requested_machine(cfg, id)?
            .ok_or_else(|| anyhow!("unknown machine id: {id}"));
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

fn select_requested_machine(cfg: &DesktopConfig, id: &str) -> Result<Option<MachineConfig>> {
    let owner = active_account_actor_id(cfg);
    let Some(machine) = cfg.machines.iter().find(|machine| machine.id == id) else {
        return Ok(None);
    };
    if !machine_belongs_to_owner(machine, owner) {
        return Err(anyhow!(
            "machine id {id} does not belong to the active account"
        ));
    }
    Ok(Some(machine.clone()))
}

async fn recover_machine_from_server_inventory(
    server_url: &str,
    cfg: &DesktopConfig,
    machine_id: &str,
    data_root: Option<&Path>,
) -> Result<Option<MachineConfig>> {
    let client = Client::connect(server_url).await?;
    client.initialize().await?;
    let value = client.call_raw(method::ACTOR_LIST, None).await?;
    let Some(actors) = value.get("actors").and_then(Value::as_array) else {
        return Ok(None);
    };

    for actor in actors {
        let Some(meta) = actor.get("_meta") else {
            continue;
        };
        let Ok(inventory) = serde_json::from_value::<ServerMachineInventory>(meta.clone()) else {
            continue;
        };
        if inventory.machine_id != machine_id {
            continue;
        }
        if !inventory.is_valid() {
            continue;
        }
        if !server_inventory_belongs_to_active_context(&inventory, cfg, data_root) {
            return Err(anyhow!(
                "machine id {machine_id} does not belong to the active workspace/account"
            ));
        }
        return Ok(Some(MachineConfig {
            workspace_id: inventory.workspace_id,
            owner_actor_id: inventory.owner_actor_id,
            id: inventory.machine_id,
            name: inventory.name,
            kind: inventory.kind,
            data_root: home_path_expr(Path::new(&inventory.data_root)),
            providers: Vec::new(),
            agents: Vec::new(),
        }));
    }

    Ok(None)
}

fn server_inventory_belongs_to_active_context(
    inventory: &ServerMachineInventory,
    cfg: &DesktopConfig,
    data_root: Option<&Path>,
) -> bool {
    if let Some(workspace_id) = inventory.workspace_id.as_deref() {
        if Some(workspace_id) != active_workspace_id(cfg) {
            return false;
        }
    }
    if let Some(owner_actor_id) = inventory.owner_actor_id.as_deref() {
        if Some(owner_actor_id) != active_or_data_root_owner_actor_id(cfg, data_root).as_deref() {
            return false;
        }
    }
    true
}

fn synthesize_requested_machine(
    cfg: &DesktopConfig,
    machine_id: &str,
    data_root: &Path,
) -> MachineConfig {
    let context = parse_machine_data_root_context(data_root);
    MachineConfig {
        workspace_id: context
            .workspace_id
            .or_else(|| active_workspace_id(cfg).map(ToString::to_string)),
        owner_actor_id: context
            .owner_actor_id
            .or_else(|| active_or_data_root_owner_actor_id(cfg, Some(data_root))),
        id: machine_id.to_string(),
        name: context
            .machine_name
            .unwrap_or_else(|| "Local Machine".into()),
        kind: default_machine_kind(),
        data_root: home_path_expr(data_root),
        providers: Vec::new(),
        agents: Vec::new(),
    }
}

#[derive(Debug, Clone, Default)]
struct MachineDataRootContext {
    workspace_id: Option<String>,
    owner_actor_id: Option<String>,
    machine_name: Option<String>,
}

fn parse_machine_data_root_context(data_root: &Path) -> MachineDataRootContext {
    let parts: Vec<String> = data_root
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => part.to_str().map(ToString::to_string),
            _ => None,
        })
        .collect();
    let mut context = MachineDataRootContext {
        machine_name: parts.last().and_then(|part| non_empty(part.trim())),
        ..MachineDataRootContext::default()
    };

    for idx in (0..parts.len()).rev() {
        if parts[idx] != "machines" || idx + 3 >= parts.len() {
            continue;
        }
        context.workspace_id = non_empty(parts[idx + 1].trim());
        context.owner_actor_id = non_empty(parts[idx + 2].trim());
        context.machine_name = non_empty(parts[idx + 3].trim()).or(context.machine_name);
        break;
    }

    context
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
        .or_else(|| {
            active_workspace_id(cfg)
                .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
                .map(|workspace| workspace.actor_id.trim())
                .filter(|actor_id| !actor_id.is_empty())
        })
}

fn active_workspace_can_be_recovered(cfg: &DesktopConfig) -> bool {
    let Some(active_id) = cfg.active.as_deref().and_then(trimmed_non_empty) else {
        return true;
    };
    let Some(active_workspace) = cfg
        .workspaces
        .iter()
        .find(|workspace| workspace.id == active_id)
    else {
        return true;
    };
    active_workspace.server_url.trim().is_empty()
        || active_workspace.actor_id.trim().is_empty()
        || active_workspace.display_name.trim().is_empty()
}

fn account_display_name(account: &HumanAccount) -> String {
    [
        account.nickname.as_str(),
        account.real_name.as_str(),
        account.staff_id.as_str(),
        account.actor_id.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .unwrap_or("you")
    .to_string()
}

fn active_or_data_root_owner_actor_id(
    cfg: &DesktopConfig,
    data_root: Option<&Path>,
) -> Option<String> {
    active_account_actor_id(cfg)
        .map(ToString::to_string)
        .or_else(|| data_root.and_then(owner_actor_id_from_data_root))
}

fn owner_actor_id_from_data_root(data_root: &Path) -> Option<String> {
    data_root.components().find_map(|component| {
        let value = component.as_os_str().to_str()?;
        if value.starts_with("actor_human_") {
            Some(value.to_string())
        } else {
            None
        }
    })
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

impl HumanAccount {
    fn from_actor_id(actor_id: &str) -> Self {
        let staff_id = human_staff_id_from_actor_id(actor_id).to_string();
        Self {
            provider: "unknown".into(),
            staff_id: staff_id.clone(),
            nickname: staff_id,
            real_name: String::new(),
            email: String::new(),
            actor_id: actor_id.to_string(),
            avatar_url: String::new(),
        }
    }

    fn fill_missing_from_actor_id(&mut self, actor_id: &str) -> bool {
        let mut changed = false;
        let staff_id = human_staff_id_from_actor_id(actor_id);
        if self.provider.trim().is_empty() {
            self.provider = "unknown".into();
            changed = true;
        }
        if self.staff_id.trim().is_empty() {
            self.staff_id = staff_id.to_string();
            changed = true;
        }
        if self.nickname.trim().is_empty() {
            self.nickname = staff_id.to_string();
            changed = true;
        }
        changed
    }
}

impl WorkspaceConfig {
    fn from_context(id: &str, server_url: &str, actor_id: &str) -> Self {
        let display_name = if actor_id.trim().is_empty() {
            id.to_string()
        } else {
            human_staff_id_from_actor_id(actor_id).to_string()
        };
        Self {
            id: id.to_string(),
            name: id.to_string(),
            server_url: server_url.to_string(),
            actor_id: actor_id.to_string(),
            display_name,
        }
    }

    fn fill_missing_context(&mut self, server_url: &str, actor_id: &str) -> bool {
        let mut changed = false;
        if self.name.trim().is_empty() {
            self.name = self.id.clone();
            changed = true;
        }
        if self.server_url.trim().is_empty() {
            self.server_url = server_url.to_string();
            changed = true;
        }
        if self.actor_id.trim().is_empty() && !actor_id.trim().is_empty() {
            self.actor_id = actor_id.to_string();
            changed = true;
        }
        if self.display_name.trim().is_empty() {
            self.display_name = if self.actor_id.trim().is_empty() {
                self.id.clone()
            } else {
                human_staff_id_from_actor_id(&self.actor_id).to_string()
            };
            changed = true;
        }
        changed
    }
}

fn fill_missing_machine_context(machine: &mut MachineConfig, fallback: &MachineConfig) -> bool {
    let mut changed = false;
    if machine
        .workspace_id
        .as_deref()
        .and_then(trimmed_non_empty)
        .is_none()
        && fallback
            .workspace_id
            .as_deref()
            .and_then(trimmed_non_empty)
            .is_some()
    {
        machine.workspace_id = fallback.workspace_id.clone();
        changed = true;
    }
    if machine
        .owner_actor_id
        .as_deref()
        .and_then(trimmed_non_empty)
        .is_none()
        && fallback
            .owner_actor_id
            .as_deref()
            .and_then(trimmed_non_empty)
            .is_some()
    {
        machine.owner_actor_id = fallback.owner_actor_id.clone();
        changed = true;
    }
    if machine.name.trim().is_empty() {
        machine.name = fallback.name.clone();
        changed = true;
    }
    if machine.kind.trim().is_empty() {
        machine.kind = fallback.kind.clone();
        changed = true;
    }
    if machine.data_root.trim().is_empty() && !fallback.data_root.trim().is_empty() {
        machine.data_root = fallback.data_root.clone();
        changed = true;
    }
    if machine.providers.is_empty() && !fallback.providers.is_empty() {
        machine.providers = fallback.providers.clone();
        changed = true;
    }
    changed
}

fn human_staff_id_from_actor_id(actor_id: &str) -> &str {
    actor_id
        .strip_prefix("actor_human_")
        .unwrap_or(actor_id)
        .trim()
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

fn home_path_expr(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(home) {
            if rest.as_os_str().is_empty() {
                return "~".into();
            }
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn trimmed_non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
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
                ..HumanAccount::default()
            }),
            workspaces: vec![WorkspaceConfig {
                id: "ws_main".into(),
                ..WorkspaceConfig::default()
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

    fn agent(actor_id: &str) -> MachineAgentConfig {
        MachineAgentConfig {
            provider_id: "codex".into(),
            actor_id: actor_id.into(),
            name: actor_id.into(),
            description: String::new(),
            model: String::new(),
            reasoning_effort: String::new(),
            autostart: true,
            avatar_url: String::new(),
        }
    }

    #[test]
    fn desktop_config_roundtrip_preserves_gui_identity_fields() {
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(HumanAccount {
                provider: "buc".into(),
                staff_id: "88084".into(),
                nickname: "星楚".into(),
                real_name: "陈博俊".into(),
                email: String::new(),
                actor_id: "actor_human_88084".into(),
                avatar_url: "//work.alibaba-inc.com/photo/88084.140x140.jpg".into(),
            }),
            workspaces: vec![WorkspaceConfig {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: "actor_human_88084".into(),
                display_name: "星楚".into(),
            }],
            machines: vec![machine("machine_2eabfd47", Some("actor_human_88084"))],
        };

        let text = toml::to_string_pretty(&cfg).expect("serialize desktop config");
        assert!(text.contains("provider = \"buc\""));
        assert!(text.contains("serverUrl = \"ws://127.0.0.1:7878/rpc\""));
        assert!(text.contains("displayName = \"星楚\""));

        let parsed: DesktopConfig = toml::from_str(&text).expect("parse desktop config");
        assert_eq!(active_account_actor_id(&parsed), Some("actor_human_88084"));
        assert_eq!(parsed.workspaces[0].server_url, "ws://127.0.0.1:7878/rpc");
        assert_eq!(parsed.workspaces[0].display_name, "星楚");
    }

    #[test]
    fn repair_desktop_config_fills_workspace_fields_without_account() {
        let mut cfg = DesktopConfig {
            active: Some("default".into()),
            account: None,
            workspaces: vec![WorkspaceConfig {
                id: "default".into(),
                ..WorkspaceConfig::default()
            }],
            machines: Vec::new(),
        };

        assert!(repair_desktop_config_for_daemon(
            &mut cfg,
            "ws://127.0.0.1:7878/rpc"
        ));
        assert_eq!(cfg.workspaces[0].name, "Local");
        assert_eq!(cfg.workspaces[0].server_url, "ws://127.0.0.1:7878/rpc");
    }

    #[test]
    fn annotate_machine_agent_specs_adds_machine_context_to_actor_meta() {
        let provider = DetectedAgentProvider {
            id: "codex".into(),
            display_name: "Codex CLI".into(),
            command: "codex".into(),
            transport_kind: "command".into(),
            args: vec!["exec".into()],
            transport_env: Default::default(),
            transport: proto::methods::AgentTransport::default(),
            default_model: Some("gpt-5.5".into()),
            model_choices: Vec::new(),
        };
        let definition = AgentDefinition {
            provider_id: "codex".into(),
            actor_id: "actor_agent_machine_2eabfd47_4edb51c3".into(),
            display_name: "蔻黛丝".into(),
            description: None,
            model: Some("gpt-5.5".into()),
            reasoning_effort: Some("xhigh".into()),
            autostart: false,
            avatar_url: None,
        };
        let mut specs = provider_specs_from_agent_definitions(&[provider], &[definition])
            .into_iter()
            .flat_map(|provider| provider.into_agent_specs())
            .collect::<Vec<_>>();
        let machine = machine("machine_2eabfd47", Some("actor_human_88084"));

        annotate_machine_agent_specs(&mut specs, &machine);

        let meta = specs[0].actor._meta.as_ref().expect("agent meta");
        assert_eq!(meta["machineId"], json!("machine_2eabfd47"));
        assert_eq!(meta["workspaceId"], json!("ws_main"));
        assert_eq!(meta["ownerActorId"], json!("actor_human_88084"));
        assert_eq!(meta["providerId"], json!("codex"));
    }

    #[test]
    fn agent_spec_from_command_uses_provider_ref_without_serialized_transport() {
        let provider = DetectedAgentProvider {
            id: "claude".into(),
            display_name: "Claude Code".into(),
            command: "claude".into(),
            transport_kind: "command".into(),
            args: Vec::new(),
            transport_env: Default::default(),
            transport: AgentTransport::default(),
            default_model: Some("sonnet".into()),
            model_choices: Vec::new(),
        };
        let spec = agent_spec_from_command(
            &json!({
                "providerId": "claude",
                "actorId": "actor_agent_writer",
                "name": "Writer",
                "description": "Writes concise updates",
                "model": "opus",
                "reasoningEffort": "high",
                "autostart": true
            }),
            "machine_test",
            &[provider],
        )
        .expect("spec");

        assert_eq!(
            spec.provider_ref
                .as_ref()
                .map(|provider| provider.id.as_str()),
            Some("claude")
        );
        assert_eq!(
            spec.provider_ref
                .as_ref()
                .and_then(|provider| provider.model.as_deref()),
            Some("opus")
        );
        assert!(spec.transport.is_empty());
        let value = serde_json::to_value(&spec).expect("json");
        assert!(value.get("providerRef").is_some());
        assert!(value.get("transport").is_none());
    }

    #[test]
    fn legacy_machine_agent_converts_to_agent_spec() {
        let provider = DetectedAgentProvider {
            id: "codex".into(),
            display_name: "Codex CLI".into(),
            command: "codex".into(),
            transport_kind: "command".into(),
            args: Vec::new(),
            transport_env: Default::default(),
            transport: AgentTransport::default(),
            default_model: Some("gpt-5.5".into()),
            model_choices: Vec::new(),
        };
        let mut legacy = agent("actor_agent_legacy");
        legacy.name = "Legacy".into();
        legacy.description = "Migrated on edit".into();
        legacy.model = "gpt-5.5".into();

        let spec = agent_spec_from_machine_agent_config(&legacy, &[provider]);

        assert_eq!(spec.actor.id, "actor_agent_legacy");
        assert_eq!(spec.actor.display_name, "Legacy");
        assert_eq!(
            spec.provider_ref
                .as_ref()
                .map(|provider| provider.id.as_str()),
            Some("codex")
        );
        assert!(spec.transport.is_empty());
        assert_eq!(
            spec.actor
                ._meta
                .as_ref()
                .and_then(|meta| meta.get("description")),
            Some(&json!("Migrated on edit"))
        );
    }

    #[test]
    fn machine_inventory_meta_publishes_agent_specs_not_legacy_agents() {
        let mut machine = machine("machine_2eabfd47", Some("actor_human_88084"));
        machine.agents.push(agent("actor_agent_legacy"));
        let mut spec = agent_spec_from_definition_without_detected_provider(&AgentDefinition {
            provider_id: "codex".into(),
            actor_id: "actor_agent_legacy".into(),
            display_name: "Legacy".into(),
            description: None,
            model: None,
            reasoning_effort: None,
            autostart: true,
            avatar_url: None,
        });
        annotate_machine_agent_specs(std::slice::from_mut(&mut spec), &machine);

        let meta =
            machine_inventory_meta(&machine, &PathBuf::from("/tmp/loom-data"), &[], &[spec], 7);

        assert!(meta.get("agents").is_none());
        assert_eq!(
            meta.get("agentSpecs")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
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

    #[test]
    fn synthesize_requested_machine_uses_active_context_and_data_root() {
        let cfg = cfg_with_owner("actor_human_1", Vec::new());

        let machine =
            synthesize_requested_machine(&cfg, "machine_remote", Path::new("/tmp/loom-remote"));

        assert_eq!(machine.id, "machine_remote");
        assert_eq!(machine.workspace_id.as_deref(), Some("ws_main"));
        assert_eq!(machine.owner_actor_id.as_deref(), Some("actor_human_1"));
        assert_eq!(machine.data_root, "/tmp/loom-remote");
    }

    #[test]
    fn synthesize_requested_machine_infers_owner_from_data_root() {
        let cfg = DesktopConfig {
            active: Some("ws_main".into()),
            account: None,
            workspaces: vec![WorkspaceConfig {
                id: "ws_main".into(),
                ..WorkspaceConfig::default()
            }],
            machines: Vec::new(),
        };

        let machine = synthesize_requested_machine(
            &cfg,
            "machine_remote",
            Path::new("/tmp/default/actor_human_88084/local_computer"),
        );

        assert_eq!(machine.owner_actor_id.as_deref(), Some("actor_human_88084"));
    }

    #[test]
    fn synthesize_requested_machine_uses_agentx_data_root_context() {
        let cfg = DesktopConfig::default();
        let data_root = PathBuf::from(
            "/home/user/.agentx/machines/ws_3ab26c27/actor_human_339795/my_computer_ae33127d",
        );

        let machine = synthesize_requested_machine(&cfg, "machine_ae33127d", &data_root);

        assert_eq!(machine.id, "machine_ae33127d");
        assert_eq!(machine.workspace_id.as_deref(), Some("ws_3ab26c27"));
        assert_eq!(
            machine.owner_actor_id.as_deref(),
            Some("actor_human_339795")
        );
        assert_eq!(machine.name, "my_computer_ae33127d");
        assert_eq!(
            machine.data_root,
            "/home/user/.agentx/machines/ws_3ab26c27/actor_human_339795/my_computer_ae33127d"
        );
        assert!(machine.agents.is_empty());
    }

    #[test]
    fn restore_selected_machine_context_rehydrates_gui_context() {
        let selected = MachineConfig {
            workspace_id: Some("ws_remote".into()),
            owner_actor_id: Some("actor_human_368136".into()),
            id: "machine_remote_actor_human_368136".into(),
            name: "Remote".into(),
            kind: default_machine_kind(),
            data_root: "~/.agentx/machines/ws_remote/actor_human_368136/local".into(),
            providers: Vec::new(),
            agents: vec![MachineAgentConfig {
                provider_id: "claude".into(),
                actor_id: "actor_agent_claude_1".into(),
                name: "Claude".into(),
                description: String::new(),
                model: String::new(),
                reasoning_effort: String::new(),
                autostart: true,
                avatar_url: String::new(),
            }],
        };
        let mut cfg = DesktopConfig {
            active: Some("default".into()),
            account: None,
            workspaces: vec![WorkspaceConfig {
                id: "default".into(),
                name: String::new(),
                server_url: String::new(),
                actor_id: String::new(),
                display_name: String::new(),
            }],
            machines: Vec::new(),
        };

        let changed =
            restore_selected_machine_context(&mut cfg, &selected, "ws://127.0.0.1:19999/rpc", true);

        assert!(changed);
        assert_eq!(cfg.active.as_deref(), Some("ws_remote"));
        assert_eq!(
            cfg.account
                .as_ref()
                .map(|account| account.actor_id.as_str()),
            Some("actor_human_368136")
        );
        let workspace = cfg
            .workspaces
            .iter()
            .find(|workspace| workspace.id == "ws_remote")
            .expect("workspace restored");
        assert_eq!(workspace.actor_id, "actor_human_368136");
        assert_eq!(workspace.server_url, "ws://127.0.0.1:19999/rpc");
        assert!(cfg
            .machines
            .iter()
            .any(|machine| machine.id == "machine_remote_actor_human_368136"));
    }

    #[test]
    fn restore_selected_machine_context_does_not_steal_complete_active_workspace() {
        let selected = MachineConfig {
            workspace_id: Some("ws_server_a".into()),
            owner_actor_id: Some("actor_human_368136".into()),
            id: "machine_server_a_actor_human_368136".into(),
            name: "Server A Machine".into(),
            kind: default_machine_kind(),
            data_root: "~/.agentx/machines/ws_server_a/actor_human_368136/local".into(),
            providers: Vec::new(),
            agents: Vec::new(),
        };
        let mut cfg = DesktopConfig {
            active: Some("ws_local".into()),
            account: Some(HumanAccount::from_actor_id("actor_human_368136")),
            workspaces: vec![WorkspaceConfig::from_context(
                "ws_local",
                "ws://127.0.0.1:21999/rpc",
                "actor_human_368136",
            )],
            machines: Vec::new(),
        };

        let changed =
            restore_selected_machine_context(&mut cfg, &selected, "ws://server-a/rpc", true);

        assert!(changed);
        assert_eq!(cfg.active.as_deref(), Some("ws_local"));
        assert!(cfg
            .workspaces
            .iter()
            .any(|workspace| workspace.id == "ws_server_a"));
        assert!(cfg
            .machines
            .iter()
            .any(|machine| machine.id == "machine_server_a_actor_human_368136"));
    }

    #[test]
    fn server_inventory_requires_active_context_match() {
        let cfg = cfg_with_owner("actor_human_1", Vec::new());
        let mut inventory = ServerMachineInventory {
            role: "machine".into(),
            source: "daemon".into(),
            inventory_version: 2,
            machine_id: "machine_remote".into(),
            workspace_id: Some("ws_main".into()),
            owner_actor_id: Some("actor_human_1".into()),
            name: "Remote".into(),
            kind: default_machine_kind(),
            data_root: "/tmp/loom-remote".into(),
        };

        assert!(server_inventory_belongs_to_active_context(
            &inventory, &cfg, None
        ));

        inventory.owner_actor_id = Some("actor_human_2".into());

        assert!(!server_inventory_belongs_to_active_context(
            &inventory, &cfg, None
        ));
    }

    #[test]
    fn server_inventory_can_use_data_root_owner_when_account_is_missing() {
        let cfg = DesktopConfig {
            active: Some("ws_main".into()),
            account: None,
            workspaces: vec![WorkspaceConfig {
                id: "ws_main".into(),
                ..WorkspaceConfig::default()
            }],
            machines: Vec::new(),
        };
        let inventory = ServerMachineInventory {
            role: "machine".into(),
            source: "daemon".into(),
            inventory_version: 2,
            machine_id: "machine_remote".into(),
            workspace_id: Some("ws_main".into()),
            owner_actor_id: Some("actor_human_88084".into()),
            name: "Remote".into(),
            kind: default_machine_kind(),
            data_root: "/tmp/loom-remote".into(),
        };
        let data_root = PathBuf::from("/tmp/default/actor_human_88084/local_computer");

        assert!(server_inventory_belongs_to_active_context(
            &inventory,
            &cfg,
            Some(&data_root)
        ));
    }

    #[test]
    fn ensure_selected_machine_restores_cached_machine_without_legacy_agents() {
        let mut selected = machine("machine_remote", Some("actor_human_1"));
        selected.agents.push(agent("actor_agent_existing"));
        let mut cfg = cfg_with_owner(
            "actor_human_1",
            vec![machine("machine_default", Some("actor_human_1"))],
        );

        let (index, restored) =
            ensure_selected_machine_config(&mut cfg, "machine_remote", &selected)
                .expect("restore selected machine");

        assert!(restored);
        assert_eq!(cfg.machines[index].id, "machine_remote");
        assert!(cfg.machines[index].agents.is_empty());
    }

    #[test]
    fn ensure_selected_machine_uses_existing_id_without_active_owner_check() {
        let selected = machine("machine_remote", Some("actor_human_1"));
        let mut cfg = cfg_with_owner(
            "actor_human_2",
            vec![machine("machine_remote", Some("actor_human_1"))],
        );

        let (index, restored) =
            ensure_selected_machine_config(&mut cfg, "machine_remote", &selected)
                .expect("select existing machine");

        assert!(!restored);
        assert_eq!(index, 0);
        assert_eq!(cfg.machines.len(), 1);
    }
}
