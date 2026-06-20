//! `loom-daemon` — machine-scoped agent host.
//!
//! The daemon is the machine-scoped agent host: it reads host identity from its
//! config dir, owns daemon-local AgentSpec/provider files, and runs the shared
//! agent worker implementation.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_runtime::discovery::{
    detect_agent_cli_providers, normalize_model_id_for_provider, DetectedAgentProvider,
};
use agent_runtime::provider::{
    builtin_provider_manifests, providers_dir, validate_manifest, ProviderRegistry,
};
use anyhow::{anyhow, Context, Result};
use proto::methods::{
    AgentModelSpec, AgentPromptAssemblySpec, AgentProviderRef, AgentSpec, ProviderManifest,
};
use proto::types::{Actor, ActorKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use crate::cmd::agent_serve::{self, MachineHostSpec};
use crate::cmd::service;
use crate::daemon_ipc;
use crate::{config, render};

const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_secs(3);

pub async fn run(
    machine_id: Option<String>,
    machine_name: Option<String>,
    data_root: Option<PathBuf>,
    allow_actors: Vec<String>,
    list_providers: bool,
    services_dir: Option<PathBuf>,
    allow_services: Vec<String>,
    no_services: bool,
    socket_path: Option<PathBuf>,
    no_ipc: bool,
    server_url: Option<String>,
) -> Result<()> {
    let detected_providers = detect_agent_cli_providers();
    if list_providers {
        print_providers(&detected_providers);
        return Ok(());
    }

    let mut cfg = load_daemon_config_or_default_if_missing()?;
    let (server_url, server_url_changed) =
        resolve_daemon_server_url(&mut cfg, server_url.as_deref());
    let (machine, restored_machine_config) = select_machine_for_daemon(
        &mut cfg,
        machine_id.as_deref(),
        machine_name.as_deref(),
        data_root.as_ref(),
    )?;
    let providers = detected_providers;
    if server_url_changed || restored_machine_config {
        save_daemon_config(&cfg)?;
    }
    let machine = cfg.machine.clone().unwrap_or(machine);
    let selected_machine_id = machine.id.clone();
    let mut selected_machine = machine.clone();
    let data_root = data_root.unwrap_or_else(|| machine_data_root(&machine));
    let data_root = abs_path(data_root);
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
            tracing::warn!("loom-daemon: warning: failed to write daemon discovery: {err:#}");
        }
        (Some(socket_path), Some(proxy_handle))
    };

    if no_services {
        tracing::info!("loom-daemon: service host disabled by --no-services");
    } else {
        spawn_service_host(services_dir, server_url.clone(), allow_services);
    }

    let machine_host_handle =
        agent_serve::spawn_machine_host_loop(machine_host, server_url.clone());
    let mut running_agents = HashMap::new();
    let mut warned_missing = HashSet::new();

    tracing::info!(
        "loom-daemon: machine={} providers={} data={} reload={}s",
        machine.id,
        providers.len(),
        data_root.display(),
        CONFIG_RELOAD_INTERVAL.as_secs()
    );
    if let Some(socket_path) = socket_path.as_ref() {
        tracing::info!("loom-daemon: socket={}", socket_path.display());
    } else {
        tracing::info!("loom-daemon: socket disabled");
    }
    tracing::info!("loom-daemon: ready (ctrl-c to stop)");

    loop {
        // Wrap config reload in a panic guard so a single reload failure
        // (e.g., from corrupted spec file parsing) doesn't kill the daemon.
        let reload_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            refresh_machine_runtime(
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
            )
        }));
        match reload_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!("loom-daemon: reload failed: {e:#}"),
            Err(panic_err) => {
                let msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                tracing::error!("loom-daemon: reload panicked: {msg}");
                // Sleep a bit after a panic to avoid tight panic loops.
                sleep(Duration::from_secs(5)).await;
            }
        }

        tokio::select! {
            _ = shutdown_signal() => break,
            maybe_command = machine_command_rx.recv() => {
                let Some(command) = maybe_command else {
                    // Channel closed — the server-side machine command sender was
                    // dropped. Log once and add a sleep so we don't tight-loop
                    // on disk I/O from refresh_machine_runtime above.
                    tracing::warn!("loom-daemon: machine command channel closed; will keep polling config");
                    sleep(Duration::from_secs(15)).await;
                    continue;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle_machine_command(
                        &selected_machine_id,
                        &mut selected_machine,
                        command.payload,
                    )
                }));
                let result = match result {
                    Ok(r) => r,
                    Err(panic_err) => {
                        let msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown panic".to_string()
                        };
                        tracing::error!("loom-daemon: machine command handler panicked: {msg}");
                        let fallback = machine_command_error(
                            json!({}),
                            format!("internal panic handling machine command: {msg}"),
                        );
                        let _ = command.reply.send(fallback);
                        continue;
                    }
                };
                if result.get("ok").and_then(Value::as_bool) == Some(true) {
                    let refresh_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        refresh_machine_runtime(
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
                        )
                    }));
                    match refresh_result {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) => {
                            let fallback = machine_command_error_from_result(
                                &result,
                                format!("machine command applied but runtime refresh failed: {err:#}"),
                            );
                            let _ = command.reply.send(fallback);
                            continue;
                        }
                        Err(panic_err) => {
                            let msg = if let Some(s) = panic_err.downcast_ref::<String>() {
                                s.clone()
                            } else if let Some(s) = panic_err.downcast_ref::<&str>() {
                                s.to_string()
                            } else {
                                "unknown panic".to_string()
                            };
                            let fallback = machine_command_error_from_result(
                                &result,
                                format!("machine command applied but refresh panicked: {msg}"),
                            );
                            let _ = command.reply.send(fallback);
                            continue;
                        }
                    }
                }
                let _ = command.reply.send(result);
            }
            _ = sleep(CONFIG_RELOAD_INTERVAL) => {}
        }
    }

    tracing::info!("loom-daemon: shutting down");
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
) -> Result<MachineSpecs> {
    let mut cfg = load_daemon_config_or_default_if_missing()?;
    let was_missing = cfg
        .machine
        .as_ref()
        .is_none_or(|machine| machine.id != selected_machine_id);
    let restored_context = if was_missing {
        if warned_missing.insert(format!("machine:{selected_machine_id}")) {
            tracing::warn!(
                "loom-daemon: selected machine {selected_machine_id} is missing from daemon.toml; restoring live runtime snapshot"
            );
        }
        cfg.machine = Some(selected_machine.clone());
        true
    } else {
        false
    };
    let restored = ensure_selected_machine_config(&mut cfg, selected_machine_id, selected_machine)?;
    let providers = detect_agent_cli_providers();
    if restored_context || restored {
        save_daemon_config(&cfg)?;
    }
    let machine = cfg
        .machine
        .clone()
        .ok_or_else(|| anyhow!("daemon machine config is missing"))?;
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

fn agent_specs_dir() -> PathBuf {
    crate::config::config_dir().join("agents")
}

fn agent_spec_path(actor_id: &str) -> PathBuf {
    agent_specs_dir().join(actor_id).join("spec.json")
}

fn flat_agent_spec_path(actor_id: &str) -> PathBuf {
    agent_specs_dir().join(format!("{actor_id}.json"))
}

fn validate_agent_actor_id(actor_id: &str) -> Result<()> {
    proto::path_component::validate_path_component(actor_id, "actor_id").map_err(|err| anyhow!(err))
}

fn validate_machine_id(machine_id: &str) -> Result<()> {
    proto::path_component::validate_path_component(machine_id, "machine_id")
        .map_err(|err| anyhow!(err))
}

fn validate_provider_id_for_path(provider_id: &str) -> Result<()> {
    proto::path_component::validate_path_component(provider_id, "provider_id")
        .map_err(|err| anyhow!(err))?;
    let mut chars = provider_id.chars();
    let Some(first) = chars.next() else {
        return Err(anyhow!("provider id is required"));
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(anyhow!(
            "provider id `{provider_id}` must start with a lowercase ascii letter or digit"
        ));
    }
    if chars.any(|ch| {
        !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    }) {
        return Err(anyhow!(
            "provider id `{provider_id}` may only contain lowercase ascii letters, digits, `_`, `-`, or `.`"
        ));
    }
    Ok(())
}

fn load_config_agent_spec(actor_id: &str) -> Result<Option<AgentSpec>> {
    validate_agent_actor_id(actor_id)?;
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
    validate_agent_actor_id(&spec.actor.id)?;
    let path = agent_spec_path(&spec.actor.id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create agent spec dir {}", parent.display()))?;
    }
    // Strip _meta before writing to disk — _meta is runtime-injected daemon
    // metadata (machineId, workspaceId, etc.) and should not be persisted to
    // spec.json. Otherwise fingerprint mismatches on reload trigger an infinite
    // restart loop.
    let mut clean_spec = spec.clone();
    clean_spec.actor._meta = None;
    let text = serde_json::to_string_pretty(&clean_spec)?;
    atomic_write(&path, &text).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn remove_config_agent_spec(actor_id: &str) -> Result<bool> {
    validate_agent_actor_id(actor_id)?;
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
    let instructions = command
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let model = command
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| normalize_model_id_for_provider(&provider.id, value));
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
    let prompt_assembly = command
        .get("promptAssembly")
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value::<AgentPromptAssemblySpec>(value.clone())
                .context("parse promptAssembly")
        })
        .transpose()?;

    let env: BTreeMap<String, String> = command
        .get("env")
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

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
        instructions,
        provider_ref: AgentProviderRef {
            id: provider.id.clone(),
            mode: Some("print".into()),
            model: model.clone(),
            reasoning_effort,
            env,
        },
        autostart: command
            .get("autostart")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        models: Some(AgentModelSpec {
            default: model.or_else(|| provider.default_model.clone()),
            choices: provider.model_choices.clone(),
        }),
        bundle: None,
        memory: None,
        announcement: None,
        trigger: None,
        prompt_assembly,
        prompt_template: None,
    })
}

fn update_agent_spec_from_command(
    mut spec: AgentSpec,
    command: &Value,
    providers: &[DetectedAgentProvider],
) -> Result<AgentSpec> {
    let mut selected_provider = providers
        .iter()
        .find(|provider| provider.id == spec.provider_ref.id);
    if let Some(provider_id) =
        optional_trimmed_str(command, "providerId").filter(|value| !value.is_empty())
    {
        let provider = providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| anyhow!("provider `{provider_id}` is not available on this machine"))?;
        spec.provider_ref.id = provider.id.clone();
        spec.provider_ref.mode.get_or_insert_with(|| "print".into());
        if let Some(model) = spec.provider_ref.model.as_mut() {
            *model = normalize_model_id_for_provider(&provider.id, model);
        }
        let meta = spec.actor._meta.get_or_insert_with(Default::default);
        meta.insert("providerId".into(), json!(provider.id.clone()));
        meta.insert("providerName".into(), json!(provider.display_name.clone()));
        meta.insert(
            "transportKind".into(),
            json!(provider.transport_kind.clone()),
        );
        if let Some(models) = spec.models.as_mut() {
            models.default = spec
                .provider_ref
                .model
                .clone()
                .or_else(|| provider.default_model.clone());
            models.choices = provider.model_choices.clone();
        }
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
    if let Some(instructions) = optional_trimmed_str(command, "instructions") {
        spec.instructions = if instructions.trim().is_empty() {
            None
        } else {
            Some(instructions)
        };
    }
    if let Some(model) = optional_trimmed_str(command, "model") {
        let model = if model.trim().is_empty() {
            None
        } else {
            Some(normalize_model_id_for_provider(
                &spec.provider_ref.id,
                &model,
            ))
        };
        spec.provider_ref.model = model.clone();
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
        spec.provider_ref.reasoning_effort = reasoning_effort.clone();
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
    if let Some(value) = command.get("promptAssembly") {
        spec.prompt_assembly = if value.is_null() {
            None
        } else {
            Some(
                serde_json::from_value::<AgentPromptAssemblySpec>(value.clone())
                    .context("parse promptAssembly")?,
            )
        };
    }
    if let Some(env_value) = command.get("env") {
        if env_value.is_null() {
            spec.provider_ref.env.clear();
        } else if let Some(obj) = env_value.as_object() {
            spec.provider_ref.env = obj
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
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
                .model
                .clone()
                .or_else(|| provider.default_model.clone()),
            choices: provider.model_choices.clone(),
        });
    }
    Ok(spec)
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

    // Collect stale IDs for server-side cleanup before removing them
    // from the running map, so zombie actors (from a prior daemon restart
    // with a different machine_id) don't shadow newer agents with the
    // same display_name.
    let stale_for_cleanup: Vec<String> = stale.clone();
    if !stale_for_cleanup.is_empty() {
        let url = server_url.to_string();
        tokio::spawn(async move {
            let client = match crate::client::Client::connect(&url).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(
                        "loom-daemon: failed to connect for stale actor cleanup: {e:#}"
                    );
                    return;
                }
            };
            for id in &stale_for_cleanup {
                let params = json!({ "actorId": id });
                match client
                    .call_raw(proto::methods::method::ACTOR_DELETE, Some(params))
                    .await
                {
                    Ok(_) => tracing::info!("[{id}] cleaned up stale actor on server"),
                    Err(e) => tracing::warn!("[{id}] failed to clean up stale actor: {e:#}"),
                }
            }
        });
    }

    for actor_id in stale {
        if let Some(agent) = running.remove(&actor_id) {
            agent.handle.abort();
            tracing::info!("[{actor_id}] stopped: removed from machine config");
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
            tracing::info!("[{actor_id}] restarting: machine config changed");
        } else {
            tracing::info!("[{actor_id}] starting from machine config");
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
    // Exclude _meta from the fingerprint because it is runtime-injected metadata
    // and should not be treated as a "config change". Otherwise server-initiated
    // agent.update commands overwrite the on-disk spec.json (including _meta),
    // causing every reload to detect a change → infinite restart loop.
    let mut spec_without_meta = spec.clone();
    spec_without_meta.actor._meta = None;
    serde_json::to_string(&spec_without_meta).unwrap_or_else(|_| format!("{spec:?}"))
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
            tracing::error!("loom-daemon: service host exited with error: {e:#}");
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
    let result = match apply_machine_command(selected_machine_id, selected_machine, &payload) {
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
    payload: &Value,
) -> Result<Value> {
    let command = payload
        .get("command")
        .ok_or_else(|| anyhow!("missing command payload"))?;
    let op = command
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("machine command op is required"))?;
    let mut cfg = load_daemon_config_or_default_if_missing()?;
    let config_changed =
        ensure_selected_machine_config(&mut cfg, selected_machine_id, selected_machine)?;
    if config_changed {
        save_daemon_config(&cfg)?;
        if let Some(machine) = cfg.machine.clone() {
            *selected_machine = machine;
        }
    }

    match op {
        "agent.create" => {
            let providers = detect_agent_cli_providers();
            let spec = agent_spec_from_command(command, selected_machine_id, &providers)?;
            if load_config_agent_spec(&spec.actor.id)?.is_some() {
                return Err(anyhow!("agent actor already exists: {}", spec.actor.id));
            }
            let path = write_config_agent_spec(&spec)?;
            Ok(json!({ "agentSpec": spec, "path": path.display().to_string() }))
        }
        "agent.remove" => {
            let actor_id = required_str(command, "actorId")?;
            if !remove_config_agent_spec(actor_id)? {
                return Err(anyhow!("daemon-configured agent not found: {actor_id}"));
            }
            Ok(json!({ "actorId": actor_id }))
        }
        "agent.update" => {
            let actor_id = required_str(command, "actorId")?;
            let providers = detect_agent_cli_providers();
            if let Some(spec) = load_config_agent_spec(actor_id)? {
                let spec = update_agent_spec_from_command(spec, command, &providers)?;
                let path = write_config_agent_spec(&spec)?;
                return Ok(json!({ "agentSpec": spec, "path": path.display().to_string() }));
            }
            Err(anyhow!("daemon-configured agent not found: {actor_id}"))
        }
        "agent.prompt.preview" => {
            let actor_id = required_str(command, "actorId")?;
            let spec = load_config_agent_spec(actor_id)?
                .ok_or_else(|| anyhow!("daemon-configured agent not found: {actor_id}"))?;
            let data_root = machine_data_root(selected_machine);
            render_agent_prompt_preview(&spec, command, &data_root)
        }
        "agent.file.list" => {
            let actor_id = required_str(command, "actorId")?;
            load_config_agent_spec(actor_id)?
                .ok_or_else(|| anyhow!("daemon-configured agent not found: {actor_id}"))?;
            let data_root = machine_data_root(selected_machine);
            list_agent_files(actor_id, command, &data_root)
        }
        "agent.file.read" => {
            let actor_id = required_str(command, "actorId")?;
            load_config_agent_spec(actor_id)?
                .ok_or_else(|| anyhow!("daemon-configured agent not found: {actor_id}"))?;
            let data_root = machine_data_root(selected_machine);
            read_agent_file(actor_id, command, &data_root)
        }
        "agent.file.write" => {
            let actor_id = required_str(command, "actorId")?;
            load_config_agent_spec(actor_id)?
                .ok_or_else(|| anyhow!("daemon-configured agent not found: {actor_id}"))?;
            let data_root = machine_data_root(selected_machine);
            write_agent_file(actor_id, command, &data_root)
        }
        "provider.add" => {
            let manifest_value = command
                .get("manifest")
                .ok_or_else(|| anyhow!("manifest is required"))?;
            let registry =
                ProviderRegistry::load(&config::config_dir()).map_err(|err| anyhow!(err))?;
            let manifest = registry
                .resolve_manifest_value(manifest_value.clone())
                .map_err(|err| anyhow!(err))?;
            let replace = command
                .get("replace")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let path = write_local_provider_manifest(&manifest, manifest_value, replace)?;
            Ok(json!({
                "providerManifest": manifest,
                "path": path.display().to_string(),
            }))
        }
        "provider.remove" => {
            let provider_id = required_str(command, "providerId")?;
            let path = remove_local_provider_manifest(provider_id)?;
            Ok(json!({
                "providerId": provider_id,
                "path": path.display().to_string(),
            }))
        }
        other => Err(anyhow!("unsupported machine command op: {other}")),
    }
}

fn ensure_selected_machine_config(
    cfg: &mut DaemonConfig,
    selected_machine_id: &str,
    selected_machine: &MachineConfig,
) -> Result<bool> {
    if let Some(machine) = cfg.machine.as_mut() {
        if machine.id == selected_machine_id {
            let changed = if selected_machine.id == selected_machine_id {
                fill_missing_machine_context(machine, selected_machine)
            } else {
                false
            };
            return Ok(changed);
        }
    }
    if selected_machine.id != selected_machine_id {
        return Err(anyhow!(
            "selected machine cache mismatch: expected {selected_machine_id}, got {}",
            selected_machine.id
        ));
    }
    cfg.machine = Some(selected_machine.clone());
    Ok(true)
}

fn write_local_provider_manifest(
    manifest: &ProviderManifest,
    raw: &Value,
    replace: bool,
) -> Result<PathBuf> {
    validate_manifest(manifest).map_err(|err| anyhow!(err))?;
    if builtin_provider_manifests()
        .iter()
        .any(|builtin| builtin.id == manifest.id)
    {
        return Err(anyhow!(
            "cannot add local provider `{}` because it would shadow a built-in provider",
            manifest.id
        ));
    }
    let dir = providers_dir(&config::config_dir());
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create provider dir {}", dir.display()))?;
    let path = dir.join(format!("{}.json", manifest.id));
    if path.exists() && !replace {
        return Err(anyhow!(
            "local provider `{}` already exists at {}; pass replace=true to overwrite",
            manifest.id,
            path.display()
        ));
    }
    let text = serde_json::to_string_pretty(raw).context("serialize provider manifest")?;
    atomic_write(&path, &text)
        .with_context(|| format!("write provider manifest {}", path.display()))?;
    Ok(path)
}

fn remove_local_provider_manifest(provider_id: &str) -> Result<PathBuf> {
    validate_provider_id_for_path(provider_id)?;
    if builtin_provider_manifests()
        .iter()
        .any(|builtin| builtin.id == provider_id)
    {
        return Err(anyhow!(
            "built-in provider `{provider_id}` cannot be removed"
        ));
    }
    let path = providers_dir(&config::config_dir()).join(format!("{provider_id}.json"));
    if !path.exists() {
        return Err(anyhow!(
            "local provider `{provider_id}` not found at {}",
            path.display()
        ));
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("remove provider manifest {}", path.display()))?;
    Ok(path)
}

const AGENT_FILE_MAX_BYTES: u64 = 128 * 1024;

fn render_agent_prompt_preview(
    spec: &AgentSpec,
    command: &Value,
    data_root: &Path,
) -> Result<Value> {
    let scope = command.get("scope").cloned().unwrap_or_else(|| {
        json!({
            "kind": "channel",
            "id": command
                .get("channelId")
                .and_then(Value::as_str)
                .unwrap_or("preview")
        })
    });
    let sample_message = command
        .get("sampleMessage")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let assembly_value = if let Some(value) = command.get("promptAssembly") {
        if value.is_null() {
            None
        } else {
            Some(value.clone())
        }
    } else {
        spec.prompt_assembly
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .context("serialize agent promptAssembly")?
    };
    let assembly = assembly_value.as_ref();
    let mut warnings = Vec::<String>::new();
    let mut parts = default_prompt_preview_parts(spec, &scope, sample_message, data_root);
    parts.extend(load_prompt_assembly_files(
        &spec.actor.id,
        assembly,
        command,
        data_root,
        &mut warnings,
    )?);
    let outputs = render_prompt_assembly_outputs(assembly, &parts, &mut warnings);
    let bindings = provider_prompt_bindings(spec)?;
    Ok(json!({
        "actorId": spec.actor.id,
        "scope": scope,
        "parts": parts,
        "outputs": outputs,
        "bindings": bindings,
        "warnings": warnings,
    }))
}

fn default_prompt_preview_parts(
    spec: &AgentSpec,
    scope: &Value,
    sample_message: &str,
    data_root: &Path,
) -> Vec<Value> {
    let mut parts = Vec::new();
    parts.push(prompt_preview_part(
        "actor_context",
        "Actor context",
        "builtin",
        format!("Actor: {} ({})", spec.actor.display_name, spec.actor.id),
        false,
    ));
    parts.push(prompt_preview_part(
        "agent_instructions",
        "Agent instructions",
        "agentSpec.instructions",
        spec.instructions.clone().unwrap_or_default(),
        false,
    ));
    parts.push(prompt_preview_part(
        "bootstrap_memory",
        "Bootstrap memory",
        "memory",
        String::new(),
        false,
    ));
    parts.push(prompt_preview_part(
        "turn_memory",
        "Turn memory",
        "memory",
        String::new(),
        false,
    ));
    parts.push(prompt_preview_part(
        "scope_bootstrap",
        "Scope bootstrap",
        "builtin",
        format!(
            "Scope: {} {}",
            scope
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("channel"),
            scope.get("id").and_then(Value::as_str).unwrap_or("preview")
        ),
        false,
    ));
    let profile_prompt_files =
        profile_prompt_files_preview_content(&spec.actor.id, data_root).unwrap_or_default();
    parts.push(prompt_preview_part(
        "profile_prompt_files",
        "Profile prompt files",
        "profile:prompts",
        profile_prompt_files,
        false,
    ));
    parts.push(prompt_preview_part(
        "runtime_context",
        "Runtime context",
        "builtin",
        format!(
            "Scope: {} {}",
            scope
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("channel"),
            scope.get("id").and_then(Value::as_str).unwrap_or("preview")
        ),
        false,
    ));
    parts.push(prompt_preview_part(
        "assignment_context",
        "Assignment context",
        "sample",
        String::new(),
        false,
    ));
    parts.push(prompt_preview_part(
        "user_message",
        "User message",
        "sampleMessage",
        sample_message.to_string(),
        false,
    ));
    parts.push(prompt_preview_part(
        "latest_message",
        "Latest message",
        "sampleMessage",
        sample_message.to_string(),
        false,
    ));
    parts
}

fn profile_prompt_files_preview_content(actor_id: &str, data_root: &Path) -> Result<String> {
    let profile_root = agent_file_root(actor_id, "profile", &Value::Null, data_root)?;
    let prompts_dir = profile_root.join("prompts");
    let metadata = match std::fs::symlink_metadata(&prompts_dir) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(err) => return Err(anyhow!("read metadata {}: {err}", prompts_dir.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(String::new());
    }
    let mut file_names = Vec::new();
    for entry in std::fs::read_dir(&prompts_dir)
        .with_context(|| format!("read {}", prompts_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().trim().to_string();
        if !file_name.is_empty() {
            file_names.push(file_name);
        }
    }
    file_names.sort();
    let sections = file_names
        .into_iter()
        .filter_map(|file_name| {
            let relative = Path::new("prompts").join(&file_name);
            match read_safe_text_file(&profile_root, &relative, AGENT_FILE_MAX_BYTES) {
                Ok(content) => {
                    let content = content.trim_end_matches(['\r', '\n']);
                    if content.trim().is_empty() {
                        None
                    } else {
                        Some(format!("=== Profile prompt: {file_name} ===\n{content}"))
                    }
                }
                Err(_) => None,
            }
        })
        .collect::<Vec<_>>();
    Ok(sections.join("\n\n"))
}

fn prompt_preview_part(
    key: &str,
    title: &str,
    source: &str,
    content: String,
    missing: bool,
) -> Value {
    json!({
        "key": key,
        "title": title,
        "source": source,
        "bytes": content.len(),
        "empty": content.trim().is_empty(),
        "missing": missing,
        "content": content,
    })
}

fn load_prompt_assembly_files(
    actor_id: &str,
    assembly: Option<&Value>,
    command: &Value,
    data_root: &Path,
    warnings: &mut Vec<String>,
) -> Result<Vec<Value>> {
    let Some(files) = assembly
        .and_then(|value| value.get("files"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut parts = Vec::new();
    for file in files {
        let key = file
            .get("key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("promptAssembly.files[].key is required"))?;
        let root = file
            .get("root")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("profile");
        let path = file
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("promptAssembly.files[].path is required"))?;
        let optional = file
            .get("optional")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let max_bytes = file
            .get("maxBytes")
            .and_then(Value::as_u64)
            .unwrap_or(AGENT_FILE_MAX_BYTES)
            .min(AGENT_FILE_MAX_BYTES);
        let root_dir = agent_file_root(actor_id, root, command, data_root)?;
        let relative = validated_relative_path(path, "path")?;
        let source = format!("{root}:{}", relative.display());
        match read_safe_text_file(&root_dir, &relative, max_bytes) {
            Ok(content) => {
                parts.push(prompt_preview_part(
                    &format!("file.{key}"),
                    file.get("title").and_then(Value::as_str).unwrap_or(key),
                    &source,
                    content,
                    false,
                ));
            }
            Err(err) if optional => {
                let _ = err;
                warnings.push(format!(
                    "optional prompt file `{source}` is missing or unreadable"
                ));
                parts.push(prompt_preview_part(
                    &format!("file.{key}"),
                    file.get("title").and_then(Value::as_str).unwrap_or(key),
                    &source,
                    String::new(),
                    true,
                ));
            }
            Err(err) => return Err(err),
        }
    }
    Ok(parts)
}

fn render_prompt_assembly_outputs(
    assembly: Option<&Value>,
    parts: &[Value],
    warnings: &mut Vec<String>,
) -> Value {
    let mut part_map = parts
        .iter()
        .filter_map(|part| {
            Some((
                part.get("key")?.as_str()?.to_string(),
                part.get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    if let Some(vars) = assembly
        .and_then(|value| value.get("vars"))
        .and_then(Value::as_object)
    {
        for (key, value) in vars {
            if let Some(value) = value.as_str() {
                part_map.insert(format!("var.{key}"), value.to_string());
            }
        }
    }
    let outputs = assembly
        .and_then(|value| value.get("outputs"))
        .and_then(Value::as_object);
    let system = outputs
        .and_then(|outputs| outputs.get("system"))
        .map(|spec| render_prompt_output_preview(spec, &part_map, warnings))
        .unwrap_or_else(|| {
            join_prompt_preview_parts(
                &part_map,
                &[
                    "actor_context",
                    "agent_instructions",
                    "bootstrap_memory",
                    "scope_bootstrap",
                    "profile_prompt_files",
                ],
                "\n\n",
                warnings,
            )
        });
    let user = outputs
        .and_then(|outputs| outputs.get("user"))
        .map(|spec| render_prompt_output_preview(spec, &part_map, warnings))
        .unwrap_or_else(|| {
            join_prompt_preview_parts(
                &part_map,
                &[
                    "turn_memory",
                    "runtime_context",
                    "assignment_context",
                    "user_message",
                ],
                "\n\n",
                warnings,
            )
        });
    let full = outputs
        .and_then(|outputs| outputs.get("full"))
        .map(|spec| {
            let mut prompt_map = part_map.clone();
            prompt_map.insert("prompt.system".into(), system.clone());
            prompt_map.insert("prompt.user".into(), user.clone());
            render_prompt_output_preview(spec, &prompt_map, warnings)
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| join_non_empty([system.as_str(), user.as_str()], "\n\n"));
    json!({
        "system": system,
        "user": user,
        "full": full,
    })
}

fn render_prompt_output_preview(
    spec: &Value,
    parts: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> String {
    if let Some(template) = spec.get("template").and_then(Value::as_str) {
        return render_prompt_template_preview(template, parts, warnings);
    }
    let join = spec.get("join").and_then(Value::as_str).unwrap_or("\n\n");
    let include = prompt_output_preview_include(spec, warnings);
    join_prompt_preview_parts(
        parts,
        &include.iter().map(String::as_str).collect::<Vec<_>>(),
        join,
        warnings,
    )
}

fn prompt_output_preview_include(spec: &Value, warnings: &mut Vec<String>) -> Vec<String> {
    if let Some(items) = spec.get("include").and_then(Value::as_array) {
        return items
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
    }
    match spec.get("preset").and_then(Value::as_str) {
        Some("loom_system") => [
            "actor_context",
            "agent_instructions",
            "bootstrap_memory",
            "scope_bootstrap",
            "profile_prompt_files",
        ]
        .into_iter()
        .map(ToString::to_string)
        .collect(),
        Some("loom_turn") => [
            "turn_memory",
            "runtime_context",
            "assignment_context",
            "user_message",
        ]
        .into_iter()
        .map(ToString::to_string)
        .collect(),
        Some("loom_full") => [
            "actor_context",
            "agent_instructions",
            "bootstrap_memory",
            "scope_bootstrap",
            "profile_prompt_files",
            "turn_memory",
            "runtime_context",
            "assignment_context",
            "user_message",
        ]
        .into_iter()
        .map(ToString::to_string)
        .collect(),
        Some(other) => {
            warnings.push(format!("unknown prompt preset `{other}`"));
            Vec::new()
        }
        None => Vec::new(),
    }
}

fn render_prompt_template_preview(
    template: &str,
    parts: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> String {
    let mut out = template.to_string();
    for key in template_placeholders(template) {
        if let Some(value) = parts.get(&key) {
            out = out.replace(&format!("{{{key}}}"), value);
        } else {
            warnings.push(format!("unknown prompt template variable `{key}`"));
            out = out.replace(&format!("{{{key}}}"), "");
        }
    }
    out
}

fn template_placeholders(value: &str) -> impl Iterator<Item = String> + '_ {
    let mut rest = value;
    std::iter::from_fn(move || loop {
        let start = rest.find('{')?;
        let after_open = &rest[start + 1..];
        let Some(end) = after_open.find('}') else {
            rest = "";
            return None;
        };
        let key = &after_open[..end];
        rest = &after_open[end + 1..];
        if !key.trim().is_empty() {
            return Some(key.to_string());
        }
    })
}

fn join_prompt_preview_parts(
    parts: &BTreeMap<String, String>,
    include: &[&str],
    join: &str,
    warnings: &mut Vec<String>,
) -> String {
    let mut values = Vec::new();
    for key in include {
        match parts.get(*key) {
            Some(value) if !value.trim().is_empty() => values.push(value.as_str()),
            Some(_) => {}
            None => warnings.push(format!("unknown prompt part `{key}`")),
        }
    }
    join_non_empty(values, join)
}

fn join_non_empty<'a>(values: impl IntoIterator<Item = &'a str>, join: &str) -> String {
    values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(join)
}

fn provider_prompt_bindings(spec: &AgentSpec) -> Result<Value> {
    let registry = ProviderRegistry::load(&config::config_dir()).map_err(|err| anyhow!(err))?;
    let transport = registry
        .resolve_transport(&spec.provider_ref)
        .map_err(|err| anyhow!(err))?;
    let mut refs = BTreeMap::<String, Vec<String>>::new();
    refs.insert(
        "args".into(),
        transport
            .args
            .iter()
            .flat_map(|value| prompt_binding_refs(value))
            .collect(),
    );
    refs.insert(
        "env".into(),
        transport
            .env
            .values()
            .flat_map(|value| prompt_binding_refs(value))
            .collect(),
    );
    refs.insert(
        "stdin".into(),
        transport
            .stdin
            .as_deref()
            .into_iter()
            .flat_map(prompt_binding_refs)
            .collect(),
    );
    Ok(json!({
        "providerId": spec.provider_ref.id,
        "mode": spec.provider_ref.mode,
        "transport": transport.kind,
        "promptRefs": refs,
    }))
}

fn prompt_binding_refs(value: &str) -> Vec<String> {
    template_placeholders(value)
        .filter(|key| key == "prompt" || key.starts_with("prompt."))
        .collect()
}

fn list_agent_files(actor_id: &str, command: &Value, data_root: &Path) -> Result<Value> {
    let root_name = required_str(command, "root")?;
    let root = agent_file_root(actor_id, root_name, command, data_root)?;
    let prefix = command
        .get("prefix")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let prefix = validated_relative_path(prefix, "prefix")?;
    let dir = root.join(&prefix);
    let mut files = Vec::new();
    if dir.exists() {
        let dir_metadata = std::fs::symlink_metadata(&dir)
            .with_context(|| format!("read metadata {}", dir.display()))?;
        if dir_metadata.file_type().is_symlink() {
            return Err(anyhow!("{} must not be a symlink", dir.display()));
        }
        if !dir_metadata.is_dir() {
            return Err(anyhow!("{} is not a directory", dir.display()));
        }
        ensure_inside_root(&root, &dir)?;
        for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if !file_type.is_file() {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&path)
                .with_context(|| format!("read metadata {}", path.display()))?;
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            files.push(json!({
                "path": relative,
                "bytes": metadata.len(),
                "modified": metadata.modified().ok().and_then(system_time_rfc3339),
            }));
        }
    }
    files.sort_by(|a, b| {
        a.get("path")
            .and_then(Value::as_str)
            .cmp(&b.get("path").and_then(Value::as_str))
    });
    Ok(json!({ "root": root_name, "prefix": prefix.display().to_string(), "files": files }))
}

fn read_agent_file(actor_id: &str, command: &Value, data_root: &Path) -> Result<Value> {
    let root_name = required_str(command, "root")?;
    let root = agent_file_root(actor_id, root_name, command, data_root)?;
    let path = validated_relative_path(required_str(command, "path")?, "path")?;
    let max_bytes = command
        .get("maxBytes")
        .and_then(Value::as_u64)
        .unwrap_or(AGENT_FILE_MAX_BYTES)
        .min(AGENT_FILE_MAX_BYTES);
    let content = read_safe_text_file(&root, &path, max_bytes)?;
    Ok(json!({
        "root": root_name,
        "path": path.display().to_string(),
        "content": content,
    }))
}

fn write_agent_file(actor_id: &str, command: &Value, data_root: &Path) -> Result<Value> {
    let root_name = required_str(command, "root")?;
    let root = agent_file_root(actor_id, root_name, command, data_root)?;
    let path = validated_relative_path(required_str(command, "path")?, "path")?;
    let content = command
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("content is required"))?;
    if content.len() as u64 > AGENT_FILE_MAX_BYTES {
        return Err(anyhow!(
            "content is {} bytes, above maxBytes {}",
            content.len(),
            AGENT_FILE_MAX_BYTES
        ));
    }
    write_safe_text_file(&root, &path, content)?;
    Ok(json!({
        "root": root_name,
        "path": path.display().to_string(),
        "bytes": content.len(),
    }))
}

fn agent_file_root(
    actor_id: &str,
    root: &str,
    command: &Value,
    data_root: &Path,
) -> Result<PathBuf> {
    validate_agent_actor_id(actor_id)?;
    match root {
        "profile" => Ok(data_root.join("agents").join(actor_id).join("profile")),
        "scopeWorkspace" | "scope-workspace" => {
            let channel_id = command
                .get("channelId")
                .and_then(Value::as_str)
                .or_else(|| {
                    command
                        .get("scope")
                        .and_then(|scope| scope.get("id"))
                        .and_then(Value::as_str)
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("channelId is required for scopeWorkspace files"))?;
            proto::path_component::validate_path_component(channel_id, "channel_id")
                .map_err(|err| anyhow!(err))?;
            Ok(data_root
                .join("channels")
                .join(channel_id)
                .join("agents")
                .join(actor_id)
                .join("workspace"))
        }
        other => Err(anyhow!("unsupported agent file root `{other}`")),
    }
}

fn validated_relative_path(value: &str, field: &str) -> Result<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(anyhow!("{field} must be relative"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!(
                    "{field} must not contain parent or root components"
                ));
            }
        }
    }
    Ok(path.to_path_buf())
}

fn read_safe_text_file(root: &Path, relative: &Path, max_bytes: u64) -> Result<String> {
    let path = root.join(relative);
    let metadata = std::fs::symlink_metadata(&path)
        .with_context(|| format!("read metadata {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(anyhow!("{} must not be a symlink", path.display()));
    }
    if !metadata.is_file() {
        return Err(anyhow!("{} is not a regular file", path.display()));
    }
    if metadata.len() > max_bytes {
        return Err(anyhow!(
            "{} is {} bytes, above maxBytes {}",
            path.display(),
            metadata.len(),
            max_bytes
        ));
    }
    ensure_inside_root(root, &path)?;
    std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
}

fn write_safe_text_file(root: &Path, relative: &Path, content: &str) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if path.exists() {
        let metadata = std::fs::symlink_metadata(&path)
            .with_context(|| format!("read metadata {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(anyhow!("{} must not be a symlink", path.display()));
        }
    }
    ensure_inside_root(root, path.parent().unwrap_or(root))?;
    std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))
}

fn ensure_inside_root(root: &Path, path: &Path) -> Result<()> {
    let root = if root.exists() {
        std::fs::canonicalize(root).with_context(|| format!("resolve {}", root.display()))?
    } else {
        root.to_path_buf()
    };
    let path = if path.exists() {
        std::fs::canonicalize(path).with_context(|| format!("resolve {}", path.display()))?
    } else {
        path.to_path_buf()
    };
    if !path.starts_with(&root) {
        return Err(anyhow!(
            "{} resolves outside {}",
            path.display(),
            root.display()
        ));
    }
    Ok(())
}

fn system_time_rfc3339(time: std::time::SystemTime) -> Option<String> {
    let datetime: chrono::DateTime<chrono::Utc> = time.into();
    Some(datetime.to_rfc3339())
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
#[serde(rename_all = "camelCase")]
struct DaemonConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    server_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    machine: Option<MachineConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            "machine.remove",
            "agent.create",
            "agent.update",
            "agent.remove",
            "agent.prompt.preview",
            "agent.file.list",
            "agent.file.read",
            "agent.file.write",
            "provider.add",
            "provider.remove"
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

fn load_daemon_config_or_default_if_missing() -> Result<DaemonConfig> {
    let path = daemon_config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("parse {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DaemonConfig::default()),
        Err(err) => Err(err).with_context(|| format!("read daemon config {}", path.display())),
    }
}

fn save_daemon_config(cfg: &DaemonConfig) -> Result<()> {
    let path = daemon_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(cfg)?;
    atomic_write(&path, &text).with_context(|| format!("write daemon config {}", path.display()))
}

/// Write `content` to `path` atomically: write to a temporary file first,
/// then rename it into place.  This prevents config corruption if the
/// process crashes mid-write.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let temp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4().simple()));
    std::fs::write(&temp_path, content)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn daemon_config_path() -> PathBuf {
    config::config_dir().join("daemon.toml")
}

fn resolve_daemon_server_url(cfg: &mut DaemonConfig, server_url: Option<&str>) -> (String, bool) {
    let raw = server_url
        .and_then(trimmed_non_empty)
        .or_else(|| trimmed_non_empty(&cfg.server_url))
        .unwrap_or("ws://127.0.0.1:7878/rpc")
        .to_string();
    let selected = normalize_daemon_server_url(&raw);
    let changed = cfg.server_url != selected;
    if changed {
        cfg.server_url = selected.clone();
    }
    tracing::info!(
        raw = %raw,
        normalized = %selected,
        "daemon server URL resolved"
    );
    (selected, changed)
}

/// Normalize a daemon server URL so that a WebSocket connection to the
/// server always succeeds regardless of how the URL was persisted.
///
/// - Forces `ws://` scheme (replaces `http://`).
/// - Appends `/rpc` path when missing so the server can upgrade the
///   connection to WebSocket (the server only upgrades on `/rpc`).
fn normalize_daemon_server_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "ws://127.0.0.1:7878/rpc".to_string();
    }

    // Parse the URL, defaulting to ws:// scheme if missing.
    let (scheme, rest) = if let Some(idx) = trimmed.find("://") {
        let scheme = &trimmed[..idx];
        let rest = &trimmed[idx + 3..];
        (scheme, rest)
    } else {
        ("ws", trimmed)
    };

    // Force ws:// scheme — http:// cannot be upgraded to WebSocket by
    // tokio_tungstenite and the server only upgrades on the /rpc route.
    let scheme = if scheme.eq_ignore_ascii_case("wss") {
        "wss"
    } else {
        "ws"
    };

    // Split host:port from path.
    let (authority, path) = if let Some(idx) = rest.find('/') {
        (&rest[..idx], &rest[idx..])
    } else {
        (rest, "/")
    };

    // Ensure path contains /rpc so the server recognises the WebSocket
    // upgrade route.
    let path = if path.contains("/rpc") {
        path.to_string()
    } else {
        format!("/rpc{}", path.trim_end_matches('/'))
    };

    format!("{}://{}{}", scheme, authority.trim_end_matches('/'), path)
}

fn select_machine_for_daemon(
    cfg: &mut DaemonConfig,
    requested: Option<&str>,
    machine_name: Option<&str>,
    data_root: Option<&PathBuf>,
) -> Result<(MachineConfig, bool)> {
    let requested = requested.and_then(|id| non_empty(id.trim()));
    if let Some(machine_id) = requested.as_deref() {
        validate_machine_id(machine_id)?;
    }
    let machine_name = machine_name.and_then(|name| non_empty(name.trim()));
    let data_root_path = data_root
        .cloned()
        .or_else(|| cfg.machine.as_ref().map(machine_data_root));
    let context = data_root_path
        .as_deref()
        .map(parse_machine_data_root_context)
        .unwrap_or_default();
    let requested_id = requested
        .clone()
        .or_else(|| cfg.machine.as_ref().map(|m| m.id.clone()))
        .unwrap_or_else(|| "local".into());
    let mut machine = cfg.machine.clone().unwrap_or_else(|| MachineConfig {
        workspace_id: context.workspace_id.clone(),
        owner_actor_id: context.owner_actor_id.clone(),
        id: requested_id.clone(),
        name: default_machine_name(machine_name.as_deref(), &context, &requested_id),
        kind: default_machine_kind(),
        data_root: data_root_path
            .as_deref()
            .map(home_path_expr)
            .unwrap_or_else(default_agent_data_root_expr),
    });
    let mut changed = false;

    if let Some(requested) = requested {
        if machine.id != requested {
            machine.id = requested.to_string();
            changed = true;
        }
    }
    if let Some(name) = machine_name {
        if machine.name != name {
            machine.name = name.to_string();
            changed = true;
        }
    } else if machine.name.trim().is_empty() {
        machine.name = default_machine_name(None, &context, &machine.id);
        changed = true;
    }
    if machine.kind.trim().is_empty() {
        machine.kind = default_machine_kind();
        changed = true;
    }
    if let Some(data_root) = data_root {
        let expr = home_path_expr(data_root);
        if machine.data_root != expr {
            machine.data_root = expr;
            changed = true;
        }
    } else if machine.data_root.trim().is_empty() {
        machine.data_root = default_agent_data_root_expr();
        changed = true;
    }
    if machine
        .workspace_id
        .as_deref()
        .and_then(trimmed_non_empty)
        .is_none()
        && context.workspace_id.is_some()
    {
        machine.workspace_id = context.workspace_id.clone();
        changed = true;
    }
    if machine
        .owner_actor_id
        .as_deref()
        .and_then(trimmed_non_empty)
        .is_none()
        && context.owner_actor_id.is_some()
    {
        machine.owner_actor_id = context.owner_actor_id.clone();
        changed = true;
    }

    if cfg.machine.as_ref() != Some(&machine) {
        cfg.machine = Some(machine.clone());
        changed = true;
    }
    Ok((machine, changed))
}

fn default_machine_name(
    requested_name: Option<&str>,
    context: &MachineDataRootContext,
    machine_id: &str,
) -> String {
    if let Some(name) = requested_name.and_then(|name| non_empty(name.trim())) {
        return name;
    }
    for key in ["LOOM_MACHINE_NAME", "COMPUTERNAME", "HOSTNAME"] {
        if let Some(name) = std::env::var(key)
            .ok()
            .and_then(|value| non_empty(value.trim()))
        {
            return name;
        }
    }
    if let Some(name) = context.machine_name.as_deref() {
        return name.to_string();
    }
    machine_id.to_string()
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
    let mut context = MachineDataRootContext::default();

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
    changed
}

fn machine_data_root(machine: &MachineConfig) -> PathBuf {
    let raw = if machine.data_root.trim().is_empty() {
        expand_home(&default_agent_data_root_expr())
    } else {
        expand_home(&machine.data_root)
    };
    // Always resolve to an absolute path — relative paths break UNC prefixing
    // in create_dir_all_unc, causing "os error 3" on agent directory creation.
    abs_path(raw)
}

fn abs_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn machine(id: &str, owner: Option<&str>) -> MachineConfig {
        MachineConfig {
            workspace_id: Some("ws_main".into()),
            owner_actor_id: owner.map(ToString::to_string),
            id: id.into(),
            name: id.into(),
            kind: default_machine_kind(),
            data_root: String::new(),
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("loom-daemon-{name}-{nanos}"))
    }

    #[test]
    fn daemon_config_roundtrip_preserves_single_machine_only() {
        let cfg = DaemonConfig {
            server_url: "ws://127.0.0.1:7878/rpc".into(),
            machine: Some(MachineConfig {
                workspace_id: Some("ws_main".into()),
                owner_actor_id: Some("actor_human_88084".into()),
                id: "machine_2eabfd47".into(),
                name: "CanfengMac".into(),
                kind: default_machine_kind(),
                data_root: "~/.agentx/machines/ws_main/actor_human_88084/canfengmac".into(),
            }),
        };

        let text = toml::to_string_pretty(&cfg).expect("serialize daemon config");
        assert!(text.contains("serverUrl = \"ws://127.0.0.1:7878/rpc\""));
        assert!(text.contains("[machine]"));
        assert!(text.contains("name = \"CanfengMac\""));
        assert!(!text.contains("[[workspaces]]"));
        assert!(!text.contains("[account]"));

        let parsed: DaemonConfig = toml::from_str(&text).expect("parse daemon config");
        assert_eq!(
            parsed.machine.as_ref().map(|machine| machine.id.as_str()),
            Some("machine_2eabfd47")
        );
    }

    #[test]
    fn resolve_daemon_server_url_uses_arg_before_config() {
        let custom_url = "ws://custom.test/rpc";
        let mut cfg = DaemonConfig {
            server_url: "ws://127.0.0.1:7878/rpc".into(),
            machine: None,
        };

        let (server_url, changed) = resolve_daemon_server_url(&mut cfg, Some(custom_url));

        assert!(changed);
        assert_eq!(server_url, custom_url);
        assert_eq!(cfg.server_url, custom_url);
    }

    #[test]
    fn select_machine_for_daemon_initializes_daemon_config_from_args() {
        let mut cfg = DaemonConfig::default();
        let data_root = PathBuf::from("/tmp/.agentx/machines/ws_main/actor_human_88084/canfengmac");

        let (machine, changed) = select_machine_for_daemon(
            &mut cfg,
            Some("machine_2eabfd47"),
            Some("CanfengMac"),
            Some(&data_root),
        )
        .expect("select daemon machine");

        assert!(changed);
        assert_eq!(machine.id, "machine_2eabfd47");
        assert_eq!(machine.name, "CanfengMac");
        assert_eq!(machine.workspace_id.as_deref(), Some("ws_main"));
        assert_eq!(machine.owner_actor_id.as_deref(), Some("actor_human_88084"));
        assert_eq!(
            cfg.machine.as_ref().map(|machine| machine.id.as_str()),
            Some("machine_2eabfd47")
        );
    }

    #[test]
    fn annotate_machine_agent_specs_adds_machine_context_to_actor_meta() {
        let mut specs = vec![AgentSpec {
            actor: Actor {
                id: "actor_agent_machine_2eabfd47_4edb51c3".into(),
                kind: ActorKind::Agent,
                display_name: "蔻黛丝".into(),
                capabilities: None,
                _meta: Some(BTreeMap::from([
                    ("providerId".into(), json!("codex")),
                    ("reasoningEffort".into(), json!("xhigh")),
                ])),
            },
            instructions: None,
            provider_ref: AgentProviderRef {
                id: "codex".into(),
                mode: Some("print".into()),
                model: Some("gpt-5.5".into()),
                reasoning_effort: Some("xhigh".into()),
                ..Default::default()
            },
            autostart: false,
            models: Some(AgentModelSpec {
                default: Some("gpt-5.5".into()),
                choices: Vec::new(),
            }),
            bundle: None,
            memory: None,
            announcement: None,
            trigger: None,
            prompt_assembly: None,
            prompt_template: None,
        }];
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
            runtime_plan: None,
            default_model: Some("sonnet".into()),
            model_choices: Vec::new(),
        };
        let spec = agent_spec_from_command(
            &json!({
                "providerId": "claude",
                "actorId": "actor_agent_writer",
                "name": "Writer",
                "description": "Concise writing agent",
                "instructions": "Writes concise updates",
                "model": "opus",
                "reasoningEffort": "high",
                "autostart": true
            }),
            "machine_test",
            &[provider],
        )
        .expect("spec");

        assert_eq!(spec.provider_ref.id.as_str(), "claude");
        assert_eq!(spec.provider_ref.model.as_deref(), Some("opus"));
        assert_eq!(spec.instructions.as_deref(), Some("Writes concise updates"));
        let value = serde_json::to_value(&spec).expect("json");
        assert!(value.get("providerRef").is_some());
        assert!(value.get("transport").is_none());
    }

    #[test]
    fn config_agent_file_operations_reject_path_traversal_actor_ids() {
        for actor_id in ["../escape", "actor/slash", ".hidden", "foo..bar"] {
            assert!(load_config_agent_spec(actor_id).is_err(), "{actor_id}");
            assert!(remove_config_agent_spec(actor_id).is_err(), "{actor_id}");
        }

        let spec = AgentSpec {
            actor: Actor {
                id: "../escape".into(),
                kind: ActorKind::Agent,
                display_name: "Unsafe".into(),
                capabilities: None,
                _meta: None,
            },
            instructions: None,
            provider_ref: AgentProviderRef {
                id: "claude".into(),
                mode: Some("print".into()),
                model: None,
                reasoning_effort: None,
                ..Default::default()
            },
            autostart: false,
            models: None,
            bundle: None,
            memory: None,
            announcement: None,
            trigger: None,
            prompt_assembly: None,
            prompt_template: None,
        };
        assert!(write_config_agent_spec(&spec).is_err());
    }

    #[test]
    fn provider_remove_rejects_unsafe_provider_ids() {
        for provider_id in [
            "../daemon",
            "provider/slash",
            ".hidden",
            "foo..bar",
            "Claude",
        ] {
            assert!(
                validate_provider_id_for_path(provider_id).is_err(),
                "{provider_id}"
            );
        }

        assert!(validate_provider_id_for_path("claude.local").is_ok());
        assert!(validate_provider_id_for_path("my-provider_1").is_ok());
    }

    #[test]
    fn prompt_preview_understands_profile_prompt_files_variable() {
        let parts = vec![prompt_preview_part(
            "profile_prompt_files",
            "Profile prompt files",
            "profile:prompts",
            "Profile prompt content".into(),
            false,
        )];
        let assembly = json!({
            "outputs": {
                "system": { "template": "{profile_prompt_files}" },
                "user": { "template": "" },
                "full": { "include": ["prompt.system", "prompt.user"] }
            }
        });
        let mut warnings = Vec::new();

        let outputs = render_prompt_assembly_outputs(Some(&assembly), &parts, &mut warnings);

        assert_eq!(outputs["system"], json!("Profile prompt content"));
        assert!(
            !warnings
                .iter()
                .any(|warning| warning.contains("profile_prompt_files")),
            "{warnings:?}"
        );
    }

    #[test]
    fn prompt_preview_understands_prompt_assembly_vars() {
        let assembly = json!({
            "vars": {
                "style": "concise"
            },
            "outputs": {
                "system": { "template": "{var.style}" },
                "user": { "template": "" },
                "full": { "include": ["prompt.system", "prompt.user"] }
            }
        });
        let mut warnings = Vec::new();

        let outputs = render_prompt_assembly_outputs(Some(&assembly), &[], &mut warnings);

        assert_eq!(outputs["system"], json!("concise"));
        assert!(
            !warnings.iter().any(|warning| warning.contains("var.style")),
            "{warnings:?}"
        );
    }

    #[test]
    fn prompt_preview_warning_redacts_absolute_prompt_file_path() {
        let data_root = temp_path("prompt-preview-redaction");
        let actor_id = "actor_agent_demo";
        let command = json!({});
        let assembly = json!({
            "files": [{
                "key": "persona",
                "root": "profile",
                "path": "prompts/persona.md",
                "optional": true
            }]
        });
        let mut warnings = Vec::new();

        let parts = load_prompt_assembly_files(
            actor_id,
            Some(&assembly),
            &command,
            &data_root,
            &mut warnings,
        )
        .expect("load prompt assembly files");

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["source"], json!("profile:prompts/persona.md"));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("profile:prompts/persona.md"));
        assert!(!warnings[0].contains(&data_root.display().to_string()));
    }

    #[test]
    fn select_machine_for_daemon_rejects_unsafe_machine_ids() {
        let mut cfg = DaemonConfig::default();

        for machine_id in ["../escape", "machine/slash", ".hidden", "foo..bar"] {
            assert!(
                select_machine_for_daemon(&mut cfg, Some(machine_id), None, None).is_err(),
                "{machine_id}"
            );
        }
    }

    #[test]
    fn machine_inventory_meta_publishes_agent_specs_not_legacy_agents() {
        let machine = machine("machine_2eabfd47", Some("actor_human_88084"));
        let mut spec = AgentSpec {
            actor: Actor {
                id: "actor_agent_legacy".into(),
                kind: ActorKind::Agent,
                display_name: "Legacy".into(),
                capabilities: None,
                _meta: Some(BTreeMap::from([("providerId".into(), json!("codex"))])),
            },
            instructions: None,
            provider_ref: AgentProviderRef {
                id: "codex".into(),
                mode: Some("print".into()),
                model: None,
                reasoning_effort: None,
                ..Default::default()
            },
            autostart: true,
            models: None,
            bundle: None,
            memory: None,
            announcement: None,
            trigger: None,
            prompt_assembly: None,
            prompt_template: None,
        };
        annotate_machine_agent_specs(std::slice::from_mut(&mut spec), &machine);

        let meta =
            machine_inventory_meta(&machine, &PathBuf::from("/tmp/loom-data"), &[], &[spec], 7);

        assert!(meta.get("agents").is_none());
        let capabilities = meta
            .get("capabilities")
            .and_then(Value::as_array)
            .expect("capabilities");
        assert!(capabilities.iter().any(|item| item == "provider.add"));
        assert!(capabilities.iter().any(|item| item == "provider.remove"));
        assert_eq!(
            meta.get("agentSpecs")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn ensure_selected_machine_restores_cached_machine() {
        let selected = machine("machine_remote", Some("actor_human_1"));
        let mut cfg = DaemonConfig {
            server_url: "ws://127.0.0.1:7878/rpc".into(),
            machine: Some(machine("machine_default", Some("actor_human_1"))),
        };

        let restored = ensure_selected_machine_config(&mut cfg, "machine_remote", &selected)
            .expect("restore selected machine");

        assert!(restored);
        assert_eq!(
            cfg.machine.as_ref().map(|machine| machine.id.as_str()),
            Some("machine_remote")
        );
    }

    #[test]
    fn ensure_selected_machine_uses_existing_id_without_active_owner_check() {
        let selected = machine("machine_remote", Some("actor_human_1"));
        let mut cfg = DaemonConfig {
            server_url: "ws://127.0.0.1:7878/rpc".into(),
            machine: Some(machine("machine_remote", Some("actor_human_1"))),
        };

        let restored = ensure_selected_machine_config(&mut cfg, "machine_remote", &selected)
            .expect("select existing machine");

        assert!(!restored);
        assert_eq!(
            cfg.machine.as_ref().map(|machine| machine.id.as_str()),
            Some("machine_remote")
        );
    }
}
