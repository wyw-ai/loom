//! Tauri commands exposed to the front-end.
//!
//! Two shapes of command here:
//! - **Workspace profile** management (`workspaces_list`, `workspaces_save`,
//!   `workspace_add`, `workspace_remove`, `set_active_workspace`): pure
//!   local-config manipulation; never touches the server.
//! - **Server RPCs** (`connect`, `channel_*`, `message_*`, `run_*`, …):
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
    config::load_or_init().map_err(|e| e.to_string())
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
    let account = account::login_oauth(&provider)
        .await
        .map_err(deep_stringify)?;
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
    let id = config::generate_id();
    let (actor_id, display_name) = workspace_identity_for_new_workspace(&cfg, &id);
    let ws = Workspace {
        id: id.clone(),
        name: args.name,
        server_url: args.server_url,
        actor_id: actor_id.clone(),
        display_name,
    };
    cfg.workspaces.push(ws);
    if args.activate {
        cfg.active = Some(id);
    }
    config::save(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

fn workspace_identity_for_new_workspace(
    cfg: &DesktopConfig,
    workspace_id: &str,
) -> (String, String) {
    if let Some(account) = cfg.account.as_ref() {
        return (account.actor_id.clone(), account_display_name(account));
    }
    let local_name = local_display_name();
    let actor_id = format!("actor_human_local_{}", local_identity_suffix(workspace_id));
    (actor_id, local_name)
}

fn local_display_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Local".into())
}

fn local_identity_suffix(value: &str) -> String {
    let suffix: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if suffix.is_empty() {
        "workspace".into()
    } else {
        suffix
    }
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
        .initialize("loom-gui", env!("CARGO_PKG_VERSION"))
        .await
        .map_err(deep_stringify)?;
    let open = client
        .open_connection(&ws.actor_id, Some(&ws.display_name))
        .await
        .map_err(deep_stringify)?;
    upsert_workspace_actor(&client, cfg.account.as_ref(), &ws)
        .await
        .map_err(deep_stringify)?;

    forward::spawn(app.clone(), Arc::clone(&state.inner), Arc::clone(&client));
    state.set(Some(client)).await;
    let _ = app.emit("loom://connection", forward::ConnectionEvent::Open);

    // Persist the chosen workspace as active and refresh the daemon's
    // server profile. Daemon runtime config is maintained by daemon startup,
    // not by GUI host discovery.
    cfg.active = Some(ws.id.clone());
    config::save(&cfg).map_err(stringify)?;

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

async fn upsert_workspace_actor(
    client: &Arc<Client>,
    account: Option<&HumanAccount>,
    workspace: &Workspace,
) -> anyhow::Result<()> {
    if let Some(account) = account {
        return upsert_human_actor(client, account).await;
    }
    client
        .call_raw(
            method::ACTOR_UPSERT,
            Some(json!({
                "actor": {
                    "id": workspace.actor_id,
                    "kind": "human",
                    "displayName": workspace.display_name,
                    "_meta": {
                        "account": {
                            "provider": "local",
                            "staffId": workspace.actor_id,
                        },
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
pub async fn thread_archive(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_ARCHIVE, Some(params))
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
pub async fn message_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::MESSAGE_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn message_send(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::MESSAGE_SEND, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn message_read(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::MESSAGE_READ, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn message_reaction_toggle(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::MESSAGE_REACTION_TOGGLE, Some(params))
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
pub async fn task_ref_attach(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_REF_ATTACH, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_ref_find(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_REF_FIND, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_ref_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_REF_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_artifact_attach(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_ARTIFACT_ATTACH, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_artifact_activate(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_ARTIFACT_ACTIVATE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_artifact_list(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_ARTIFACT_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_fact_append(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_FACT_APPEND, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_fact_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_FACT_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_projection_put(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_PROJECTION_PUT, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_projection_get(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_PROJECTION_GET, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_projection_list(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_PROJECTION_LIST, Some(params))
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
pub async fn task_assignment_context(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_ASSIGNMENT_CONTEXT, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_assignment_preflight(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_ASSIGNMENT_PREFLIGHT, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_change_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_CHANGE_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_change_ack(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_CHANGE_ACK, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn inbox_list(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::INBOX_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn delivery_ack(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::DELIVERY_ACK, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_workspace_lease_acquire(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_WORKSPACE_LEASE_ACQUIRE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_workspace_lease_release(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_WORKSPACE_LEASE_RELEASE, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn task_workspace_lease_list(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::TASK_WORKSPACE_LEASE_LIST, Some(params))
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
pub async fn run_cancel(state: State<'_, AppState>, params: Value) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::RUN_CANCEL, Some(params))
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
    #[serde(default)]
    pub avatar_url: String,
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
    let active_owner_actor_id = config::active_owner_actor_id(&cfg);
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
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[tauri::command]
pub async fn agent_update(
    state: State<'_, AppState>,
    args: AgentUpdateArgs,
) -> Result<AgentInfo, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    if let Some(machine_id) = args
        .machine_id
        .as_deref()
        .map(str::trim)
        .filter(|machine_id| !machine_id.is_empty())
    {
        if server_machine_by_id(&cfg, state.try_client().await, machine_id)
            .await
            .is_some()
        {
            let actor_id = args.actor_id.clone();
            let output = run_remote_machine_command(
                &state,
                &cfg,
                machine_id,
                json!({
                    "op": "agent.update",
                    "actorId": actor_id,
                    "displayName": args.display_name,
                    "description": args.description,
                    "providerId": args.provider_id,
                    "model": args.model,
                    "reasoningEffort": args.reasoning_effort,
                    "autostart": args.autostart,
                    "avatarUrl": args.avatar_url,
                }),
            )
            .await?;
            drop(output);
            let remote = server_machine_by_id(&cfg, state.try_client().await, machine_id)
                .await
                .ok_or_else(|| {
                    format!("machine inventory disappeared after update: {machine_id}")
                })?;
            let info = remote
                .agents
                .into_iter()
                .find(|agent| agent.info.spec.actor.id == actor_id)
                .map(|agent| agent.info)
                .ok_or_else(|| {
                    format!("updated agent not found in remote inventory: {}", actor_id)
                })?;
            upsert_agent_actor_to_server(state.try_client().await, &info).await;
            return Ok(info);
        }
    }
    if let Some(info) = update_machine_agent_in_config(&args).map_err(stringify)? {
        upsert_agent_actor_to_server(state.try_client().await, &info).await;
        return Ok(info);
    }
    Err(format!(
        "daemon-configured agent not found: {}",
        args.actor_id
    ))
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    pub workspace_id: Option<String>,
    pub owner_actor_id: Option<String>,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub source: String,
    pub read_only: bool,
    pub can_command: bool,
    pub can_open_local_path: bool,
    pub capabilities: Vec<String>,
    pub inventory_revision: u64,
    pub inventory_observed_at: Option<String>,
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

#[derive(Debug, Clone, serde::Serialize)]
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
    #[serde(default)]
    pub base_sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileFileResult {
    pub path: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[tauri::command]
pub async fn agent_profile_file_read(
    state: State<'_, AppState>,
    args: AgentProfileFileReadArgs,
) -> Result<AgentProfileFileResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    if server_machine_by_id(&cfg, state.try_client().await, &args.machine_id)
        .await
        .is_some()
    {
        let output = run_remote_machine_command(
            &state,
            &cfg,
            &args.machine_id,
            json!({
                "op": "agent.profile.read",
                "actorId": args.actor_id,
                "file": args.file,
            }),
        )
        .await?;
        return profile_file_result_from_output(output);
    }
    match resolve_machine_agent_profile_file(&args.machine_id, &args.actor_id, &args.file) {
        Ok(path) => {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            Ok(AgentProfileFileResult {
                path: config::home_path_expr(&path),
                text,
                sha256: None,
            })
        }
        Err(local_err) => Err(stringify(local_err)),
    }
}

#[tauri::command]
pub async fn agent_profile_file_write(
    state: State<'_, AppState>,
    args: AgentProfileFileWriteArgs,
) -> Result<AgentProfileFileResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    if server_machine_by_id(&cfg, state.try_client().await, &args.machine_id)
        .await
        .is_some()
    {
        let output = run_remote_machine_command(
            &state,
            &cfg,
            &args.machine_id,
            json!({
                "op": "agent.profile.write",
                "actorId": args.actor_id,
                "file": args.file,
                "text": args.text,
                "baseSha256": args.base_sha256,
            }),
        )
        .await?;
        return profile_file_result_from_output(output);
    }
    match resolve_machine_agent_profile_file(&args.machine_id, &args.actor_id, &args.file) {
        Ok(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create directory {}: {e}", parent.display()))?;
            }
            std::fs::write(&path, &args.text)
                .map_err(|e| format!("write {}: {e}", path.display()))?;
            Ok(AgentProfileFileResult {
                path: config::home_path_expr(&path),
                text: args.text,
                sha256: None,
            })
        }
        Err(local_err) => Err(stringify(local_err)),
    }
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
    let owner_actor_id = config::active_owner_actor_id(&cfg);
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
    let active_owner_actor_id = config::active_owner_actor_id(&cfg);
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
    let active_owner_actor_id = config::active_owner_actor_id(&cfg);
    let actor_id = actor_id_from_input(&args.actor_id, name, &machine_id).map_err(stringify)?;
    if server_machine_by_id(&cfg, state.try_client().await, &machine_id)
        .await
        .is_some()
    {
        let output = run_remote_machine_command(
            &state,
            &cfg,
            &machine_id,
            json!({
                "op": "agent.create",
                "providerId": args.provider_id,
                "actorId": actor_id,
                "name": name,
                "description": args.description,
                "model": args.model,
                "reasoningEffort": args.reasoning_effort,
                "autostart": args.autostart,
                "avatarUrl": args.avatar_url,
            }),
        )
        .await?;
        drop(output);
        return machines_from_config(&cfg, state.try_client().await).await;
    }
    let maybe_machine_index = cfg.machines.iter().position(|machine| {
        machine.id == machine_id
            && config::machine_belongs_to_workspace_and_owner(
                machine,
                active_workspace_id.as_deref(),
                active_owner_actor_id.as_deref(),
            )
    });
    let Some(machine_index) = maybe_machine_index else {
        return Err(format!("unknown machine id: {machine_id}"));
    };
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
        avatar_url: args.avatar_url.trim().to_string(),
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
    let active_owner_actor_id = config::active_owner_actor_id(&cfg);
    if server_machine_by_id(&cfg, state.try_client().await, &args.machine_id)
        .await
        .is_some()
    {
        let output = run_remote_machine_command(
            &state,
            &cfg,
            &args.machine_id,
            json!({
                "op": "agent.remove",
                "actorId": args.actor_id,
            }),
        )
        .await?;
        drop(output);
        return machines_from_config(&cfg, state.try_client().await).await;
    }
    let maybe_machine = cfg.machines.iter_mut().find(|machine| {
        machine.id == args.machine_id
            && config::machine_belongs_to_workspace_and_owner(
                machine,
                active_workspace_id.as_deref(),
                active_owner_actor_id.as_deref(),
            )
    });
    let Some(machine) = maybe_machine else {
        return Err(format!("unknown machine id: {}", args.machine_id));
    };
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

async fn upsert_agent_actor_to_server(client: Option<Arc<Client>>, info: &AgentInfo) {
    let Some(client) = client else {
        return;
    };
    if let Err(err) = client
        .call_raw(
            method::ACTOR_UPSERT,
            Some(json!({
                "actor": &info.spec.actor,
            })),
        )
        .await
    {
        tracing::warn!(
            actor_id = %info.spec.actor.id,
            %err,
            "failed to upsert updated agent actor to server"
        );
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
    let mut result = MachineListResult {
        machines: Vec::new(),
    };
    merge_server_machine_inventory(&mut result, cfg, client.as_ref(), server_url).await;
    apply_connection_status(&mut result, client).await;
    Ok(result)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteMachineMeta {
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
    config_dir: String,
    providers: Vec<DetectedAgentProvider>,
    agents: Vec<MachineAgentConfig>,
    capabilities: Vec<String>,
    revision: u64,
    observed_at: String,
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
        if let Some(meta) = remote_machine_meta_from_actor(actor) {
            if is_legacy_remote_machine_inventory(&meta) {
                cleanup_legacy_remote_machine_inventory(client, actor, &meta).await;
                continue;
            }
        }
        let Some(machine) = server_machine_info_from_actor(actor, cfg, server_url) else {
            continue;
        };
        upsert_server_machine_info(&mut result.machines, machine);
    }
}

fn upsert_server_machine_info(machines: &mut Vec<MachineInfo>, machine: MachineInfo) {
    if let Some(existing) = machines.iter_mut().find(|m| m.id == machine.id) {
        if existing.source == "local_config"
            || machine.inventory_revision >= existing.inventory_revision
        {
            *existing = machine;
        }
    } else {
        machines.push(machine);
    }
}

async fn server_machine_by_id(
    cfg: &DesktopConfig,
    client: Option<Arc<Client>>,
    machine_id: &str,
) -> Option<MachineInfo> {
    let client = client?;
    let server_url = active_server_url(cfg);
    let value = client.call_raw(method::ACTOR_LIST, None).await.ok()?;
    let actors = value.get("actors").and_then(Value::as_array)?;
    actors
        .iter()
        .filter_map(|actor| server_machine_info_from_actor(actor, cfg, server_url))
        .find(|machine| machine.id == machine_id)
}

async fn run_remote_machine_command(
    state: &State<'_, AppState>,
    cfg: &DesktopConfig,
    machine_id: &str,
    command: Value,
) -> Result<Value, String> {
    let client = state
        .try_client()
        .await
        .ok_or_else(|| "connect to the workspace before managing a remote machine".to_string())?;
    let machine = server_machine_by_id(cfg, Some(client.clone()), machine_id)
        .await
        .ok_or_else(|| format!("unknown machine id: {machine_id}"))?;
    if !machine.can_command {
        return Err(format!(
            "machine `{}` is read-only for the current account",
            machine.id
        ));
    }
    let operation = command
        .get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|op| !op.is_empty())
        .ok_or_else(|| "machine command op is required".to_string())?
        .to_string();
    let if_inventory_revision = if is_mutating_machine_operation(&operation) {
        Some(machine.inventory_revision)
    } else {
        None
    };
    let workspace_id = machine
        .workspace_id
        .as_deref()
        .or_else(|| config::active_workspace_id(cfg));
    let value = client
        .call_raw(
            method::MACHINE_COMMAND,
            Some(json!({
                "machineId": machine.id,
                "machineActorId": machine.connection_actor_id,
                "workspaceId": workspace_id,
                "ifInventoryRevision": if_inventory_revision,
                "command": command,
                "timeoutMs": 30_000,
            })),
        )
        .await
        .map_err(deep_stringify)?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("remote machine command failed");
        return Err(error.to_string());
    }
    Ok(value.get("output").cloned().unwrap_or(Value::Null))
}

fn is_mutating_machine_operation(operation: &str) -> bool {
    matches!(
        operation,
        "agent.create" | "agent.update" | "agent.remove" | "agent.profile.write"
    )
}

fn profile_file_result_from_output(output: Value) -> Result<AgentProfileFileResult, String> {
    let path = output
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "remote profile result missing path".to_string())?
        .to_string();
    let text = output
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "remote profile result missing text".to_string())?
        .to_string();
    let sha256 = output
        .get("sha256")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    Ok(AgentProfileFileResult { path, text, sha256 })
}

fn server_machine_info_from_actor(
    actor: &Value,
    cfg: &DesktopConfig,
    server_url: &str,
) -> Option<MachineInfo> {
    let meta = remote_machine_meta_from_actor(actor)?;
    if !is_complete_remote_machine_inventory(&meta) {
        return None;
    }
    if is_legacy_remote_machine_inventory(&meta) {
        return None;
    }
    let connection_actor_id = actor.get("id")?.as_str()?.to_string();
    let machine_id = meta.machine_id.clone();
    let name = meta.name.clone();
    let kind = meta.kind.clone();
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
    let config_dir = PathBuf::from(&meta.config_dir);
    let (serve_command, setup_script) =
        daemon_start_commands(&data_root, Some(&config_dir), server_url, &machine_id);

    let can_command = remote_machine_belongs_to_active_owner(&meta, cfg)
        && meta
            .capabilities
            .iter()
            .any(|capability| capability == "machine.command");
    let read_only = !can_command;
    Some(MachineInfo {
        workspace_id: meta.workspace_id,
        owner_actor_id: meta.owner_actor_id,
        id: machine_id,
        name,
        kind,
        source: "server_inventory".into(),
        read_only,
        can_command,
        can_open_local_path: false,
        capabilities: meta.capabilities,
        inventory_revision: meta.revision,
        inventory_observed_at: Some(meta.observed_at),
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

fn remote_machine_meta_from_actor(actor: &Value) -> Option<RemoteMachineMeta> {
    let meta_value = actor.get("_meta")?.clone();
    serde_json::from_value(meta_value).ok()
}

fn remote_machine_belongs_to_active_owner(meta: &RemoteMachineMeta, cfg: &DesktopConfig) -> bool {
    if let Some(owner_actor_id) = meta.owner_actor_id.as_deref() {
        if Some(owner_actor_id) != config::active_owner_actor_id(cfg).as_deref() {
            return false;
        }
    }
    true
}

fn is_complete_remote_machine_inventory(meta: &RemoteMachineMeta) -> bool {
    meta.role == "machine"
        && meta.source == "daemon"
        && meta.inventory_version == 2
        && !meta.machine_id.trim().is_empty()
        && !meta.name.trim().is_empty()
        && !meta.kind.trim().is_empty()
        && !meta.data_root.trim().is_empty()
        && !meta.config_dir.trim().is_empty()
        && !meta.capabilities.is_empty()
        && meta.revision != 0
        && !meta.observed_at.trim().is_empty()
}

fn is_legacy_remote_machine_inventory(meta: &RemoteMachineMeta) -> bool {
    meta.role == "machine"
        && meta.source == "daemon"
        && meta.machine_id.starts_with("machine_")
        && meta.machine_id.contains("_actor_human_")
}

async fn cleanup_legacy_remote_machine_inventory(
    client: &Arc<Client>,
    actor: &Value,
    meta: &RemoteMachineMeta,
) {
    let mut actor_ids = Vec::new();
    if let Some(actor_id) = actor.get("id").and_then(Value::as_str) {
        actor_ids.push(actor_id.to_string());
    }
    actor_ids.extend(
        meta.agents
            .iter()
            .map(|agent| agent.actor_id.clone())
            .filter(|actor_id| !is_supported_remote_actor_id(actor_id)),
    );
    delete_actors_from_server(Some(client.clone()), &actor_ids).await;
}

fn is_supported_remote_actor_id(actor_id: &str) -> bool {
    let trimmed = actor_id.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 64
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
}

async fn temporary_machine_check_client(cfg: &DesktopConfig) -> Option<Arc<Client>> {
    let client = Client::connect(active_server_url(cfg)).await.ok()?;
    client
        .initialize("loom-gui-machine-check", env!("CARGO_PKG_VERSION"))
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
    let mut allowed_agents = HashSet::new();

    if let Some(actors) = value.get("actors").and_then(Value::as_array) {
        for actor in actors {
            let Some(meta) = remote_machine_meta_from_actor(actor) else {
                continue;
            };
            if !is_complete_remote_machine_inventory(&meta)
                || !remote_machine_belongs_to_active_owner(&meta, cfg)
            {
                continue;
            }
            allowed_agents.extend(meta.agents.iter().map(|agent| agent.actor_id.clone()));
        }
    }

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
    let daemon_config_dir = config::daemon_config_dir_for_machine(machine);
    machine_info_with_daemon_config(machine, server_url, &daemon_config_dir)
}

fn machine_info_with_daemon_config(
    machine: &MachineConfig,
    server_url: &str,
    daemon_config_dir: &Path,
) -> anyhow::Result<MachineInfo> {
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
    let (serve_command, setup_script) =
        daemon_start_commands(&data_root, Some(daemon_config_dir), server_url, &machine.id);

    Ok(MachineInfo {
        workspace_id: machine.workspace_id.clone(),
        owner_actor_id: machine.owner_actor_id.clone(),
        id: machine.id.clone(),
        name: machine.name.clone(),
        kind: machine.kind.clone(),
        source: "local_config".into(),
        read_only: false,
        can_command: false,
        can_open_local_path: true,
        capabilities: vec![
            "inventory.read".into(),
            "agent.create".into(),
            "agent.remove".into(),
            "profile.write".into(),
            "machine.remove".into(),
        ],
        inventory_revision: 0,
        inventory_observed_at: None,
        status: setup_status.into(),
        setup_status: setup_status.into(),
        connection_status: "notConnected".into(),
        connection_actor_id,
        data_root: config::home_path_expr(&data_root),
        config_dir: config::home_path_expr(daemon_config_dir),
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

fn daemon_start_commands(
    data_root: &Path,
    config_dir: Option<&Path>,
    server_url: &str,
    machine_id: &str,
) -> (String, String) {
    let data_root_arg = shell_path_arg(data_root);
    let config_dir_arg = config_dir.map(shell_path_arg);
    let daemon_bin = preferred_daemon_binary()
        .map(|path| shell_path_arg(&path))
        .unwrap_or_else(|| "loom-daemon".into());
    let serve_command = if let Some(config_dir_arg) = config_dir_arg.as_ref() {
        format!(
            "LOOM_CONFIG_DIR={} LOOM_AGENT_DATA_ROOT={} {} --server {} --machine-id {}",
            config_dir_arg,
            data_root_arg,
            daemon_bin,
            shell_arg(server_url),
            shell_arg(machine_id),
        )
    } else {
        format!(
            "LOOM_AGENT_DATA_ROOT={} {} --server {} --machine-id {}",
            data_root_arg,
            daemon_bin,
            shell_arg(server_url),
            shell_arg(machine_id),
        )
    };
    let mkdir_args = if let Some(config_dir_arg) = config_dir_arg.as_ref() {
        format!("{} {}", shell_path_arg(data_root), config_dir_arg)
    } else {
        shell_path_arg(data_root)
    };
    let config_export = config_dir_arg
        .as_ref()
        .map(|config_dir_arg| format!("export LOOM_CONFIG_DIR={config_dir_arg}\n"))
        .unwrap_or_default();
    let setup_script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p {}\n{}export LOOM_AGENT_DATA_ROOT={}\nif [[ -z \"${{LOOM_DAEMON_BIN:-}}\" ]]; then\n  LOOM_DAEMON_BIN={}\nfi\nif [[ ! -x \"$LOOM_DAEMON_BIN\" ]]; then\n  if command -v \"$LOOM_DAEMON_BIN\" >/dev/null 2>&1; then\n    LOOM_DAEMON_BIN=\"$(command -v \"$LOOM_DAEMON_BIN\")\"\n  else\n    LOOM_DAEMON_BIN=\"$(command -v loom-daemon)\"\n  fi\nfi\nexec \"$LOOM_DAEMON_BIN\" --server {} --machine-id {}\n",
        mkdir_args,
        config_export,
        shell_path_arg(data_root),
        daemon_bin,
        shell_arg(server_url),
        shell_arg(machine_id),
    );
    (serve_command, setup_script)
}

fn preferred_daemon_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LOOM_DAEMON_BIN")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }

    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("loom-daemon"));
            if dir.file_name().and_then(|name| name.to_str()) == Some("MacOS") {
                if let Some(contents_dir) = dir.parent() {
                    candidates.push(
                        contents_dir
                            .join("Resources")
                            .join("bin")
                            .join("loom-daemon"),
                    );
                }
            }
        }
    }
    if let Some(triple) = host_runtime_target_triple() {
        candidates.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("dist")
                .join("release")
                .join(triple)
                .join("loom-daemon"),
        );
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn host_runtime_target_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        _ => None,
    }
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
        avatar_url: non_empty(agent.avatar_url.trim()),
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
    let active_owner_actor_id = config::active_owner_actor_id(&cfg);
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
        if let Some(avatar_url) = args.avatar_url.as_deref() {
            agent.avatar_url = avatar_url.trim().to_string();
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
    const ACTOR_ID_PREFIX: &str = "actor_agent_";
    const ACTOR_ID_SUFFIX_LEN: usize = 8;
    const MAX_ACTOR_ID_LEN: usize = 64;

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
        let max_stem_len = MAX_ACTOR_ID_LEN - ACTOR_ID_PREFIX.len() - 1 - ACTOR_ID_SUFFIX_LEN;
        let stem = truncate_slug(&stem, max_stem_len);
        return Ok(format!("{ACTOR_ID_PREFIX}{stem}_{suffix}"));
    }
    if trimmed.len() > MAX_ACTOR_ID_LEN {
        anyhow::bail!("actor id must be at most {MAX_ACTOR_ID_LEN} characters")
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
    let compact = compact
        .split_once("_actor_")
        .map(|(head, _)| head)
        .unwrap_or(compact);
    format!("machine_{compact}")
}

fn truncate_slug(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    value.chars().take(max_len).collect()
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
            provider: "github".into(),
            staff_id: "88084".into(),
            nickname: "octocat".into(),
            real_name: "Octo Cat".into(),
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
                avatar_url: String::new(),
            }],
        }
    }

    #[test]
    fn generated_actor_id_uses_machine_stem_for_short_ascii_name_fragments() {
        let id = actor_id_from_input("", "G仔", "machine_macbook_01").expect("actor id");

        assert!(id.starts_with("actor_agent_machine_macbook_01_"));
        assert!(id.len() <= 64);
    }

    #[test]
    fn generated_actor_id_can_use_meaningful_display_slug() {
        let id = actor_id_from_input("", "Reviewer", "machine_macbook_01").expect("actor id");

        assert!(id.starts_with("actor_agent_reviewer_"));
        assert!(id.len() <= 64);
    }

    #[test]
    fn generated_actor_id_truncates_long_machine_owner_slug() {
        let id = actor_id_from_input("", "Q", "machine_abbb0e0b_actor_human_local_ws_abbb0e0b")
            .expect("actor id");

        assert!(id.starts_with("actor_agent_machine_abbb0e0b_"));
        assert!(id.len() <= 64);
    }

    #[test]
    fn explicit_actor_id_is_preserved_when_valid() {
        let id = actor_id_from_input("actor_agent_custom:01", "G仔", "machine_macbook_01")
            .expect("actor id");

        assert_eq!(id, "actor_agent_custom:01");
    }

    #[test]
    fn explicit_actor_id_rejects_overlong_values() {
        let id = format!("actor_agent_{}", "x".repeat(80));

        let err = actor_id_from_input(&id, "G仔", "machine_macbook_01").expect_err("overlong id");

        assert!(err.to_string().contains("at most 64"));
    }

    #[test]
    fn local_workspace_identity_does_not_require_oauth_account() {
        let cfg = DesktopConfig::default();

        let (actor_id, display_name) = workspace_identity_for_new_workspace(&cfg, "ws_local");

        assert_eq!(actor_id, "actor_human_local_ws_local");
        assert!(!display_name.trim().is_empty());
    }

    #[test]
    fn daemon_start_command_uses_daemon_binary() {
        let (serve_command, setup_script) = daemon_start_commands(
            Path::new("/tmp/loom data"),
            Some(Path::new("/tmp/loom config")),
            "ws://127.0.0.1:7878/rpc",
            "machine_test",
        );

        assert!(
            serve_command.contains("--server ws://127.0.0.1:7878/rpc --machine-id machine_test")
        );
        assert!(serve_command.starts_with("LOOM_CONFIG_DIR="));
        assert!(serve_command.contains("LOOM_AGENT_DATA_ROOT="));
        assert!(!serve_command.contains(" daemon --machine-id "));
        assert!(setup_script.contains("export LOOM_CONFIG_DIR="));
        assert!(setup_script.contains("LOOM_DAEMON_BIN"));
        assert!(setup_script.contains(
            "exec \"$LOOM_DAEMON_BIN\" --server ws://127.0.0.1:7878/rpc --machine-id machine_test"
        ));
        assert!(!setup_script.contains(" daemon --machine-id "));
    }

    #[test]
    fn actor_list_filter_ignores_local_machine_agents() {
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

        assert!(!actor_ids.contains(&"actor_agent_mine"));
        assert!(!actor_ids.contains(&"actor_agent_other"));
        assert!(actor_ids.contains(&"actor_service_other"));
    }

    #[test]
    fn actor_list_filter_allows_owned_agents_from_server_machine_inventory_across_workspace_ids() {
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: None,
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: "actor_human_88084".into(),
                display_name: "actor_human_88084".into(),
            }],
            machines: Vec::new(),
        };
        let value = json!({
            "actors": [
                {
                    "id": "actor_service_machine_remote",
                    "kind": "service",
                    "displayName": "Remote Box",
                    "_meta": {
                        "role": "machine",
                        "source": "daemon",
                        "machineId": "machine_remote",
                        "inventoryVersion": 2,
                        "revision": 7,
                        "observedAt": "2026-05-13T10:50:00Z",
                        "workspaceId": "server_workspace",
                        "ownerActorId": "actor_human_88084",
                        "name": "Remote Box",
                        "kind": "remote",
                        "dataRoot": "/home/canfeng/.agentx/machine_remote",
                        "configDir": "/home/canfeng/.loom-apps",
                        "capabilities": ["inventory.read", "connection.status", "machine.command"],
                        "providers": [{
                            "id": "codex",
                            "displayName": "Codex CLI",
                            "command": "/usr/bin/codex",
                            "transportKind": "command",
                            "args": ["exec"],
                            "defaultModel": "gpt-5.5",
                            "modelChoices": []
                        }],
                        "agents": [{
                            "providerId": "codex",
                            "actorId": "actor_remote_agent",
                            "name": "Remote Agent",
                            "model": "gpt-5.5",
                            "autostart": true
                        }]
                    }
                },
                { "id": "actor_remote_agent", "kind": "agent", "displayName": "Remote Agent" },
                { "id": "actor_other_agent", "kind": "agent", "displayName": "Other Agent" }
            ]
        });

        let filtered = filter_actor_list_for_active_context(value, &cfg);
        let actor_ids = filtered["actors"]
            .as_array()
            .expect("actors")
            .iter()
            .filter_map(|actor| actor["id"].as_str())
            .collect::<Vec<_>>();

        assert!(actor_ids.contains(&"actor_remote_agent"));
        assert!(!actor_ids.contains(&"actor_other_agent"));
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
                "source": "daemon",
                "machineId": "machine_remote",
                "inventoryVersion": 2,
                "revision": 7,
                "observedAt": "2026-05-13T10:50:00Z",
                "workspaceId": "default",
                "ownerActorId": account.actor_id,
                "name": "Remote Box",
                "kind": "remote",
                "dataRoot": "/home/canfeng/.agentx/machine_remote",
                "configDir": "/home/canfeng/.loom-apps",
                "capabilities": ["inventory.read", "connection.status", "machine.command"],
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
        assert_eq!(machine.source, "server_inventory");
        assert!(!machine.read_only);
        assert_eq!(
            machine.capabilities,
            vec![
                "inventory.read".to_string(),
                "connection.status".to_string(),
                "machine.command".to_string()
            ]
        );
        assert_eq!(machine.inventory_revision, 7);
        assert_eq!(
            machine.inventory_observed_at.as_deref(),
            Some("2026-05-13T10:50:00Z")
        );
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
    fn server_machine_inventory_is_visible_across_local_workspace_profile_ids() {
        let account = test_account();
        let cfg = DesktopConfig {
            active: Some("local_profile".into()),
            account: Some(account.clone()),
            workspaces: vec![Workspace {
                id: "local_profile".into(),
                name: "Remote Server".into(),
                server_url: "ws://example/rpc".into(),
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
                "source": "daemon",
                "machineId": "machine_remote",
                "inventoryVersion": 2,
                "revision": 7,
                "observedAt": "2026-05-13T10:50:00Z",
                "workspaceId": "server_workspace",
                "ownerActorId": account.actor_id,
                "name": "Remote Box",
                "kind": "remote",
                "dataRoot": "/home/canfeng/.agentx/machine_remote",
                "configDir": "/home/canfeng/.loom-apps",
                "capabilities": ["inventory.read", "connection.status", "machine.command"],
                "providers": [],
                "agents": []
            }
        });

        let machine = server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc")
            .expect("server machine must not be hidden by local GUI workspace id");

        assert_eq!(machine.workspace_id.as_deref(), Some("server_workspace"));
        assert!(!machine.read_only);
        assert!(machine.can_command);
    }

    #[test]
    fn server_machine_inventory_from_other_owner_is_visible_but_read_only() {
        let account = test_account();
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(account.clone()),
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Remote Server".into(),
                server_url: "ws://example/rpc".into(),
                actor_id: account.actor_id.clone(),
                display_name: account_display_name(&account),
            }],
            machines: vec![],
        };
        let actor = json!({
            "id": "actor_service_machine_other",
            "kind": "service",
            "displayName": "Other Box",
            "_meta": {
                "role": "machine",
                "source": "daemon",
                "machineId": "machine_other",
                "inventoryVersion": 2,
                "revision": 4,
                "observedAt": "2026-05-13T10:50:00Z",
                "workspaceId": "server_workspace",
                "ownerActorId": "actor_human_other",
                "name": "Other Box",
                "kind": "remote",
                "dataRoot": "/home/other/.agentx/machine_other",
                "configDir": "/home/other/.loom-apps",
                "capabilities": ["inventory.read", "connection.status", "machine.command"],
                "providers": [],
                "agents": []
            }
        });

        let machine = server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc")
            .expect("other-owner daemon should still be discoverable");

        assert_eq!(machine.id, "machine_other");
        assert!(machine.read_only);
        assert!(!machine.can_command);
    }

    #[test]
    fn server_machine_inventory_replaces_local_stub_with_same_id() {
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
                "source": "daemon",
                "machineId": "machine_remote",
                "inventoryVersion": 2,
                "revision": 3,
                "observedAt": "2026-05-13T10:50:00Z",
                "workspaceId": "default",
                "ownerActorId": account.actor_id,
                "name": "Remote Box",
                "kind": "remote",
                "dataRoot": "/home/canfeng/.agentx/machine_remote",
                "configDir": "/home/canfeng/.loom-apps",
                "capabilities": ["inventory.read", "connection.status", "machine.command"],
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
        let server_machine = server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc")
            .expect("server machine");
        let mut local_stub = server_machine.clone();
        local_stub.source = "local_config".into();
        local_stub.inventory_revision = 0;
        local_stub.agents.clear();
        local_stub.agent_count = 0;
        let mut machines = vec![local_stub];

        upsert_server_machine_info(&mut machines, server_machine);

        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].source, "server_inventory");
        assert_eq!(machines[0].agent_count, 1);
        assert_eq!(
            machines[0].agents[0].info.spec.actor.id,
            "actor_remote_agent"
        );
    }

    #[test]
    fn server_machine_inventory_ignores_legacy_machine_actor_meta() {
        let account = test_account();
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(account.clone()),
            workspaces: vec![],
            machines: vec![],
        };
        let actor = json!({
            "id": "actor_service_machine_abbb0e0b_actor_human_local_ws_abbb0e0b",
            "kind": "service",
            "displayName": "Old Local Machine",
            "_meta": {
                "role": "machine",
                "source": "daemon",
                "machineId": "machine_abbb0e0b_actor_human_local_ws_abbb0e0b",
                "inventoryVersion": 2,
                "revision": 7,
                "observedAt": "2026-05-13T10:50:00Z",
                "workspaceId": "default",
                "ownerActorId": account.actor_id,
                "name": "Old Local Machine",
                "kind": "local",
                "dataRoot": "/Users/boyd/.agentx/machines/ws_abbb0e0b/actor_human_local_ws_abbb0e0b/local",
                "configDir": "/Users/boyd/.loom-apps",
                "capabilities": ["inventory.read", "connection.status", "machine.command", "agent.create"],
                "providers": [],
                "agents": []
            }
        });

        assert!(server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc").is_none());
    }
}
