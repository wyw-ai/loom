//! Tauri commands exposed to the front-end.
//!
//! Two shapes of command here:
//! - **Workspace profile** management (`workspaces_list`, `workspaces_save`,
//!   `workspace_add`, `workspace_remove`, `set_active_workspace`): pure
//!   local-config manipulation; never touches the server.
//! - **Server RPCs** (`connect`, `channel_*`, `scope_*`, `event_append`, …):
//!   thin wrappers over `Client::call_raw` with JSON pass-through. We
//!   deliberately avoid typed Rust structs here so schema drift stays
//!   debuggable on the TypeScript side.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use proto::methods::method;
use proto::methods::{
    AgentActorSpec, AgentInfo, AgentListResult, AgentModelChoice, AgentProviderSpec, AgentSpec,
    IdentityFiles, IdentityScaffoldSpec, IdentitySpec,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::config::{self, DesktopConfig, MachineConfig, Workspace};
use crate::forward;
use crate::state::AppState;
use crate::ws::Client;

// ---- workspace management -------------------------------------------------

#[tauri::command]
pub async fn workspaces_list() -> Result<DesktopConfig, String> {
    config::load_or_init().map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct SaveWorkspacesArgs {
    pub config: DesktopConfig,
}

#[tauri::command]
pub async fn workspaces_save(args: SaveWorkspacesArgs) -> Result<DesktopConfig, String> {
    config::save(&args.config).map_err(|e| e.to_string())?;
    Ok(config::load_or_init().map_err(|e| e.to_string())?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAddArgs {
    pub name: String,
    pub server_url: String,
    pub actor_id: String,
    #[serde(default)]
    pub display_name: String,
    /// Mark the new workspace as active immediately. Default true — the
    /// usual "add and jump in" flow.
    #[serde(default = "default_activate")]
    pub activate: bool,
}

fn default_activate() -> bool {
    true
}

#[tauri::command]
pub async fn workspace_add(args: WorkspaceAddArgs) -> Result<DesktopConfig, String> {
    let mut cfg = config::load_or_init().map_err(|e| e.to_string())?;
    let had_workspaces = !cfg.workspaces.is_empty();
    let id = config::generate_id();
    let ws = Workspace {
        id: id.clone(),
        name: args.name,
        server_url: args.server_url,
        actor_id: args.actor_id,
        display_name: args.display_name,
    };
    cfg.workspaces.push(ws);
    if had_workspaces {
        cfg.machines
            .push(config::default_machine_for_workspace(&id));
    } else {
        let mut adopted_unscoped = false;
        for machine in &mut cfg.machines {
            if machine.workspace_id.is_none() {
                machine.workspace_id = Some(id.clone());
                adopted_unscoped = true;
            }
        }
        if !adopted_unscoped {
            cfg.machines
                .push(config::default_machine_for_workspace(&id));
        }
    }
    if args.activate {
        cfg.active = Some(id);
    }
    config::save(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

#[derive(Deserialize)]
pub struct WorkspaceIdArgs {
    pub id: String,
}

#[tauri::command]
pub async fn workspace_remove(args: WorkspaceIdArgs) -> Result<DesktopConfig, String> {
    let mut cfg = config::load_or_init().map_err(|e| e.to_string())?;
    cfg.workspaces.retain(|w| w.id != args.id);
    cfg.machines
        .retain(|machine| machine.workspace_id.as_deref() != Some(args.id.as_str()));
    if cfg.active.as_deref() == Some(args.id.as_str()) {
        cfg.active = cfg.workspaces.first().map(|w| w.id.clone());
    }
    config::save(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

#[tauri::command]
pub async fn set_active_workspace(args: WorkspaceIdArgs) -> Result<DesktopConfig, String> {
    let mut cfg = config::load_or_init().map_err(|e| e.to_string())?;
    if !cfg.workspaces.iter().any(|w| w.id == args.id) {
        return Err(format!("unknown workspace id: {}", args.id));
    }
    cfg.active = Some(args.id);
    config::save(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

// ---- connect / disconnect -------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectArgs {
    pub workspace_id: String,
}

#[tauri::command]
pub async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    args: ConnectArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(|e| e.to_string())?;
    let ws = cfg
        .workspaces
        .iter()
        .find(|w| w.id == args.workspace_id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace id: {}", args.workspace_id))?;

    // Drop any prior client first — only one WS at a time for v0. Letting the
    // old Arc fall off scope ends its writer/reader tasks cleanly.
    state.set(None).await;

    let client = Client::connect(&ws.server_url)
        .await
        .map_err(deep_stringify)?;
    client
        .initialize("joi-gui", env!("CARGO_PKG_VERSION"))
        .await
        .map_err(deep_stringify)?;
    let open = client
        .open_connection(&ws.actor_id, Some(&ws.display_name))
        .await
        .map_err(deep_stringify)?;

    forward::spawn(app.clone(), Arc::clone(&client));
    state.set(Some(client)).await;
    let _ = app.emit("joi://connection", forward::ConnectionEvent::Open);

    // Persist the chosen workspace as active.
    let _ = set_active_workspace(WorkspaceIdArgs { id: ws.id.clone() }).await;

    Ok(json!({ "workspace": ws, "open": open }))
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    state.set(None).await;
    Ok(())
}

// ---- channel / thread / scope passthrough ---------------------------------

#[tauri::command]
pub async fn channel_list(state: State<'_, AppState>) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_LIST, None)
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_create(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_CREATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_update(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_UPDATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_delete(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_DELETE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_invite(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_INVITE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_revoke(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_REVOKE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_members(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_MEMBERS, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn thread_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn thread_create(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_CREATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn thread_update(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_UPDATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn thread_delete(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_DELETE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn scope_subscribe(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::SCOPE_SUBSCRIBE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn scope_unsubscribe(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::SCOPE_UNSUBSCRIBE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn scope_read(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::SCOPE_READ, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn event_append(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::EVENT_APPEND, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn turn_close(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TURN_CLOSE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn actor_list(state: State<'_, AppState>) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::ACTOR_LIST, None)
        .await
        .map_err(stringify)
}

// ---- local agent / machine management -------------------------------------

#[tauri::command]
pub async fn agent_list() -> Result<AgentListResult, String> {
    let agents = load_agent_specs()
        .map_err(stringify)?
        .into_iter()
        .map(|spec| AgentInfo {
            spec,
            status: "registered".into(),
            pid: None,
            session_id: None,
        })
        .collect();
    Ok(AgentListResult { agents })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCreateArgs {
    #[serde(default)]
    pub machine_id: Option<String>,
    pub provider_id: String,
    #[serde(default)]
    pub actor_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub autostart: bool,
}

#[tauri::command]
pub async fn agent_create(args: AgentCreateArgs) -> Result<AgentInfo, String> {
    let specs_dir = agent_specs_dir_for(args.machine_id.as_deref()).map_err(stringify)?;
    let agent_spec = add_agent_actor_to_provider(args, &specs_dir).map_err(stringify)?;
    Ok(AgentInfo {
        spec: agent_spec,
        status: "registered".into(),
        pid: None,
        session_id: None,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRemoveArgs {
    #[serde(default)]
    pub machine_id: Option<String>,
    pub actor_id: String,
}

#[tauri::command]
pub async fn agent_remove(args: AgentRemoveArgs) -> Result<AgentListResult, String> {
    let specs_dir = agent_specs_dir_for(args.machine_id.as_deref()).map_err(stringify)?;
    remove_agent_by_actor_id_from_dir(&args.actor_id, &specs_dir).map_err(stringify)?;
    agent_list().await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUpdateArgs {
    #[serde(default)]
    pub machine_id: Option<String>,
    pub actor_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[tauri::command]
pub async fn agent_update(args: AgentUpdateArgs) -> Result<AgentInfo, String> {
    let specs_dir = agent_specs_dir_for(args.machine_id.as_deref()).map_err(stringify)?;
    let spec = update_agent_spec_in_dir(args, &specs_dir).map_err(stringify)?;
    Ok(AgentInfo {
        spec,
        status: "registered".into(),
        pid: None,
        session_id: None,
    })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub setup_status: String,
    pub connection_status: String,
    pub connection_actor_id: String,
    pub specs_dir: String,
    pub data_root: String,
    pub config_dir: String,
    pub agent_count: usize,
    pub online_agent_count: usize,
    pub providers: Vec<MachineAgentProviderInfo>,
    pub agents: Vec<AgentInfo>,
    pub serve_command: String,
    pub setup_script: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAgentProviderInfo {
    pub id: String,
    pub name: String,
    pub transport_kind: String,
    pub command: String,
    pub args: Vec<String>,
    pub actor_count: usize,
    pub default_model: Option<String>,
    pub model_choices: Vec<AgentModelChoice>,
}

#[derive(Debug, serde::Serialize)]
pub struct MachineListResult {
    pub machines: Vec<MachineInfo>,
}

#[tauri::command]
pub async fn machine_list(state: State<'_, AppState>) -> Result<MachineListResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    machines_from_config(&cfg, state.try_client().await).await
}

#[tauri::command]
pub async fn machine_check(state: State<'_, AppState>) -> Result<MachineListResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    let client = match state.try_client().await {
        Some(client) => Some(client),
        None => temporary_machine_check_client(&cfg).await,
    };
    machines_from_config(&cfg, client).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCreateArgs {
    pub name: String,
    #[serde(default)]
    pub specs_dir: String,
    #[serde(default)]
    pub data_root: String,
}

#[tauri::command]
pub async fn machine_create(
    state: State<'_, AppState>,
    args: MachineCreateArgs,
) -> Result<MachineListResult, String> {
    let name = args.name.trim();
    if name.is_empty() {
        return Err("machine name is required".into());
    }
    let mut cfg = config::load_or_init().map_err(stringify)?;
    let workspace_id = config::active_workspace_id(&cfg).map(ToString::to_string);
    let workspace_dir = workspace_id
        .as_deref()
        .map(slugify)
        .unwrap_or_else(|| "unassigned".into());
    let machine_id = config::generate_machine_id();
    let dir_slug = format!(
        "{}_{}",
        slugify(name),
        machine_id.trim_start_matches("machine_")
    );
    let (specs_dir_expr, specs_dir) = machine_path_input(
        args.specs_dir.trim(),
        config::machine_specs_dir_expr(&workspace_dir, &dir_slug),
    )
    .map_err(stringify)?;
    let (data_root_expr, data_root) = machine_path_input(
        args.data_root.trim(),
        config::machine_data_root_expr(&workspace_dir, &dir_slug),
    )
    .map_err(stringify)?;
    std::fs::create_dir_all(&specs_dir)
        .map_err(|e| format!("create specs dir {}: {e}", specs_dir.display()))?;
    std::fs::create_dir_all(&data_root)
        .map_err(|e| format!("create data root {}: {e}", data_root.display()))?;

    cfg.machines.push(MachineConfig {
        workspace_id,
        id: machine_id,
        name: name.to_string(),
        kind: "local".into(),
        specs_dir: specs_dir_expr,
        data_root: data_root_expr,
    });
    config::save(&cfg).map_err(stringify)?;
    machines_from_config(&cfg, state.try_client().await).await
}

fn machine_path_input(input: &str, default_expr: String) -> anyhow::Result<(String, PathBuf)> {
    let raw = if input.is_empty() {
        default_expr
    } else {
        input.to_string()
    };
    let path = normalize_local_path(config::expand_home(&raw))?;
    Ok((config::home_path_expr(&path), path))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineRemoveArgs {
    pub machine_id: String,
}

#[tauri::command]
pub async fn machine_remove(
    state: State<'_, AppState>,
    args: MachineRemoveArgs,
) -> Result<MachineListResult, String> {
    let mut cfg = config::load_or_init().map_err(stringify)?;
    let active_machine_count = cfg
        .machines
        .iter()
        .filter(|machine| config::machine_belongs_to_active_workspace(machine, &cfg))
        .count();
    if active_machine_count <= 1 {
        return Err("last machine cannot be removed".into());
    }
    let before = cfg.machines.len();
    let active_workspace_id = config::active_workspace_id(&cfg).map(ToString::to_string);
    cfg.machines.retain(|machine| {
        machine.id != args.machine_id || machine.workspace_id != active_workspace_id
    });
    if cfg.machines.len() == before {
        return Err(format!("unknown machine id: {}", args.machine_id));
    }
    config::save(&cfg).map_err(stringify)?;
    machines_from_config(&cfg, state.try_client().await).await
}
#[tauri::command]
pub async fn machine_agent_create(
    state: State<'_, AppState>,
    args: AgentCreateArgs,
) -> Result<MachineListResult, String> {
    if args
        .machine_id
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Err("machine id is required".into());
    }
    let specs_dir = agent_specs_dir_for(args.machine_id.as_deref()).map_err(stringify)?;
    add_agent_actor_to_provider(args, &specs_dir).map_err(stringify)?;
    let cfg = config::load_or_init().map_err(stringify)?;
    machines_from_config(&cfg, state.try_client().await).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAgentRemoveArgs {
    pub machine_id: String,
    pub actor_id: String,
}

#[tauri::command]
pub async fn machine_agent_remove(
    state: State<'_, AppState>,
    args: MachineAgentRemoveArgs,
) -> Result<MachineListResult, String> {
    let specs_dir = agent_specs_dir_for(Some(&args.machine_id)).map_err(stringify)?;
    remove_agent_by_actor_id_from_dir(&args.actor_id, &specs_dir).map_err(stringify)?;
    let cfg = config::load_or_init().map_err(stringify)?;
    machines_from_config(&cfg, state.try_client().await).await
}

fn stringify(e: anyhow::Error) -> String {
    deep_stringify(e)
}

/// Flatten an `anyhow::Error` into a single line that preserves the whole
/// source chain (`outer: middle: inner-os-error`). The front-end toast only
/// shows one line — without this, the underlying cause ("connection
/// refused", "http 404", "tls handshake") gets swallowed and the user sees
/// only the top-level "ws connect ws://…" wrapper we add in ws.rs.
fn deep_stringify(e: anyhow::Error) -> String {
    format!("{:#}", e)
}

async fn machines_from_config(
    cfg: &DesktopConfig,
    client: Option<Arc<Client>>,
) -> Result<MachineListResult, String> {
    let server_url = active_server_url(cfg);

    let mut machines = Vec::new();
    for machine in cfg
        .machines
        .iter()
        .filter(|machine| config::machine_belongs_to_active_workspace(machine, cfg))
    {
        machines.push(machine_info(machine, server_url).map_err(stringify)?);
    }
    let mut result = MachineListResult { machines };
    apply_connection_status(&mut result, client).await;
    Ok(result)
}

async fn temporary_machine_check_client(cfg: &DesktopConfig) -> Option<Arc<Client>> {
    let client = Client::connect(active_server_url(cfg)).await.ok()?;
    client
        .initialize("joi-gui-machine-check", env!("CARGO_PKG_VERSION"))
        .await
        .ok()?;
    Some(client)
}

fn active_server_url(cfg: &DesktopConfig) -> &str {
    config::active_workspace_id(cfg)
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .map(|workspace| workspace.server_url.as_str())
        .unwrap_or("ws://127.0.0.1:7878/rpc")
}

async fn apply_connection_status(result: &mut MachineListResult, client: Option<Arc<Client>>) {
    let Some(client) = client else {
        for machine in &mut result.machines {
            machine.connection_status = "notConnected".into();
        }
        return;
    };

    let actor_ids: Vec<String> = result
        .machines
        .iter()
        .flat_map(|machine| {
            std::iter::once(machine.connection_actor_id.clone()).chain(
                machine
                    .agents
                    .iter()
                    .map(|agent| agent.spec.actor.id.clone()),
            )
        })
        .collect();
    if actor_ids.is_empty() {
        for machine in &mut result.machines {
            machine.connection_status = "offline".into();
        }
        return;
    }

    let Ok(value) = client
        .call_raw(
            method::CONNECTION_LIST,
            Some(json!({ "actorIds": actor_ids })),
        )
        .await
    else {
        for machine in &mut result.machines {
            machine.connection_status = "unavailable".into();
        }
        return;
    };
    let live = actor_ids_from_connection_list(&value);

    for machine in &mut result.machines {
        machine.online_agent_count = 0;
        for agent in &mut machine.agents {
            if live.contains(&agent.spec.actor.id) {
                agent.status = "online".into();
                machine.online_agent_count += 1;
            } else {
                agent.status = "registered".into();
            }
        }
        if live.contains(&machine.connection_actor_id) {
            machine.connection_status = "online".into();
            machine.status = "online".into();
        } else {
            machine.connection_status = "offline".into();
            machine.status = machine.setup_status.clone();
        }
    }
}

fn actor_ids_from_connection_list(value: &Value) -> HashSet<String> {
    value
        .get("actorIds")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect()
}

fn machine_info(machine: &MachineConfig, server_url: &str) -> anyhow::Result<MachineInfo> {
    let specs_dir = config::expand_home(&machine.specs_dir);
    let data_root = if machine.data_root.trim().is_empty() {
        config::default_agent_data_root()
    } else {
        config::expand_home(&machine.data_root)
    };
    let provider_specs = load_agent_providers_from_dir(&specs_dir)?;
    let providers = provider_specs
        .iter()
        .map(|(_, provider)| provider_summary(provider))
        .collect::<Vec<_>>();
    let agents: Vec<AgentInfo> = agent_specs_from_providers(
        provider_specs
            .iter()
            .map(|(_, provider)| provider.clone())
            .collect(),
    )
    .into_iter()
    .map(|spec| AgentInfo {
        spec,
        status: "registered".into(),
        pid: None,
        session_id: None,
    })
    .collect();
    let setup_status = if !specs_dir.exists() {
        "missing"
    } else if providers.is_empty() {
        "empty"
    } else {
        "configured"
    };
    let connection_actor_id = machine_connection_actor_id(machine);
    let specs_dir_arg = shell_path_arg(&specs_dir);
    let data_root_arg = shell_path_arg(&data_root);
    let serve_command = format!(
        "JOI_MACHINE_ID={} JOI_MACHINE_ACTOR_ID={} JOI_MACHINE_NAME={} JOI_AGENT_DATA_ROOT={} joi --server {} agent serve --specs {}",
        shell_arg(&machine.id),
        shell_arg(&connection_actor_id),
        shell_arg(&machine.name),
        data_root_arg,
        shell_arg(server_url),
        specs_dir_arg,
    );
    let setup_script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p {} {}\nexport JOI_MACHINE_ID={}\nexport JOI_MACHINE_ACTOR_ID={}\nexport JOI_MACHINE_NAME={}\nexport JOI_AGENT_DATA_ROOT={}\nexec joi --server {} agent serve --specs {}\n",
        shell_path_arg(&specs_dir),
        shell_path_arg(&data_root),
        shell_arg(&machine.id),
        shell_arg(&connection_actor_id),
        shell_arg(&machine.name),
        shell_path_arg(&data_root),
        shell_arg(server_url),
        shell_path_arg(&specs_dir),
    );

    Ok(MachineInfo {
        id: machine.id.clone(),
        name: machine.name.clone(),
        kind: machine.kind.clone(),
        status: setup_status.into(),
        setup_status: setup_status.into(),
        connection_status: "notConnected".into(),
        connection_actor_id,
        specs_dir: config::home_path_expr(&specs_dir),
        data_root: config::home_path_expr(&data_root),
        config_dir: config::home_path_expr(&config::config_dir()),
        agent_count: agents.len(),
        online_agent_count: 0,
        providers,
        agents,
        serve_command,
        setup_script,
    })
}

fn machine_connection_actor_id(machine: &MachineConfig) -> String {
    format!("actor_service_{}", machine.id)
}

fn normalize_local_path(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn shell_arg(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=@".contains(ch))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_path_arg(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(home) {
            if rest.as_os_str().is_empty() {
                return "\"${HOME}\"".into();
            }
            return format!(
                "\"${{HOME}}/{}\"",
                shell_double_quote(&rest.display().to_string())
            );
        }
    }
    shell_arg(&path.display().to_string())
}

fn shell_double_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn agent_specs_dir_for(machine_id: Option<&str>) -> anyhow::Result<PathBuf> {
    let Some(machine_id) = machine_id
        .map(str::trim)
        .filter(|machine_id| !machine_id.is_empty())
    else {
        return Ok(config::default_agent_specs_dir());
    };
    let cfg = config::load_or_init()?;
    let machine = cfg
        .machines
        .iter()
        .find(|machine| {
            machine.id == machine_id && config::machine_belongs_to_active_workspace(machine, &cfg)
        })
        .ok_or_else(|| anyhow::anyhow!("unknown machine id: {machine_id}"))?;
    normalize_local_path(config::expand_home(&machine.specs_dir))
}

fn load_agent_providers_from_dir(dir: &Path) -> anyhow::Result<Vec<(PathBuf, AgentProviderSpec)>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut providers = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        let provider: AgentProviderSpec = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
        providers.push((path, provider));
    }
    providers.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(providers)
}

fn load_agent_specs() -> anyhow::Result<Vec<AgentSpec>> {
    load_agent_specs_from_dir(&config::default_agent_specs_dir())
}

fn load_agent_specs_from_dir(dir: &Path) -> anyhow::Result<Vec<AgentSpec>> {
    let providers = load_agent_providers_from_dir(dir)?
        .into_iter()
        .map(|(_, provider)| provider)
        .collect();
    Ok(agent_specs_from_providers(providers))
}

fn agent_specs_from_providers(providers: Vec<AgentProviderSpec>) -> Vec<AgentSpec> {
    let mut out = Vec::new();
    for provider in providers {
        let provider_id = provider.provider.id.clone();
        let provider_name =
            non_empty(provider.provider.display_name.trim()).unwrap_or_else(|| provider_id.clone());
        for mut spec in provider.into_agent_specs() {
            let transport_kind = spec.transport.kind.clone();
            let meta = spec.actor._meta.get_or_insert_with(BTreeMap::new);
            meta.entry("providerId".into())
                .or_insert_with(|| json!(provider_id.clone()));
            meta.entry("providerName".into())
                .or_insert_with(|| json!(provider_name.clone()));
            meta.entry("transportKind".into())
                .or_insert_with(|| json!(transport_kind.clone()));
            out.push(spec);
        }
    }
    out.sort_by(|a, b| a.actor.id.cmp(&b.actor.id));
    out
}

fn provider_summary(provider: &AgentProviderSpec) -> MachineAgentProviderInfo {
    let models = provider.defaults.models.clone().unwrap_or_default();
    MachineAgentProviderInfo {
        id: provider.provider.id.clone(),
        name: non_empty(provider.provider.display_name.trim())
            .unwrap_or_else(|| provider.provider.id.clone()),
        transport_kind: provider.transport.kind.clone(),
        command: provider.transport.command.clone(),
        args: provider.transport.args.clone(),
        actor_count: provider.actors.len(),
        default_model: models.default,
        model_choices: models.choices,
    }
}

fn remove_agent_by_actor_id_from_dir(actor_id: &str, dir: &Path) -> anyhow::Result<()> {
    let mut found = false;
    for (path, mut provider) in load_agent_providers_from_dir(dir)? {
        let before = provider.actors.len();
        provider.actors.retain(|actor| actor.id != actor_id);
        if provider.actors.len() == before {
            continue;
        }
        found = true;
        write_provider_to_path(&path, &provider)?;
    }
    if !found {
        anyhow::bail!("agent actor not found: {actor_id}");
    }
    Ok(())
}

fn update_agent_spec_in_dir(args: AgentUpdateArgs, dir: &Path) -> anyhow::Result<AgentSpec> {
    for (path, mut provider) in load_agent_providers_from_dir(dir)? {
        let mut changed = false;
        for actor in &mut provider.actors {
            if actor.id != args.actor_id {
                continue;
            }
            if let Some(display_name) = args.display_name.as_deref() {
                actor.display_name = non_empty(display_name.trim());
                changed = true;
            }
            if let Some(description) = args.description.as_deref() {
                actor.identity = Some(IdentitySpec {
                    files: IdentityFiles::default(),
                    description: non_empty(description.trim()),
                    scaffold: non_empty(description.trim()).map(|text| IdentityScaffoldSpec {
                        identity: Some(text),
                        soul: None,
                    }),
                });
                changed = true;
            }
        }
        if changed {
            write_provider_to_path(&path, &provider)?;
            let mut specs = agent_specs_from_providers(vec![provider])
                .into_iter()
                .filter(|spec| spec.actor.id == args.actor_id);
            if let Some(spec) = specs.next() {
                return Ok(spec);
            }
        }
    }
    anyhow::bail!("agent actor not found: {}", args.actor_id)
}

fn write_provider_to_path(path: &Path, provider: &AgentProviderSpec) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(provider)?;
    std::fs::write(path, format!("{text}\n"))?;
    Ok(())
}

fn add_agent_actor_to_provider(args: AgentCreateArgs, dir: &Path) -> anyhow::Result<AgentSpec> {
    let display_name = args.name.trim();
    if display_name.is_empty() {
        anyhow::bail!("agent name is required");
    }
    let provider_id = args.provider_id.trim();
    if provider_id.is_empty() {
        anyhow::bail!("agent provider is required");
    }
    let actor_id = actor_id_from_input(&args.actor_id, display_name)?;
    let mut providers = load_agent_providers_from_dir(dir)?;
    if providers
        .iter()
        .any(|(_, provider)| provider.actors.iter().any(|actor| actor.id == actor_id))
    {
        anyhow::bail!("agent actor already exists: {actor_id}");
    }
    let Some((path, provider)) = providers
        .iter_mut()
        .find(|(_, provider)| provider.provider.id == provider_id)
    else {
        anyhow::bail!("unknown agent provider: {provider_id}");
    };

    let model = non_empty(args.model.trim());
    if let Some(model) = model.as_deref() {
        ensure_provider_allows_model(provider, model)?;
    }

    let mut meta = BTreeMap::new();
    meta.insert("createdBy".into(), json!("joi-gui"));

    provider.actors.push(AgentActorSpec {
        id: actor_id.clone(),
        display_name: Some(display_name.to_string()),
        capabilities: None,
        meta: Some(meta),
        transport: None,
        autostart: Some(args.autostart),
        model,
        models: None,
        bundle: None,
        identity: identity_from_description(args.description.trim()),
        memory: None,
        announcement: None,
    });
    write_provider_to_path(path, provider)?;

    agent_specs_from_providers(vec![provider.clone()])
        .into_iter()
        .find(|spec| spec.actor.id == actor_id)
        .ok_or_else(|| anyhow::anyhow!("agent provider generated no matching actor"))
}

fn identity_from_description(description: &str) -> Option<IdentitySpec> {
    non_empty(description).map(|description| IdentitySpec {
        files: IdentityFiles::default(),
        description: Some(description.clone()),
        scaffold: Some(IdentityScaffoldSpec {
            identity: Some(description),
            soul: None,
        }),
    })
}

fn ensure_provider_allows_model(provider: &AgentProviderSpec, model: &str) -> anyhow::Result<()> {
    let Some(models) = provider.defaults.models.as_ref() else {
        return Ok(());
    };
    if models.choices.is_empty()
        || models.default.as_deref() == Some(model)
        || models.choices.iter().any(|choice| choice.id == model)
    {
        return Ok(());
    }
    anyhow::bail!(
        "model `{model}` is not declared by provider `{}`",
        provider.provider.id
    )
}

fn actor_id_from_input(value: &str, display_name: &str) -> anyhow::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
        return Ok(format!("actor_agent_{}_{}", slugify(display_name), suffix));
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return Ok(trimmed.to_string());
    }
    anyhow::bail!("actor id contains unsupported characters")
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_sep = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_sep = false;
        } else if !last_sep && !out.is_empty() {
            out.push('_');
            last_sep = true;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "agent".into()
    } else {
        trimmed.into()
    }
}
