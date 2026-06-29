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

use agent_runtime::discovery::DetectedAgentProvider;
use proto::methods::method;
use proto::methods::{AgentInfo, AgentListResult, AgentModelChoice, AgentSpec, ServiceSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::config::{
    self, account_display_name, apply_account_identity, DesktopConfig, HumanAccount, Workspace,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLoginProviderStatus {
    pub provider: String,
    pub display_name: String,
    pub available: bool,
    pub missing_env: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountAuthStatus {
    pub providers: Vec<AccountLoginProviderStatus>,
}

#[tauri::command]
pub async fn account_auth_status() -> Result<AccountAuthStatus, String> {
    Ok(AccountAuthStatus {
        providers: account::OAuthProvider::all()
            .into_iter()
            .map(|provider| {
                let available = provider.has_client_id();
                AccountLoginProviderStatus {
                    provider: provider.id().into(),
                    display_name: provider.display().into(),
                    available,
                    missing_env: (!available).then(|| provider.client_id_env_name().into()),
                }
            })
            .collect(),
    })
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
        server_url: config::normalize_workspace_server_url(&args.server_url).map_err(stringify)?,
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
pub async fn agent_list(state: State<'_, AppState>) -> Result<AgentListResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    agent_list_from_daemon_inventory(&cfg, state.try_client().await).await
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
    pub instructions: String,
    #[serde(default)]
    pub prompt_assembly: Option<Value>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub env: Option<std::collections::BTreeMap<String, String>>,
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
    let cfg = config::load_or_init().map_err(stringify)?;
    let client = state
        .try_client()
        .await
        .ok_or_else(|| "connect to the workspace before managing agents".to_string())?;
    let machine = server_machine_for_agent(&cfg, Some(client), &args.actor_id)
        .await
        .ok_or_else(|| {
            format!(
                "agent `{}` is not present in daemon inventory; start the daemon before editing it",
                args.actor_id
            )
        })?;
    run_remote_machine_command(
        &state,
        &cfg,
        &machine.id,
        json!({
            "op": "agent.remove",
            "actorId": args.actor_id,
        }),
    )
    .await?;
    agent_list_from_daemon_inventory(&cfg, state.try_client().await).await
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
    pub instructions: Option<String>,
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
    #[serde(default)]
    pub prompt_assembly: Option<Value>,
    #[serde(default)]
    pub env: Option<std::collections::BTreeMap<String, String>>,
}

#[tauri::command]
pub async fn agent_update(
    state: State<'_, AppState>,
    args: AgentUpdateArgs,
) -> Result<AgentInfo, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    let client = state
        .try_client()
        .await
        .ok_or_else(|| "connect to the workspace before managing agents".to_string())?;
    let target_machine_id = if let Some(machine_id) = args
        .machine_id
        .as_deref()
        .map(str::trim)
        .filter(|machine_id| !machine_id.is_empty())
    {
        server_machine_by_id(&cfg, Some(client.clone()), machine_id)
            .await
            .ok_or_else(|| {
                format!(
                    "machine `{machine_id}` is not present in daemon inventory; start the daemon before editing agents"
                )
            })?
            .id
    } else {
        server_machine_for_agent(&cfg, Some(client), &args.actor_id)
            .await
            .ok_or_else(|| {
                format!(
                    "agent `{}` is not present in daemon inventory; start the daemon before editing it",
                    args.actor_id
                )
            })?
            .id
    };
    let actor_id = args.actor_id.clone();
    let mut command = json!({
        "op": "agent.update",
        "actorId": actor_id,
        "displayName": args.display_name,
        "description": args.description,
        "instructions": args.instructions,
        "providerId": args.provider_id,
        "model": args.model,
        "reasoningEffort": args.reasoning_effort,
        "autostart": args.autostart,
        "avatarUrl": args.avatar_url,
        "env": args.env,
    });
    if let Some(prompt_assembly) = args.prompt_assembly {
        command["promptAssembly"] = prompt_assembly;
    }
    let output = run_remote_machine_command(&state, &cfg, &target_machine_id, command).await?;
    agent_info_from_machine_command_output(output)
        .ok_or_else(|| format!("updated agent not returned by daemon: {}", args.actor_id))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptPreviewArgs {
    pub machine_id: String,
    pub actor_id: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub scope: Option<Value>,
    #[serde(default)]
    pub sample_message: Option<String>,
    #[serde(default)]
    pub prompt_assembly: Option<Value>,
}

#[tauri::command]
pub async fn agent_prompt_preview(
    state: State<'_, AppState>,
    args: AgentPromptPreviewArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    ensure_server_machine_present(&cfg, &state, &args.machine_id).await?;
    let mut command = json!({
        "op": "agent.prompt.preview",
        "actorId": args.actor_id,
        "channelId": args.channel_id,
        "scope": args.scope,
        "sampleMessage": args.sample_message,
    });
    if let Some(prompt_assembly) = args.prompt_assembly {
        command["promptAssembly"] = prompt_assembly;
    }
    run_remote_machine_command(&state, &cfg, &args.machine_id, command).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFileListArgs {
    pub machine_id: String,
    pub actor_id: String,
    pub root: String,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub scope: Option<Value>,
}

#[tauri::command]
pub async fn agent_file_list(
    state: State<'_, AppState>,
    args: AgentFileListArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    ensure_server_machine_present(&cfg, &state, &args.machine_id).await?;
    run_remote_machine_command(
        &state,
        &cfg,
        &args.machine_id,
        json!({
            "op": "agent.file.list",
            "actorId": args.actor_id,
            "root": args.root,
            "prefix": args.prefix,
            "channelId": args.channel_id,
            "scope": args.scope,
        }),
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFileReadArgs {
    pub machine_id: String,
    pub actor_id: String,
    pub root: String,
    pub path: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub scope: Option<Value>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

#[tauri::command]
pub async fn agent_file_read(
    state: State<'_, AppState>,
    args: AgentFileReadArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    ensure_server_machine_present(&cfg, &state, &args.machine_id).await?;
    run_remote_machine_command(
        &state,
        &cfg,
        &args.machine_id,
        json!({
            "op": "agent.file.read",
            "actorId": args.actor_id,
            "root": args.root,
            "path": args.path,
            "channelId": args.channel_id,
            "scope": args.scope,
            "maxBytes": args.max_bytes,
        }),
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFileWriteArgs {
    pub machine_id: String,
    pub actor_id: String,
    pub root: String,
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub scope: Option<Value>,
}

#[tauri::command]
pub async fn agent_file_write(
    state: State<'_, AppState>,
    args: AgentFileWriteArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    ensure_server_machine_present(&cfg, &state, &args.machine_id).await?;
    run_remote_machine_command(
        &state,
        &cfg,
        &args.machine_id,
        json!({
            "op": "agent.file.write",
            "actorId": args.actor_id,
            "root": args.root,
            "path": args.path,
            "content": args.content,
            "channelId": args.channel_id,
            "scope": args.scope,
        }),
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAddArgs {
    pub machine_id: String,
    pub manifest: Value,
    #[serde(default)]
    pub replace: Option<bool>,
}

#[tauri::command]
pub async fn provider_add(
    state: State<'_, AppState>,
    args: ProviderAddArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    ensure_server_machine_present(&cfg, &state, &args.machine_id).await?;
    run_remote_machine_command(
        &state,
        &cfg,
        &args.machine_id,
        json!({
            "op": "provider.add",
            "manifest": args.manifest,
            "replace": args.replace.unwrap_or(false),
        }),
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRemoveArgs {
    pub machine_id: String,
    pub provider_id: String,
}

#[tauri::command]
pub async fn provider_remove(
    state: State<'_, AppState>,
    args: ProviderRemoveArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    ensure_server_machine_present(&cfg, &state, &args.machine_id).await?;
    run_remote_machine_command(
        &state,
        &cfg,
        &args.machine_id,
        json!({
            "op": "provider.remove",
            "providerId": args.provider_id,
        }),
    )
    .await
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
    pub service_count: usize,
    pub providers: Vec<MachineAgentProviderInfo>,
    pub agents: Vec<MachineAgentInfo>,
    pub services: Vec<ServiceSpec>,
    pub serve_command: String,
    pub setup_script: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAgentInfo {
    #[serde(flatten)]
    pub info: AgentInfo,
    pub profile_path: String,
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
pub struct MachineCreateArgs {
    pub name: String,
    #[serde(default)]
    pub data_root: Option<String>,
}

#[tauri::command]
pub async fn machine_create(
    state: State<'_, AppState>,
    args: MachineCreateArgs,
) -> Result<MachineListResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    let name = args.name.trim();
    if name.is_empty() {
        return Err("host name is required".into());
    }
    let machine = pending_machine_registration(&cfg, name, args.data_root)?;
    save_pending_machine_registration(&machine)?;
    let mut result = machines_from_config(&cfg, state.try_client().await).await?;
    upsert_server_machine_info(&mut result.machines, machine);
    Ok(result)
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
    let cfg = config::load_or_init().map_err(stringify)?;
    let machine_id = args.machine_id.trim();
    // `machine_id` is joined into a filesystem path and remove_dir_all'd
    // below, so it must be a plain identifier. Reject anything that could
    // escape the daemon-configs/ directory (path traversal → arbitrary
    // directory deletion). Since no path separators are allowed, traversal
    // via `..` segments is impossible; `.`/`..` alone are rejected too
    // (they would delete the daemon-configs dir itself).
    if machine_id.is_empty()
        || machine_id.contains('\\')
        || machine_id.contains('/')
        || machine_id.contains('\0')
        || machine_id == "."
        || machine_id == ".."
    {
        return Err("invalid machine id".into());
    }

    // Try to find this machine on the server and delete its actor.
    // Server machines are registered as service actors with _meta.role == "machine".
    if let Some(client) = state.try_client().await {
        if let Ok(value) = client.call_raw(method::ACTOR_LIST, None).await {
            if let Some(actors) = value.get("actors").and_then(Value::as_array) {
                for actor in actors {
                    let meta = actor.get("_meta");
                    let machine_meta_id = meta
                        .and_then(|m| m.get("machineId"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let is_service = actor
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(|k| k == "service")
                        .unwrap_or(false);
                    if is_service && machine_meta_id == machine_id {
                        if let Some(actor_id) = actor.get("id").and_then(Value::as_str) {
                            delete_actors_from_server(
                                Some(client.clone()),
                                &[actor_id.to_string()],
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }

    // Clean up local daemon config directory.
    let config_dir = config::config_dir().join("daemon-configs").join(machine_id);
    if config_dir.exists() {
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    // Return updated machine list.
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
    let cfg = config::load_or_init().map_err(stringify)?;
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
                "instructions": args.instructions,
                "promptAssembly": args.prompt_assembly,
                "model": args.model,
                "reasoningEffort": args.reasoning_effort,
                "autostart": args.autostart,
                "avatarUrl": args.avatar_url,
                "env": args.env,
            }),
        )
        .await?;
        drop(output);
        return machines_from_config(&cfg, state.try_client().await).await;
    }
    Err(format!(
        "machine `{machine_id}` is not present in daemon inventory; start the daemon before creating agents"
    ))
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
                "op": "agent.remove",
                "actorId": args.actor_id,
            }),
        )
        .await?;
        drop(output);
        return machines_from_config(&cfg, state.try_client().await).await;
    }
    Err(format!(
        "machine `{}` is not present in daemon inventory; start the daemon before removing agents",
        args.machine_id
    ))
}

async fn ensure_server_machine_present(
    cfg: &DesktopConfig,
    state: &State<'_, AppState>,
    machine_id: &str,
) -> Result<(), String> {
    let machine_id = machine_id.trim();
    if machine_id.is_empty() {
        return Err("machine id is required".into());
    }
    server_machine_by_id(cfg, state.try_client().await, machine_id)
        .await
        .map(|_| ())
        .ok_or_else(|| {
            format!(
                "machine `{machine_id}` is not present in daemon inventory; start the daemon before managing agents"
            )
        })
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
        machines: pending_machine_infos(cfg)?,
    };
    merge_server_machine_inventory(&mut result, cfg, client.as_ref(), server_url).await?;
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
    #[serde(default, rename = "agentSpecs")]
    agent_specs: Vec<AgentSpec>,
    #[serde(default, rename = "serviceSpecs")]
    service_specs: Vec<ServiceSpec>,
    capabilities: Vec<String>,
    revision: u64,
    observed_at: String,
}

async fn merge_server_machine_inventory(
    result: &mut MachineListResult,
    cfg: &DesktopConfig,
    client: Option<&Arc<Client>>,
    server_url: &str,
) -> Result<(), String> {
    let Some(client) = client else {
        return Ok(());
    };
    let value = client
        .call_raw(method::ACTOR_LIST, None)
        .await
        .map_err(deep_stringify)?;
    let actors = value
        .get("actors")
        .and_then(Value::as_array)
        .ok_or_else(|| "actor/list returned malformed machine inventory response".to_string())?;
    for actor in actors {
        if let Some(meta) = remote_machine_meta_from_actor(actor) {
            if is_legacy_remote_machine_inventory(&meta) {
                cleanup_legacy_remote_machine_inventory(client, actor).await;
                continue;
            }
        }
        let Some(machine) = server_machine_info_from_actor(actor, cfg, server_url) else {
            continue;
        };
        upsert_server_machine_info(&mut result.machines, machine);
    }
    Ok(())
}

fn upsert_server_machine_info(machines: &mut Vec<MachineInfo>, machine: MachineInfo) {
    if let Some(existing) = machines.iter_mut().find(|m| m.id == machine.id) {
        if machine.inventory_revision > existing.inventory_revision
            || (machine.inventory_revision == existing.inventory_revision
                && machine.inventory_observed_at > existing.inventory_observed_at)
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

async fn server_machine_for_agent(
    cfg: &DesktopConfig,
    client: Option<Arc<Client>>,
    actor_id: &str,
) -> Option<MachineInfo> {
    let client = client?;
    let server_url = active_server_url(cfg);
    let value = client.call_raw(method::ACTOR_LIST, None).await.ok()?;
    let actors = value.get("actors").and_then(Value::as_array)?;
    actors
        .iter()
        .filter_map(|actor| server_machine_info_from_actor(actor, cfg, server_url))
        .find(|machine| {
            machine
                .agents
                .iter()
                .any(|agent| agent.info.spec.actor.id == actor_id)
        })
}

async fn agent_list_from_daemon_inventory(
    cfg: &DesktopConfig,
    client: Option<Arc<Client>>,
) -> Result<AgentListResult, String> {
    let machines = machines_from_config(cfg, client).await?;
    Ok(AgentListResult {
        agents: machines
            .machines
            .into_iter()
            .flat_map(|machine| machine.agents.into_iter().map(|agent| agent.info))
            .collect(),
    })
}

fn agent_info_from_machine_command_output(output: Value) -> Option<AgentInfo> {
    let spec: AgentSpec = serde_json::from_value(output.get("agentSpec")?.clone()).ok()?;
    Some(AgentInfo {
        spec,
        status: "registered".into(),
        pid: None,
        session_id: None,
    })
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
        "agent.create"
            | "agent.update"
            | "agent.remove"
            | "agent.file.write"
            | "provider.add"
            | "provider.remove"
    )
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
    let agent_specs = meta.agent_specs.clone();
    for provider in &mut providers {
        provider.actor_count += agent_specs
            .iter()
            .filter(|spec| spec.provider_ref.id.as_str() == provider.id.as_str())
            .count();
    }
    let agents: Vec<MachineAgentInfo> = agent_specs
        .into_iter()
        .map(|spec| {
            let profile_path = agent_profile_path(&data_root, &spec);
            MachineAgentInfo {
                info: AgentInfo {
                    spec,
                    status: "registered".into(),
                    pid: None,
                    session_id: None,
                },
                profile_path: profile_path.display().to_string(),
            }
        })
        .collect();
    let services = meta.service_specs.clone();
    let setup_status = if providers.is_empty() {
        "noCli"
    } else if agents.is_empty() {
        "ready"
    } else {
        "configured"
    };
    let config_dir = PathBuf::from(&meta.config_dir);
    let (serve_command, setup_script) = daemon_start_commands(
        &data_root,
        Some(&config_dir),
        server_url,
        &machine_id,
        &name,
    );

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
        service_count: services.len(),
        providers,
        agents,
        services,
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

async fn cleanup_legacy_remote_machine_inventory(client: &Arc<Client>, actor: &Value) {
    let mut actor_ids = Vec::new();
    if let Some(actor_id) = actor.get("id").and_then(Value::as_str) {
        actor_ids.push(actor_id.to_string());
    }
    delete_actors_from_server(Some(client.clone()), &actor_ids).await;
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
        .unwrap_or(config::DEFAULT_SERVER_URL)
}

fn pending_machine_registration(
    cfg: &DesktopConfig,
    name: &str,
    data_root: Option<String>,
) -> Result<MachineInfo, String> {
    const MACHINE_ID_SUFFIX_LEN: usize = 8;
    const MAX_MACHINE_ID_LEN: usize = 64;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let slug = slugify(name);
    let slug = if slug == "agent" { "host".into() } else { slug };
    let max_slug_len = MAX_MACHINE_ID_LEN - "machine_".len() - 1 - MACHINE_ID_SUFFIX_LEN;
    let machine_id = format!(
        "machine_{}_{}",
        truncate_slug(&slug, max_slug_len),
        &suffix[..MACHINE_ID_SUFFIX_LEN],
    );
    let data_root = data_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| normalize_local_path(config::expand_home(value)).map_err(stringify))
        .transpose()?
        .unwrap_or_else(|| default_machine_data_root(&machine_id));
    let config_dir = config::config_dir()
        .join("daemon-configs")
        .join(&machine_id);
    std::fs::create_dir_all(&data_root)
        .map_err(|e| format!("create data root {}: {e}", data_root.display()))?;
    std::fs::create_dir_all(&config_dir)
        .map_err(|e| format!("create config directory {}: {e}", config_dir.display()))?;
    let server_url = active_server_url(cfg);
    let (serve_command, setup_script) =
        daemon_start_commands(&data_root, Some(&config_dir), server_url, &machine_id, name);

    Ok(MachineInfo {
        workspace_id: config::active_workspace_id(cfg).map(str::to_string),
        owner_actor_id: config::active_owner_actor_id(cfg),
        id: machine_id,
        name: name.to_string(),
        kind: "local".into(),
        source: "local_registration".into(),
        read_only: true,
        can_command: false,
        can_open_local_path: true,
        capabilities: vec!["machine.register".into()],
        inventory_revision: 0,
        inventory_observed_at: None,
        status: "pending".into(),
        setup_status: "pending".into(),
        connection_status: "notConnected".into(),
        connection_actor_id: String::new(),
        data_root: data_root.display().to_string(),
        config_dir: config_dir.display().to_string(),
        agent_count: 0,
        online_agent_count: 0,
        service_count: 0,
        providers: Vec::new(),
        agents: Vec::new(),
        services: Vec::new(),
        serve_command,
        setup_script,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingMachineRegistration {
    workspace_id: Option<String>,
    owner_actor_id: Option<String>,
    id: String,
    name: String,
    kind: String,
    data_root: String,
    config_dir: String,
}

fn pending_machine_registrations_path() -> PathBuf {
    config::config_dir().join("pending-hosts.json")
}

fn load_pending_machine_registrations() -> Result<Vec<PendingMachineRegistration>, String> {
    let path = pending_machine_registrations_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("read pending hosts {}: {err}", path.display())),
    };
    serde_json::from_str(&text)
        .map_err(|err| format!("parse pending hosts {}: {err}", path.display()))
}

fn write_pending_machine_registrations(
    registrations: &[PendingMachineRegistration],
) -> Result<(), String> {
    let path = pending_machine_registrations_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create pending hosts directory {}: {err}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(registrations)
        .map_err(|err| format!("serialize pending hosts: {err}"))?;
    std::fs::write(&path, text)
        .map_err(|err| format!("write pending hosts {}: {err}", path.display()))
}

fn pending_machine_from_info(machine: &MachineInfo) -> PendingMachineRegistration {
    PendingMachineRegistration {
        workspace_id: machine.workspace_id.clone(),
        owner_actor_id: machine.owner_actor_id.clone(),
        id: machine.id.clone(),
        name: machine.name.clone(),
        kind: machine.kind.clone(),
        data_root: machine.data_root.clone(),
        config_dir: machine.config_dir.clone(),
    }
}

fn save_pending_machine_registration(machine: &MachineInfo) -> Result<(), String> {
    let mut registrations = load_pending_machine_registrations()?;
    let pending = pending_machine_from_info(machine);
    if let Some(existing) = registrations
        .iter_mut()
        .find(|registration| registration.id == pending.id)
    {
        *existing = pending;
    } else {
        registrations.push(pending);
    }
    write_pending_machine_registrations(&registrations)
}

fn pending_machine_infos(cfg: &DesktopConfig) -> Result<Vec<MachineInfo>, String> {
    let registrations = load_pending_machine_registrations()?;
    Ok(pending_machine_infos_from_registrations(
        cfg,
        &registrations,
    ))
}

fn pending_machine_infos_from_registrations(
    cfg: &DesktopConfig,
    registrations: &[PendingMachineRegistration],
) -> Vec<MachineInfo> {
    registrations
        .iter()
        .filter(|registration| pending_machine_visible(cfg, registration))
        .map(|registration| pending_machine_info_from_registration(cfg, registration))
        .collect()
}

fn pending_machine_visible(cfg: &DesktopConfig, registration: &PendingMachineRegistration) -> bool {
    if let Some(workspace_id) = registration.workspace_id.as_deref() {
        if config::active_workspace_id(cfg) != Some(workspace_id) {
            return false;
        }
    }
    if let Some(owner_actor_id) = registration.owner_actor_id.as_deref() {
        if config::active_owner_actor_id(cfg).as_deref() != Some(owner_actor_id) {
            return false;
        }
    }
    true
}

fn pending_machine_info_from_registration(
    cfg: &DesktopConfig,
    registration: &PendingMachineRegistration,
) -> MachineInfo {
    let data_root = PathBuf::from(&registration.data_root);
    let config_dir = PathBuf::from(&registration.config_dir);
    let (serve_command, setup_script) = daemon_start_commands(
        &data_root,
        Some(&config_dir),
        active_server_url(cfg),
        &registration.id,
        &registration.name,
    );
    MachineInfo {
        workspace_id: registration.workspace_id.clone(),
        owner_actor_id: registration.owner_actor_id.clone(),
        id: registration.id.clone(),
        name: registration.name.clone(),
        kind: if registration.kind.trim().is_empty() {
            "local".into()
        } else {
            registration.kind.clone()
        },
        source: "local_registration".into(),
        read_only: true,
        can_command: false,
        can_open_local_path: true,
        capabilities: vec!["machine.register".into()],
        inventory_revision: 0,
        inventory_observed_at: None,
        status: "pending".into(),
        setup_status: "pending".into(),
        connection_status: "notConnected".into(),
        connection_actor_id: String::new(),
        data_root: registration.data_root.clone(),
        config_dir: registration.config_dir.clone(),
        agent_count: 0,
        online_agent_count: 0,
        service_count: 0,
        providers: Vec::new(),
        agents: Vec::new(),
        services: Vec::new(),
        serve_command,
        setup_script,
    }
}

fn default_machine_data_root(machine_id: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(config::config_dir)
        .join("loom")
        .join("hosts")
        .join(machine_id)
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
        .filter(|actor_id| !actor_id.is_empty())
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
            allowed_agents.extend(meta.agent_specs.iter().map(|spec| spec.actor.id.clone()));
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

fn agent_profile_path(data_root: &Path, spec: &AgentSpec) -> PathBuf {
    data_root
        .join("agents")
        .join(&spec.actor.id)
        .join("profile")
}

fn normalize_local_path(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

#[allow(clippy::disallowed_methods)] // G1 OS shell (PM-Arbitration-003) — see body
fn open_path_with_system(path: &Path) -> anyhow::Result<()> {
    // FIXME(windows-compat-iter): GUI surface deferred per PRD §2.3
    // [G1: OS shell visibility] (PM-Arbitration-003 judgement).
    // These user-facing file-manager launchers must NOT use
    // `loom_platform::process::Command` until P1-Cmd-Sweep verifies that
    // the newtype's default Windows flags don't break Explorer's popup
    // behavior (ARCH-flagged risk for `explorer`). P0-Lint
    // (`clippy::disallowed_methods`) is active workspace-wide; the fn-level
    // allow above is the documented G1 exemption (covers all three platform
    // branches below).
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
    machine_name: &str,
) -> (String, String) {
    let data_root_arg = shell_path_arg(data_root);
    let config_dir_arg = config_dir.map(shell_path_arg);
    let daemon_bin = preferred_daemon_binary()
        .map(|path| shell_path_arg(&path))
        .unwrap_or_else(|| "loom-daemon".into());
    let serve_command = if let Some(config_dir_arg) = config_dir_arg.as_ref() {
        format!(
            "LOOM_CONFIG_DIR={} LOOM_AGENT_DATA_ROOT={} {} --server {} --machine-id {} --machine-name {}{}",
            config_dir_arg,
            data_root_arg,
            daemon_bin,
            shell_arg(server_url),
            shell_arg(machine_id),
            shell_arg(machine_name),
            if cfg!(windows) { " --no-ipc" } else { "" },
        )
    } else {
        format!(
            "LOOM_AGENT_DATA_ROOT={} {} --server {} --machine-id {} --machine-name {}{}",
            data_root_arg,
            daemon_bin,
            shell_arg(server_url),
            shell_arg(machine_id),
            shell_arg(machine_name),
            if cfg!(windows) { " --no-ipc" } else { "" },
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
        "#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p {}\n{}export LOOM_AGENT_DATA_ROOT={}\nif [[ -z \"${{LOOM_DAEMON_BIN:-}}\" ]]; then\n  LOOM_DAEMON_BIN={}\nfi\nif [[ ! -x \"$LOOM_DAEMON_BIN\" ]]; then\n  if command -v \"$LOOM_DAEMON_BIN\" >/dev/null 2>&1; then\n    LOOM_DAEMON_BIN=\"$(command -v \"$LOOM_DAEMON_BIN\")\"\n  else\n    LOOM_DAEMON_BIN=\"$(command -v loom-daemon)\"\n  fi\nfi\nexec \"$LOOM_DAEMON_BIN\" --server {} --machine-id {} --machine-name {}{}\n",
        mkdir_args,
        config_export,
        shell_path_arg(data_root),
        daemon_bin,
        shell_arg(server_url),
        shell_arg(machine_id),
        shell_arg(machine_name),
        if cfg!(windows) { " --no-ipc" } else { "" },
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
        let custom_url = "ws://custom.test/rpc";
        let (serve_command, setup_script) = daemon_start_commands(
            Path::new("/tmp/loom data"),
            Some(Path::new("/tmp/loom config")),
            custom_url,
            "machine_test",
            "CanfengMac",
        );

        assert!(serve_command.contains(&format!(
            "--server {custom_url} --machine-id machine_test --machine-name CanfengMac"
        )));
        assert!(serve_command.starts_with("LOOM_CONFIG_DIR="));
        assert!(serve_command.contains("LOOM_AGENT_DATA_ROOT="));
        assert!(!serve_command.contains(" daemon --machine-id "));
        assert!(setup_script.contains("export LOOM_CONFIG_DIR="));
        assert!(setup_script.contains("LOOM_DAEMON_BIN"));
        assert!(setup_script.contains(&format!(
            "exec \"$LOOM_DAEMON_BIN\" --server {custom_url} --machine-id machine_test --machine-name CanfengMac"
        )));
        assert!(!setup_script.contains(" daemon --machine-id "));
    }

    #[test]
    fn pending_machine_registration_is_visible_for_active_workspace() {
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
        };
        let registrations = vec![PendingMachineRegistration {
            workspace_id: Some("default".into()),
            owner_actor_id: Some(account.actor_id.clone()),
            id: "machine_pending".into(),
            name: "Pending Host".into(),
            kind: "local".into(),
            data_root: "/tmp/loom-pending-data".into(),
            config_dir: "/tmp/loom-pending-config".into(),
        }];

        let machines = pending_machine_infos_from_registrations(&cfg, &registrations);

        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].id, "machine_pending");
        assert_eq!(machines[0].source, "local_registration");
        assert_eq!(machines[0].setup_status, "pending");
        assert!(machines[0]
            .serve_command
            .contains("--machine-id machine_pending"));
    }

    #[test]
    fn server_inventory_replaces_pending_machine_registration() {
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
        };
        let registrations = vec![PendingMachineRegistration {
            workspace_id: Some("default".into()),
            owner_actor_id: Some(account.actor_id.clone()),
            id: "machine_pending".into(),
            name: "Pending Host".into(),
            kind: "local".into(),
            data_root: "/tmp/loom-pending-data".into(),
            config_dir: "/tmp/loom-pending-config".into(),
        }];
        let actor = json!({
            "id": "actor_service_machine_pending",
            "kind": "service",
            "displayName": "Pending Host",
            "_meta": {
                "role": "machine",
                "source": "daemon",
                "machineId": "machine_pending",
                "inventoryVersion": 2,
                "revision": 2,
                "observedAt": "2026-05-13T10:50:00Z",
                "workspaceId": "default",
                "ownerActorId": account.actor_id,
                "name": "Pending Host",
                "kind": "local",
                "dataRoot": "/tmp/loom-pending-data",
                "configDir": "/tmp/loom-pending-config",
                "capabilities": ["inventory.read", "connection.status", "machine.command"],
                "providers": [],
                "agentSpecs": []
            }
        });
        let server_machine = server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc")
            .expect("server machine");
        let mut machines = pending_machine_infos_from_registrations(&cfg, &registrations);

        upsert_server_machine_info(&mut machines, server_machine);

        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].id, "machine_pending");
        assert_eq!(machines[0].source, "server_inventory");
        assert_eq!(machines[0].inventory_revision, 2);
    }

    #[test]
    fn actor_list_filter_ignores_agents_without_daemon_inventory() {
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
                        "agentSpecs": [{
                            "actor": {
                                "id": "actor_remote_agent",
                                "kind": "agent",
                                "displayName": "Remote Agent"
                            },
                            "providerRef": {
                                "id": "codex",
                                "mode": "print",
                                "model": "gpt-5.5"
                            },
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
                "agentSpecs": [{
                    "actor": {
                        "id": "actor_remote_agent",
                        "kind": "agent",
                        "displayName": "Remote Agent"
                    },
                    "providerRef": {
                        "id": "claude",
                        "mode": "print",
                        "model": "claude-sonnet-4.6"
                    },
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
            format!(
                "/home/canfeng/.agentx/machine_remote{}agents{}actor_remote_agent{}profile",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            )
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
                "agentSpecs": []
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
                "agentSpecs": []
            }
        });

        let machine = server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc")
            .expect("other-owner daemon should still be discoverable");

        assert_eq!(machine.id, "machine_other");
        assert!(machine.read_only);
        assert!(!machine.can_command);
    }

    #[test]
    fn server_machine_inventory_replaces_older_snapshot_with_same_id() {
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
                "agentSpecs": [{
                    "actor": {
                        "id": "actor_remote_agent",
                        "kind": "agent",
                        "displayName": "Remote Agent"
                    },
                    "providerRef": {
                        "id": "claude",
                        "mode": "print",
                        "model": "claude-sonnet-4.6"
                    },
                    "autostart": true
                }]
            }
        });
        let server_machine = server_machine_info_from_actor(&actor, &cfg, "ws://example/rpc")
            .expect("server machine");
        let mut older_snapshot = server_machine.clone();
        older_snapshot.inventory_revision = 0;
        older_snapshot.agents.clear();
        older_snapshot.agent_count = 0;
        let mut machines = vec![older_snapshot];

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
    fn server_machine_inventory_same_revision_keeps_newer_observed_snapshot() {
        let mut current = MachineInfo {
            workspace_id: Some("default".into()),
            owner_actor_id: Some("actor_human_1".into()),
            id: "machine_remote".into(),
            name: "Remote Box".into(),
            kind: "local".into(),
            source: "server_inventory".into(),
            read_only: false,
            can_command: true,
            can_open_local_path: false,
            capabilities: vec!["machine.command".into()],
            inventory_revision: 7,
            inventory_observed_at: Some("2026-05-13T10:51:00Z".into()),
            status: "configured".into(),
            setup_status: "configured".into(),
            connection_status: "online".into(),
            connection_actor_id: "actor_service_machine_remote".into(),
            data_root: "/tmp/loom-data".into(),
            config_dir: "/tmp/loom-config".into(),
            agent_count: 1,
            online_agent_count: 0,
            service_count: 0,
            providers: Vec::new(),
            agents: Vec::new(),
            services: Vec::new(),
            serve_command: String::new(),
            setup_script: String::new(),
        };
        let mut stale = current.clone();
        stale.name = "Stale Box".into();
        stale.inventory_observed_at = Some("2026-05-13T10:50:00Z".into());
        let mut machines = vec![current.clone()];

        upsert_server_machine_info(&mut machines, stale);

        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].name, "Remote Box");

        current.name = "Fresh Box".into();
        current.inventory_observed_at = Some("2026-05-13T10:52:00Z".into());
        upsert_server_machine_info(&mut machines, current);

        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].name, "Fresh Box");
    }

    #[test]
    fn server_machine_inventory_ignores_legacy_machine_actor_meta() {
        let account = test_account();
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(account.clone()),
            workspaces: vec![],
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
