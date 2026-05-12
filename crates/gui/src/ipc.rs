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

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_runtime::discovery::{
    apply_provider_overrides, detect_agent_cli_providers, provider_specs_from_agent_definitions,
    AgentDefinition, DetectedAgentProvider,
};
use proto::methods::method;
use proto::methods::{AgentInfo, AgentListResult, AgentModelChoice, AgentSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::config::{
    self, account_display_name, apply_account_identity, DesktopConfig, HumanAccount,
    MachineAgentConfig, MachineConfig, Workspace,
};
use crate::forward;
use crate::state::AppState;
use crate::ws::Client;
use crate::{account, avatar};

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

#[tauri::command]
pub async fn account_get() -> Result<Option<HumanAccount>, String> {
    Ok(config::load_or_init().map_err(stringify)?.account)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginArgs {
    pub provider: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginResult {
    pub account: HumanAccount,
    pub config: DesktopConfig,
}

#[tauri::command]
pub async fn account_login(
    state: State<'_, AppState>,
    args: AccountLoginArgs,
) -> Result<AccountLoginResult, String> {
    let provider = args.provider.trim().to_ascii_lowercase();
    if provider != "buc" {
        return Err(format!("unsupported account provider: {}", args.provider));
    }

    let account = account::login_buc().await.map_err(deep_stringify)?;
    let avatar_account = account.clone();
    tokio::spawn(async move {
        if let Err(err) = avatar::prefetch_account_avatar(&avatar_account).await {
            tracing::warn!(%err, "avatar prefetch failed");
        }
    });
    let mut cfg = config::load_or_init().map_err(stringify)?;
    cfg.account = Some(account.clone());
    apply_account_identity(&mut cfg);
    config::save(&cfg).map_err(stringify)?;
    let cfg = config::load_or_init().map_err(stringify)?;
    state.set(None).await;
    Ok(AccountLoginResult {
        account: cfg.account.clone().unwrap_or(account),
        config: cfg,
    })
}

#[tauri::command]
pub async fn account_logout(state: State<'_, AppState>) -> Result<DesktopConfig, String> {
    let mut cfg = config::load_or_init().map_err(stringify)?;
    cfg.account = None;
    config::save(&cfg).map_err(stringify)?;
    state.set(None).await;
    Ok(cfg)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarCachedUrlArgs {
    pub url: String,
}

#[tauri::command]
pub async fn avatar_cached_url(args: AvatarCachedUrlArgs) -> Result<String, String> {
    avatar::cached_avatar_data_url(&args.url)
        .await
        .map_err(deep_stringify)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAddArgs {
    pub name: String,
    pub server_url: String,
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
    let account = cfg
        .account
        .clone()
        .ok_or_else(|| "account login required before adding a human workspace".to_string())?;
    let id = config::generate_id();
    let ws = Workspace {
        id: id.clone(),
        name: args.name,
        server_url: args.server_url,
        actor_id: account.actor_id.clone(),
        display_name: account_display_name(&account),
    };
    cfg.workspaces.push(ws);
    cfg.machines.push(config::default_machine_for_workspace(
        &id,
        Some(&account.actor_id),
    ));
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
    let mut cfg = config::load_or_init().map_err(|e| e.to_string())?;
    let account = cfg
        .account
        .clone()
        .ok_or_else(|| "account login required before connecting as a human".to_string())?;
    if apply_account_identity(&mut cfg) {
        config::save(&cfg).map_err(stringify)?;
    }
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
    upsert_human_actor(&client, &account)
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

async fn upsert_human_actor(client: &Arc<Client>, account: &HumanAccount) -> anyhow::Result<()> {
    client
        .call_raw(
            method::ACTOR_UPSERT,
            Some(json!({
                "actor": {
                    "id": account.actor_id,
                    "kind": "human",
                    "displayName": account_display_name(account),
                    "_meta": {
                        "account": {
                            "provider": account.provider,
                            "staffId": account.staff_id,
                            "nickname": account.nickname,
                            "realName": account.real_name,
                            "email": account.email,
                        },
                        "avatarUrl": account.avatar_url,
                    },
                },
            })),
        )
        .await?;
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
pub async fn task_create(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_CREATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_get(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_GET, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_update(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_UPDATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_assignment_create(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_ASSIGNMENT_CREATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_assignment_update(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_ASSIGNMENT_UPDATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn artifact_publish(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::ARTIFACT_PUBLISH, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn artifact_get(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::ARTIFACT_GET, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn artifact_read(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::ARTIFACT_READ, Some(params))
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
pub async fn reminder_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::REMINDER_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn actor_list(state: State<'_, AppState>) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    let value = state
        .client()
        .await?
        .call_raw(method::ACTOR_LIST, None)
        .await
        .map_err(stringify)?;
    Ok(filter_actor_list_for_active_context(value, &cfg))
}

#[tauri::command]
pub async fn actor_upsert(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::ACTOR_UPSERT, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn actor_delete(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::ACTOR_DELETE, Some(params))
        .await
        .map_err(stringify)
}

// ---- local agent / machine management -------------------------------------

#[tauri::command]
pub async fn agent_list() -> Result<AgentListResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    let server_url = active_server_url(&cfg);
    let mut agents = Vec::new();
    for machine in cfg
        .machines
        .iter()
        .filter(|machine| config::machine_belongs_to_active_workspace(machine, &cfg))
    {
        agents.extend(
            machine_info(machine, server_url)
                .map_err(stringify)?
                .agents
                .into_iter()
                .map(|agent| agent.info),
        );
    }
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
    pub reasoning_effort: String,
    #[serde(default)]
    pub autostart: bool,
}

#[tauri::command]
pub async fn agent_create(args: AgentCreateArgs) -> Result<AgentInfo, String> {
    Err(format!(
        "agent creation is daemon-only; create this agent on a machine profile{}",
        args.machine_id
            .as_deref()
            .map(|machine| format!(" ({machine})"))
            .unwrap_or_default()
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRemoveArgs {
    pub actor_id: String,
}

#[tauri::command]
pub async fn agent_remove(
    state: State<'_, AppState>,
    args: AgentRemoveArgs,
) -> Result<AgentListResult, String> {
    let mut cfg = config::load_or_init().map_err(stringify)?;
    let active_workspace_id = config::active_workspace_id(&cfg).map(ToString::to_string);
    let active_owner_actor_id = config::active_account_actor_id(&cfg).map(ToString::to_string);
    let mut removed = false;
    for machine in cfg.machines.iter_mut().filter(|machine| {
        config::machine_belongs_to_workspace_and_owner(
            machine,
            active_workspace_id.as_deref(),
            active_owner_actor_id.as_deref(),
        )
    }) {
        let before = machine.agents.len();
        machine
            .agents
            .retain(|agent| agent.actor_id != args.actor_id);
        removed |= machine.agents.len() != before;
    }
    if !removed {
        return Err(format!(
            "daemon-configured agent not found: {}",
            args.actor_id
        ));
    }
    config::save(&cfg).map_err(stringify)?;
    delete_actors_from_server(state.try_client().await, &[args.actor_id]).await;
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
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub autostart: Option<bool>,
}

#[tauri::command]
pub async fn agent_update(args: AgentUpdateArgs) -> Result<AgentInfo, String> {
    if let Some(info) = update_machine_agent_in_config(&args).map_err(stringify)? {
        return Ok(info);
    }
    Err(format!(
        "daemon-configured agent not found: {}",
        args.actor_id
    ))
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
    pub data_root: String,
    pub config_dir: String,
    pub agent_count: usize,
    pub online_agent_count: usize,
    pub providers: Vec<MachineAgentProviderInfo>,
    pub agents: Vec<MachineAgentInfo>,
    pub serve_command: String,
    pub setup_script: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAgentInfo {
    #[serde(flatten)]
    pub info: AgentInfo,
    pub profile_path: String,
    pub identity_path: String,
    pub soul_path: String,
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
pub struct OpenLocalPathArgs {
    pub path: String,
}

#[tauri::command]
pub async fn open_local_path(args: OpenLocalPathArgs) -> Result<(), String> {
    let raw = args.path.trim();
    if raw.is_empty() {
        return Err("path is required".into());
    }
    let path = normalize_local_path(config::expand_home(raw)).map_err(stringify)?;
    if !path.exists() {
        if path.extension().is_some() {
            return Err(format!("path does not exist: {}", path.display()));
        }
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("create directory {}: {e}", path.display()))?;
    }
    open_path_with_system(&path).map_err(stringify)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileFileReadArgs {
    pub machine_id: String,
    pub actor_id: String,
    pub file: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileFileWriteArgs {
    pub machine_id: String,
    pub actor_id: String,
    pub file: String,
    pub text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileFileResult {
    pub path: String,
    pub text: String,
}

#[tauri::command]
pub async fn agent_profile_file_read(
    args: AgentProfileFileReadArgs,
) -> Result<AgentProfileFileResult, String> {
    let path = resolve_machine_agent_profile_file(&args.machine_id, &args.actor_id, &args.file)
        .map_err(stringify)?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    Ok(AgentProfileFileResult {
        path: config::home_path_expr(&path),
        text,
    })
}

#[tauri::command]
pub async fn agent_profile_file_write(
    args: AgentProfileFileWriteArgs,
) -> Result<AgentProfileFileResult, String> {
    let path = resolve_machine_agent_profile_file(&args.machine_id, &args.actor_id, &args.file)
        .map_err(stringify)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create directory {}: {e}", parent.display()))?;
    }
    std::fs::write(&path, &args.text).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(AgentProfileFileResult {
        path: config::home_path_expr(&path),
        text: args.text,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCreateArgs {
    pub name: String,
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
    let owner_actor_id = config::active_account_actor_id(&cfg).map(ToString::to_string);
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
    let data_root_key = owner_actor_id
        .as_deref()
        .map(|owner| format!("{}/{}", slugify(owner), dir_slug))
        .unwrap_or_else(|| dir_slug.clone());
    let (data_root_expr, data_root) = machine_path_input(
        args.data_root.trim(),
        config::machine_data_root_expr(&workspace_dir, &data_root_key),
    )
    .map_err(stringify)?;
    std::fs::create_dir_all(&data_root)
        .map_err(|e| format!("create data root {}: {e}", data_root.display()))?;

    cfg.machines.push(MachineConfig {
        workspace_id,
        owner_actor_id,
        id: machine_id,
        name: name.to_string(),
        kind: "local".into(),
        data_root: data_root_expr,
        providers: Vec::new(),
        agents: Vec::new(),
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
    let active_owner_actor_id = config::active_account_actor_id(&cfg).map(ToString::to_string);
    let server_url = active_server_url(&cfg).to_string();
    let actor_ids = cfg
        .machines
        .iter()
        .find(|machine| {
            machine.id == args.machine_id
                && config::machine_belongs_to_workspace_and_owner(
                    machine,
                    active_workspace_id.as_deref(),
                    active_owner_actor_id.as_deref(),
                )
        })
        .map(|machine| actor_ids_for_machine(machine, &server_url))
        .unwrap_or_default();
    cfg.machines.retain(|machine| {
        machine.id != args.machine_id
            || !config::machine_belongs_to_workspace_and_owner(
                machine,
                active_workspace_id.as_deref(),
                active_owner_actor_id.as_deref(),
            )
    });
    if cfg.machines.len() == before {
        return Err(format!("unknown machine id: {}", args.machine_id));
    }
    config::save(&cfg).map_err(stringify)?;
    delete_actors_from_server(state.try_client().await, &actor_ids).await;
    machines_from_config(&cfg, state.try_client().await).await
}
#[tauri::command]
pub async fn machine_agent_create(
    state: State<'_, AppState>,
    args: AgentCreateArgs,
) -> Result<MachineListResult, String> {
    let machine_id = args
        .machine_id
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    if machine_id.is_empty() {
        return Err("machine id is required".into());
    }
    let name = args.name.trim();
    if name.is_empty() {
        return Err("agent name is required".into());
    }
    let mut cfg = config::load_or_init().map_err(stringify)?;
    let active_workspace_id = config::active_workspace_id(&cfg).map(ToString::to_string);
    let active_owner_actor_id = config::active_account_actor_id(&cfg).map(ToString::to_string);
    let actor_id = actor_id_from_input(&args.actor_id, name, &machine_id).map_err(stringify)?;
    let machine_index = cfg
        .machines
        .iter()
        .position(|machine| {
            machine.id == machine_id
                && config::machine_belongs_to_workspace_and_owner(
                    machine,
                    active_workspace_id.as_deref(),
                    active_owner_actor_id.as_deref(),
                )
        })
        .ok_or_else(|| format!("unknown machine id: {machine_id}"))?;
    if cfg.machines[machine_index]
        .agents
        .iter()
        .any(|agent| agent.actor_id == actor_id)
    {
        return Err(format!("agent actor already exists: {actor_id}"));
    }
    let providers = detect_agent_cli_providers();
    if !providers
        .iter()
        .any(|provider| provider.id == args.provider_id)
    {
        return Err(format!(
            "provider `{}` is not available on PATH for daemon mode",
            args.provider_id
        ));
    }
    let actor_id_for_upsert = actor_id.clone();
    cfg.machines[machine_index].agents.push(MachineAgentConfig {
        provider_id: args.provider_id,
        actor_id,
        name: name.to_string(),
        description: args.description.trim().to_string(),
        model: args.model.trim().to_string(),
        reasoning_effort: args.reasoning_effort.trim().to_string(),
        autostart: args.autostart,
    });
    config::save(&cfg).map_err(stringify)?;
    if let Some(client) = state.try_client().await {
        let _ = client
            .call_raw(
                method::ACTOR_UPSERT,
                Some(json!({
                    "actor": {
                        "id": actor_id_for_upsert,
                        "kind": "agent",
                        "displayName": name,
                    }
                })),
            )
            .await;
    }
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
    let mut cfg = config::load_or_init().map_err(stringify)?;
    let active_workspace_id = config::active_workspace_id(&cfg).map(ToString::to_string);
    let active_owner_actor_id = config::active_account_actor_id(&cfg).map(ToString::to_string);
    let machine = cfg
        .machines
        .iter_mut()
        .find(|machine| {
            machine.id == args.machine_id
                && config::machine_belongs_to_workspace_and_owner(
                    machine,
                    active_workspace_id.as_deref(),
                    active_owner_actor_id.as_deref(),
                )
        })
        .ok_or_else(|| format!("unknown machine id: {}", args.machine_id))?;
    let before = machine.agents.len();
    machine
        .agents
        .retain(|agent| agent.actor_id != args.actor_id);
    if machine.agents.len() == before {
        return Err(format!(
            "daemon-configured agent not found: {}",
            args.actor_id
        ));
    }
    config::save(&cfg).map_err(stringify)?;
    delete_actors_from_server(state.try_client().await, &[args.actor_id]).await;
    machines_from_config(&cfg, state.try_client().await).await
}

async fn delete_actors_from_server(client: Option<Arc<Client>>, actor_ids: &[String]) {
    let Some(client) = client else {
        return;
    };
    let mut seen = HashSet::new();
    for actor_id in actor_ids {
        let actor_id = actor_id.trim();
        if actor_id.is_empty() || !seen.insert(actor_id.to_string()) {
            continue;
        }
        if let Err(err) = client
            .call_raw(
                method::ACTOR_DELETE,
                Some(json!({
                    "actorId": actor_id,
                })),
            )
            .await
        {
            tracing::warn!(%actor_id, %err, "failed to delete actor from server");
        }
    }
}

fn actor_ids_for_machine(machine: &MachineConfig, server_url: &str) -> Vec<String> {
    let mut actor_ids = vec![machine_connection_actor_id(machine)];
    actor_ids.extend(machine.agents.iter().map(|agent| agent.actor_id.clone()));
    match machine_info(machine, server_url) {
        Ok(info) => {
            actor_ids.extend(
                info.agents
                    .into_iter()
                    .map(|agent| agent.info.spec.actor.id),
            );
        }
        Err(err) => {
            tracing::warn!(
                machine_id = %machine.id,
                %err,
                "failed to read machine agents while deleting machine; falling back to config agents"
            );
            actor_ids.extend(machine.agents.iter().map(|agent| agent.actor_id.clone()));
        }
    }
    actor_ids.sort();
    actor_ids.dedup();
    actor_ids
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
    merge_server_machine_inventory(&mut result, cfg, client.as_ref(), server_url).await;
    apply_connection_status(&mut result, client).await;
    Ok(result)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteMachineMeta {
    #[serde(default)]
    role: String,
    #[serde(default)]
    inventory_version: u64,
    #[serde(default)]
    machine_id: String,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    owner_actor_id: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    data_root: String,
    #[serde(default)]
    config_dir: String,
    #[serde(default)]
    providers: Vec<DetectedAgentProvider>,
    #[serde(default)]
    agents: Vec<MachineAgentConfig>,
}

async fn merge_server_machine_inventory(
    result: &mut MachineListResult,
    cfg: &DesktopConfig,
    client: Option<&Arc<Client>>,
    server_url: &str,
) {
    let Some(client) = client else {
        return;
    };
    let Ok(value) = client.call_raw(method::ACTOR_LIST, None).await else {
        return;
    };
    let Some(actors) = value.get("actors").and_then(Value::as_array) else {
        return;
    };
    for actor in actors {
        let Some(machine) = server_machine_info_from_actor(actor, cfg, server_url) else {
            continue;
        };
        if let Some(existing) = result.machines.iter_mut().find(|m| m.id == machine.id) {
            *existing = machine;
        } else {
            result.machines.push(machine);
        }
    }
}

fn server_machine_info_from_actor(
    actor: &Value,
    cfg: &DesktopConfig,
    server_url: &str,
) -> Option<MachineInfo> {
    let meta_value = actor.get("_meta")?.clone();
    let meta: RemoteMachineMeta = serde_json::from_value(meta_value).ok()?;
    if meta.role != "machine"
        || meta.inventory_version == 0
        || !remote_machine_belongs_to_active_context(&meta, cfg)
    {
        return None;
    }
    let connection_actor_id = actor.get("id")?.as_str()?.to_string();
    let machine_id = if meta.machine_id.trim().is_empty() {
        connection_actor_id
            .strip_prefix("actor_service_")
            .unwrap_or(connection_actor_id.as_str())
            .to_string()
    } else {
        meta.machine_id.clone()
    };
    let name = if meta.name.trim().is_empty() {
        actor
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or(machine_id.as_str())
            .to_string()
    } else {
        meta.name.clone()
    };
    let kind = if meta.kind.trim().is_empty() {
        "remote".to_string()
    } else {
        meta.kind.clone()
    };
    let data_root = PathBuf::from(&meta.data_root);
    let mut providers = meta
        .providers
        .iter()
        .map(detected_provider_summary)
        .collect::<Vec<_>>();
    for provider in &mut providers {
        provider.actor_count += meta
            .agents
            .iter()
            .filter(|agent| agent.provider_id == provider.id)
            .count();
    }
    let machine_agent_defs = meta
        .agents
        .iter()
        .map(machine_agent_definition)
        .collect::<Vec<_>>();
    let agents: Vec<MachineAgentInfo> =
        provider_specs_from_agent_definitions(&meta.providers, &machine_agent_defs)
            .into_iter()
            .flat_map(|provider| provider.into_agent_specs())
            .map(|spec| {
                let paths = agent_profile_paths(&data_root, &spec);
                MachineAgentInfo {
                    info: AgentInfo {
                        spec,
                        status: "registered".into(),
                        pid: None,
                        session_id: None,
                    },
                    profile_path: paths.profile.display().to_string(),
                    identity_path: paths.identity.display().to_string(),
                    soul_path: paths.soul.display().to_string(),
                }
            })
            .collect();
    let setup_status = if providers.is_empty() {
        "noCli"
    } else if agents.is_empty() {
        "ready"
    } else {
        "configured"
    };
    let data_root_arg = shell_path_arg(&data_root);
    let serve_command = format!(
        "JOI_AGENT_DATA_ROOT={} joi --server {} daemon --machine-id {}",
        data_root_arg,
        shell_arg(server_url),
        shell_arg(&machine_id),
    );
    let setup_script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p {}\nexport JOI_AGENT_DATA_ROOT={}\nexec joi --server {} daemon --machine-id {}\n",
        shell_path_arg(&data_root),
        shell_path_arg(&data_root),
        shell_arg(server_url),
        shell_arg(&machine_id),
    );

    Some(MachineInfo {
        id: machine_id,
        name,
        kind,
        status: setup_status.into(),
        setup_status: setup_status.into(),
        connection_status: "notConnected".into(),
        connection_actor_id,
        data_root: meta.data_root,
        config_dir: meta.config_dir,
        agent_count: agents.len(),
        online_agent_count: 0,
        providers,
        agents,
        serve_command,
        setup_script,
    })
}

fn remote_machine_belongs_to_active_context(meta: &RemoteMachineMeta, cfg: &DesktopConfig) -> bool {
    if let Some(workspace_id) = meta.workspace_id.as_deref() {
        if Some(workspace_id) != config::active_workspace_id(cfg) {
            return false;
        }
    }
    if let Some(owner_actor_id) = meta.owner_actor_id.as_deref() {
        if Some(owner_actor_id) != config::active_account_actor_id(cfg) {
            return false;
        }
    }
    true
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
                    .map(|agent| agent.info.spec.actor.id.clone()),
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
            if live.contains(&agent.info.spec.actor.id) {
                agent.info.status = "online".into();
                machine.online_agent_count += 1;
            } else {
                agent.info.status = "registered".into();
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

fn filter_actor_list_for_active_context(mut value: Value, cfg: &DesktopConfig) -> Value {
    let allowed_agents = cfg
        .machines
        .iter()
        .filter(|machine| config::machine_belongs_to_active_workspace(machine, cfg))
        .flat_map(|machine| machine.agents.iter().map(|agent| agent.actor_id.clone()))
        .collect::<HashSet<_>>();

    let Some(actors) = value.get_mut("actors").and_then(Value::as_array_mut) else {
        return value;
    };

    actors.retain(|actor| {
        let kind = actor
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind != "agent" {
            return true;
        }
        actor
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| allowed_agents.contains(id))
    });

    value
}

fn machine_info(machine: &MachineConfig, server_url: &str) -> anyhow::Result<MachineInfo> {
    let data_root = if machine.data_root.trim().is_empty() {
        config::default_agent_data_root()
    } else {
        config::expand_home(&machine.data_root)
    };
    let detected_providers =
        apply_provider_overrides(detect_agent_cli_providers(), &machine.providers);
    let mut providers = detected_providers
        .iter()
        .map(detected_provider_summary)
        .collect::<Vec<_>>();
    for provider in &mut providers {
        provider.actor_count += machine
            .agents
            .iter()
            .filter(|agent| agent.provider_id == provider.id)
            .count();
    }
    let machine_agent_defs = machine
        .agents
        .iter()
        .map(machine_agent_definition)
        .collect::<Vec<_>>();
    let provider_specs =
        provider_specs_from_agent_definitions(&detected_providers, &machine_agent_defs);
    let agents: Vec<MachineAgentInfo> = provider_specs
        .into_iter()
        .flat_map(|provider| provider.into_agent_specs())
        .map(|spec| {
            let paths = agent_profile_paths(&data_root, &spec);
            MachineAgentInfo {
                info: AgentInfo {
                    spec,
                    status: "registered".into(),
                    pid: None,
                    session_id: None,
                },
                profile_path: config::home_path_expr(&paths.profile),
                identity_path: config::home_path_expr(&paths.identity),
                soul_path: config::home_path_expr(&paths.soul),
            }
        })
        .collect();
    let setup_status = if providers.is_empty() {
        "noCli"
    } else if agents.is_empty() {
        "ready"
    } else {
        "configured"
    };
    let connection_actor_id = machine_connection_actor_id(machine);
    let data_root_arg = shell_path_arg(&data_root);
    let serve_command = format!(
        "JOI_AGENT_DATA_ROOT={} joi --server {} daemon --machine-id {}",
        data_root_arg,
        shell_arg(server_url),
        shell_arg(&machine.id),
    );
    let setup_script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p {}\nexport JOI_AGENT_DATA_ROOT={}\nexec joi --server {} daemon --machine-id {}\n",
        shell_path_arg(&data_root),
        shell_path_arg(&data_root),
        shell_arg(server_url),
        shell_arg(&machine.id),
    );

    Ok(MachineInfo {
        id: machine.id.clone(),
        name: machine.name.clone(),
        kind: machine.kind.clone(),
        status: setup_status.into(),
        setup_status: setup_status.into(),
        connection_status: "notConnected".into(),
        connection_actor_id,
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

struct AgentProfilePaths {
    profile: PathBuf,
    identity: PathBuf,
    soul: PathBuf,
}

fn agent_profile_paths(data_root: &Path, spec: &AgentSpec) -> AgentProfilePaths {
    let profile = data_root
        .join("agents")
        .join(&spec.actor.id)
        .join("profile");
    let identity_file = spec
        .identity
        .as_ref()
        .map(|identity| identity.files.identity.as_str())
        .unwrap_or("identity.md");
    let soul_file = spec
        .identity
        .as_ref()
        .map(|identity| identity.files.soul.as_str())
        .unwrap_or("soul.md");
    AgentProfilePaths {
        identity: resolve_profile_path(&profile, identity_file),
        soul: resolve_profile_path(&profile, soul_file),
        profile,
    }
}

fn resolve_profile_path(profile: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        profile.join(path)
    }
}

fn resolve_machine_agent_profile_file(
    machine_id: &str,
    actor_id: &str,
    file: &str,
) -> anyhow::Result<PathBuf> {
    let cfg = config::load_or_init()?;
    let machine = cfg
        .machines
        .iter()
        .find(|machine| {
            machine.id == machine_id && config::machine_belongs_to_active_workspace(machine, &cfg)
        })
        .ok_or_else(|| anyhow::anyhow!("unknown machine id: {machine_id}"))?;
    let agent = machine
        .agents
        .iter()
        .find(|agent| agent.actor_id == actor_id)
        .ok_or_else(|| anyhow::anyhow!("unknown agent actor id: {actor_id}"))?;
    let file_name = match file.trim() {
        "identity" => "identity.md",
        "soul" => "soul.md",
        other => return Err(anyhow::anyhow!("unknown profile file: {other}")),
    };
    let data_root = if machine.data_root.trim().is_empty() {
        config::default_agent_data_root()
    } else {
        config::expand_home(&machine.data_root)
    };
    Ok(data_root
        .join("agents")
        .join(&agent.actor_id)
        .join("profile")
        .join(file_name))
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

fn open_path_with_system(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer");
        command.arg(path);
        command
    };

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    let status = command
        .status()
        .map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "open {} exited with status {status}",
            path.display()
        ))
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

fn detected_provider_summary(provider: &DetectedAgentProvider) -> MachineAgentProviderInfo {
    MachineAgentProviderInfo {
        id: provider.id.clone(),
        name: provider.display_name.clone(),
        transport_kind: provider.transport_kind.clone(),
        command: provider.command.clone(),
        args: provider.args.clone(),
        actor_count: 0,
        default_model: provider.default_model.clone(),
        model_choices: provider.model_choices.clone(),
    }
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

fn update_machine_agent_in_config(args: &AgentUpdateArgs) -> anyhow::Result<Option<AgentInfo>> {
    let Some(machine_id) = args
        .machine_id
        .as_deref()
        .map(str::trim)
        .filter(|machine_id| !machine_id.is_empty())
    else {
        return Ok(None);
    };

    let mut cfg = config::load_or_init()?;
    let active_workspace_id = config::active_workspace_id(&cfg).map(ToString::to_string);
    let active_owner_actor_id = config::active_account_actor_id(&cfg).map(ToString::to_string);
    let Some(machine_index) = cfg.machines.iter().position(|machine| {
        machine.id == machine_id
            && config::machine_belongs_to_workspace_and_owner(
                machine,
                active_workspace_id.as_deref(),
                active_owner_actor_id.as_deref(),
            )
    }) else {
        return Ok(None);
    };
    let Some(agent_index) = cfg.machines[machine_index]
        .agents
        .iter()
        .position(|agent| agent.actor_id == args.actor_id)
    else {
        return Ok(None);
    };

    {
        let agent = &mut cfg.machines[machine_index].agents[agent_index];
        if let Some(provider_id) = args.provider_id.as_deref() {
            agent.provider_id = provider_id.trim().to_string();
        }
        if let Some(display_name) = args.display_name.as_deref() {
            agent.name = display_name.trim().to_string();
        }
        if let Some(description) = args.description.as_deref() {
            agent.description = description.trim().to_string();
        }
        if let Some(model) = args.model.as_deref() {
            agent.model = model.trim().to_string();
        }
        if let Some(reasoning_effort) = args.reasoning_effort.as_deref() {
            agent.reasoning_effort = reasoning_effort.trim().to_string();
        }
        if let Some(autostart) = args.autostart {
            agent.autostart = autostart;
        }
    }

    config::save(&cfg)?;
    let server_url = active_server_url(&cfg).to_string();
    let machine = machine_info(&cfg.machines[machine_index], &server_url)?;
    Ok(machine
        .agents
        .into_iter()
        .find(|agent| agent.info.spec.actor.id == args.actor_id)
        .map(|agent| agent.info))
}

fn actor_id_from_input(
    value: &str,
    display_name: &str,
    machine_id: &str,
) -> anyhow::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let display_slug = slugify(display_name);
        let machine_slug = compact_machine_slug(machine_id);
        let stem = if display_slug == "agent" || display_slug.chars().count() < 2 {
            machine_slug
        } else {
            display_slug
        };
        return Ok(format!("actor_agent_{}_{}", stem, suffix));
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return Ok(trimmed.to_string());
    }
    anyhow::bail!("actor id contains unsupported characters")
}

fn compact_machine_slug(machine_id: &str) -> String {
    let slug = slugify(machine_id);
    let compact = slug
        .strip_prefix("machine_")
        .filter(|rest| !rest.is_empty())
        .unwrap_or(slug.as_str());
    format!("machine_{compact}")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_account() -> HumanAccount {
        config::normalize_human_account(HumanAccount {
            provider: "buc".into(),
            staff_id: "88084".into(),
            nickname: "星楚".into(),
            real_name: "陈博俊".into(),
            email: String::new(),
            actor_id: String::new(),
            avatar_url: String::new(),
        })
    }

    fn test_machine(id: &str, owner_actor_id: Option<&str>, agent_actor_id: &str) -> MachineConfig {
        MachineConfig {
            workspace_id: Some("default".into()),
            owner_actor_id: owner_actor_id.map(ToString::to_string),
            id: id.into(),
            name: id.into(),
            kind: "local".into(),
            data_root: "~/.agentx".into(),
            providers: Vec::new(),
            agents: vec![MachineAgentConfig {
                provider_id: "codex".into(),
                actor_id: agent_actor_id.into(),
                name: agent_actor_id.into(),
                description: String::new(),
                model: String::new(),
                reasoning_effort: String::new(),
                autostart: false,
            }],
        }
    }

    #[test]
    fn generated_actor_id_uses_machine_stem_for_short_ascii_name_fragments() {
        let id = actor_id_from_input("", "G仔", "machine_macbook_01").expect("actor id");

        assert!(id.starts_with("actor_agent_machine_macbook_01_"));
    }

    #[test]
    fn generated_actor_id_can_use_meaningful_display_slug() {
        let id = actor_id_from_input("", "Reviewer", "machine_macbook_01").expect("actor id");

        assert!(id.starts_with("actor_agent_reviewer_"));
    }

    #[test]
    fn explicit_actor_id_is_preserved_when_valid() {
        let id = actor_id_from_input("actor_agent_custom:01", "G仔", "machine_macbook_01")
            .expect("actor id");

        assert_eq!(id, "actor_agent_custom:01");
    }

    #[test]
    fn actor_list_filter_hides_agents_from_other_machine_owners() {
        let account = test_account();
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(account.clone()),
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: account.actor_id.clone(),
                display_name: account_display_name(&account),
            }],
            machines: vec![
                test_machine("mine", Some(account.actor_id.as_str()), "actor_agent_mine"),
                test_machine("other", Some("actor_human_other"), "actor_agent_other"),
            ],
        };
        let value = json!({
            "actors": [
                { "id": account.actor_id, "kind": "human", "displayName": "星楚" },
                { "id": "actor_agent_mine", "kind": "agent", "displayName": "Mine" },
                { "id": "actor_agent_other", "kind": "agent", "displayName": "Other" },
                { "id": "actor_service_other", "kind": "service", "displayName": "Other Service" }
            ]
        });

        let filtered = filter_actor_list_for_active_context(value, &cfg);
        let actor_ids = filtered["actors"]
            .as_array()
            .expect("actors")
            .iter()
            .filter_map(|actor| actor["id"].as_str())
            .collect::<Vec<_>>();

        assert!(actor_ids.contains(&"actor_agent_mine"));
        assert!(!actor_ids.contains(&"actor_agent_other"));
        assert!(actor_ids.contains(&"actor_service_other"));
    }

    #[test]
    fn server_machine_inventory_builds_computer_info() {
        let account = test_account();
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(account.clone()),
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: account.actor_id.clone(),
                display_name: account_display_name(&account),
            }],
            machines: vec![],
        };
        let actor = json!({
            "id": "actor_service_machine_remote",
            "kind": "service",
            "displayName": "Remote Box",
            "_meta": {
                "role": "machine",
                "machineId": "machine_remote",
                "inventoryVersion": 1,
                "workspaceId": "default",
                "ownerActorId": account.actor_id,
                "name": "Remote Box",
                "kind": "remote",
                "dataRoot": "/home/canfeng/.agentx/machine_remote",
                "configDir": "/home/canfeng/.joi-apps",
                "providers": [{
                    "id": "claude",
                    "displayName": "Claude Code",
                    "command": "/usr/bin/claude",
                    "transportKind": "command",
                    "args": ["-p"],
                    "defaultModel": "claude-sonnet-4.6",
                    "modelChoices": []
                }],
                "agents": [{
                    "providerId": "claude",
                    "actorId": "actor_remote_agent",
                    "name": "Remote Agent",
                    "model": "claude-sonnet-4.6",
                    "autostart": true
                }]
            }
        });

        let machine = server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc")
            .expect("server machine");

        assert_eq!(machine.id, "machine_remote");
        assert_eq!(machine.connection_actor_id, "actor_service_machine_remote");
        assert_eq!(machine.agent_count, 1);
        assert_eq!(machine.providers[0].id, "claude");
        assert_eq!(machine.providers[0].actor_count, 1);
        assert_eq!(machine.agents[0].info.spec.actor.id, "actor_remote_agent");
        assert_eq!(
            machine.agents[0].profile_path,
            "/home/canfeng/.agentx/machine_remote/agents/actor_remote_agent/profile"
        );
    }

    #[test]
    fn server_machine_inventory_ignores_legacy_machine_actor_meta() {
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(test_account()),
            workspaces: vec![],
            machines: vec![],
        };
        let actor = json!({
            "id": "actor_service_machine_old",
            "kind": "service",
            "_meta": {
                "role": "machine",
                "machineId": "machine_old"
            }
        });

        assert!(server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc").is_none());
    }
}
