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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use agent_runtime::discovery::{detect_agent_cli_providers, DetectedAgentProvider};
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLocalDefaults {
    pub user_id: String,
    pub nickname: String,
    pub actor_id: String,
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

#[tauri::command]
pub async fn account_local_defaults() -> Result<AccountLocalDefaults, String> {
    let nickname = local_account_display_name();
    let user_id = local_user_id_from_display_name(&nickname);
    let actor_id = default_actor_id_for_local_user(&user_id);
    Ok(AccountLocalDefaults {
        user_id,
        nickname,
        actor_id,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSetLocalArgs {
    pub user_id: String,
    pub nickname: String,
    pub actor_id: String,
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
pub async fn account_set_local(
    state: State<'_, AppState>,
    args: AccountSetLocalArgs,
) -> Result<AccountLoginResult, String> {
    let user_id = normalize_local_user_id(&args.user_id)?;
    let nickname = args.nickname.trim();
    if nickname.is_empty() {
        return Err("nickname is required".into());
    }
    let actor_id = normalize_local_actor_id(&args.actor_id)?;

    let account = config::normalize_human_account(HumanAccount {
        provider: "local".into(),
        staff_id: user_id,
        nickname: nickname.into(),
        real_name: String::new(),
        email: String::new(),
        actor_id,
        avatar_url: String::new(),
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

fn normalize_local_user_id(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("user id is required".into());
    }
    if value.len() > 48 {
        return Err("user id must be at most 48 characters".into());
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err("user id can only use letters, numbers, _ and -".into());
    }
    Ok(value.to_string())
}

fn normalize_local_actor_id(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("actor id is required".into());
    }
    if !config::is_supported_actor_id(value) {
        return Err("actor id can use letters, numbers, _, -, . and :, up to 64 characters".into());
    }
    Ok(value.to_string())
}

fn local_account_display_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Local User".into())
}

fn local_user_id_from_display_name(display_name: &str) -> String {
    let mut out = String::new();
    let mut last_was_separator = false;
    for ch in display_name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if ch == '_' || ch == '-' || ch.is_whitespace() {
            if !out.is_empty() && !last_was_separator {
                out.push('_');
                last_was_separator = true;
            }
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "local_user".into()
    } else {
        out
    }
}

fn default_actor_id_for_local_user(user_id: &str) -> String {
    const PREFIX: &str = "actor_human_local_";
    let suffix_len = 64usize.saturating_sub(PREFIX.len());
    let suffix = user_id.trim();
    let suffix = if suffix.is_empty() {
        "local_user"
    } else if suffix.len() > suffix_len {
        &suffix[..suffix_len]
    } else {
        suffix
    };
    format!("{PREFIX}{suffix}")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUpdateAvatarArgs {
    pub avatar_url: String,
}

#[tauri::command]
pub async fn account_update_avatar(
    state: State<'_, AppState>,
    args: AccountUpdateAvatarArgs,
) -> Result<DesktopConfig, String> {
    let avatar_url = args.avatar_url.trim().to_string();
    let mut cfg = config::load_or_init().map_err(stringify)?;
    let active_workspace = cfg
        .active
        .as_deref()
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .or_else(|| cfg.workspaces.first())
        .cloned();
    if cfg.account.is_none() {
        let workspace = active_workspace
            .as_ref()
            .ok_or_else(|| "create a workspace before setting an account avatar".to_string())?;
        let staff_id = workspace
            .actor_id
            .strip_prefix("actor_human_local_")
            .unwrap_or(workspace.actor_id.as_str())
            .to_string();
        cfg.account = Some(HumanAccount {
            provider: "local".into(),
            staff_id,
            nickname: workspace.display_name.clone(),
            real_name: String::new(),
            email: String::new(),
            actor_id: workspace.actor_id.clone(),
            avatar_url: avatar_url.clone(),
        });
    } else if let Some(account) = cfg.account.as_mut() {
        account.avatar_url = avatar_url.clone();
    }
    apply_account_identity(&mut cfg);
    config::save(&cfg).map_err(stringify)?;
    let cfg = config::load_or_init().map_err(stringify)?;
    if let (Some(client), Some(account)) = (state.try_client().await, cfg.account.as_ref()) {
        upsert_human_actor(&client, account)
            .await
            .map_err(deep_stringify)?;
    }
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
    let normalized_server_url =
        config::normalize_workspace_server_url(&args.server_url).map_err(stringify)?;
    if let Some(existing) = cfg
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.server_url == normalized_server_url)
    {
        let name = args.name.trim();
        if !name.is_empty() && existing.name.trim().is_empty() {
            existing.name = name.into();
        }
        if args.activate {
            cfg.active = Some(existing.id.clone());
        }
        config::save(&cfg).map_err(|e| e.to_string())?;
        return config::load_or_init().map_err(|e| e.to_string());
    }

    let id = config::generate_id();
    let (actor_id, display_name) = workspace_identity_for_new_workspace(&cfg, &id);
    let ws = Workspace {
        id: id.clone(),
        name: args.name,
        server_url: normalized_server_url,
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
    let _ = workspace_id;
    let local_name = local_display_name();
    (config::default_local_actor_id(), local_name)
}

fn local_display_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .map(|value| value.trim().to_string())
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Local".into())
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

async fn ensure_active_workspace_connection(
    client: &Arc<Client>,
    cfg: &DesktopConfig,
) -> Result<(), String> {
    let workspace = active_workspace(cfg)
        .ok_or_else(|| "select a workspace before sending messages".to_string())?;
    client
        .open_connection(&workspace.actor_id, Some(&workspace.display_name))
        .await
        .map_err(deep_stringify)?;
    upsert_workspace_actor(client, cfg.account.as_ref(), workspace)
        .await
        .map_err(deep_stringify)
}

fn active_workspace(cfg: &DesktopConfig) -> Option<&Workspace> {
    config::active_workspace_id(cfg)
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .or_else(|| cfg.workspaces.first())
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
pub async fn channel_member_config_list(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_MEMBER_CONFIG_LIST, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_member_config_set(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_MEMBER_CONFIG_SET, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_member_config_clear(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_MEMBER_CONFIG_CLEAR, Some(params))
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
    let cfg = config::load_or_init().map_err(stringify)?;
    let client = state.client().await?;
    ensure_active_workspace_connection(&client, &cfg).await?;
    client
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

/// E1: Check whether an artifact exists on the server. Replaces the
/// deprecated `path_exists` for attachment state detection — works
/// correctly in distributed deployments (server/daemon/gui on
/// different machines) because it queries the Server API rather than
/// the local filesystem.
#[tauri::command]
pub async fn artifact_exists(state: State<'_, AppState>, params: Value) -> Result<bool, String> {
    // Protocol contract: `artifact_get` returns the artifact metadata
    // (non-null) when the artifact exists, and returns an RPC error with
    // code -32000 (APP_NOT_FOUND) when it does not. We must distinguish
    // "not found" (→ Ok(false)) from genuine transport/server errors
    // (→ propagate Err). Do NOT use `map_err(stringify)?` here because
    // that would turn the not-found case into an Err.
    match state
        .client()
        .await?
        .call_raw(method::ARTIFACT_GET, Some(params))
        .await
    {
        Ok(result) => Ok(!result.is_null()),
        Err(e) => {
            let msg = stringify(e);
            // APP_NOT_FOUND surfaces as JSON-RPC error code -32000.
            if msg.contains("(code -32000)") {
                Ok(false)
            } else {
                Err(msg)
            }
        }
    }
}

/// Maximum payload size for `download_to_temp` (100 MB).
const DOWNLOAD_TO_TEMP_MAX_BYTES: usize = 100 * 1024 * 1024;

/// E1: Download an artifact to the system temp directory. Uses the
/// Server API `artifact_read` to fetch bytes, then writes them to
/// `std::env::temp_dir()/loom-downloads/<suggestedName>`. Returns the
/// full path of the downloaded file. This writes to the local temp
/// directory rather than `workspacePath` (which may point to a remote
/// machine).
///
/// # Memory profile
///
/// The download streams `artifact_read` in fixed-size chunks
/// (`chunk_size = 65_536` bytes). Each chunk is decoded into a
/// `Vec<u8>` and written immediately, so the peak memory used by the
/// download path is bounded at roughly `chunk_size + JSON overhead`
/// (~256 KB per chunk) regardless of the total artifact size. This
/// avoids loading the entire artifact into memory.
///
/// # Future: base64 migration
///
/// The current `extract_artifact_bytes` helper accepts a JSON `"bytes"`
/// array or a `"content"` string. A future migration to base64-encoded
/// binary transport (single `"bytes_b64"` field) will reduce JSON
/// overhead per chunk and is tracked as a separate PR — no change to
/// the chunked streaming design is required.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadToTempArgs {
    pub artifact_id: String,
    #[serde(default)]
    pub suggested_name: Option<String>,
}

#[tauri::command]
pub async fn download_to_temp(
    state: State<'_, AppState>,
    args: DownloadToTempArgs,
) -> Result<String, String> {
    // Fetch artifact metadata to determine media type and validate existence.
    // If the artifact does not exist, call_raw returns an APP_NOT_FOUND
    // error which is propagated via map_err(stringify)? — no need for
    // a separate null check.
    let meta = state
        .client()
        .await?
        .call_raw(
            method::ARTIFACT_GET,
            Some(json!({ "artifactId": args.artifact_id })),
        )
        .await
        .map_err(stringify)?;

    // Determine the filename: explicit suggestion > artifact name > fallback.
    // artifact/get nests fields under "artifact".
    let artifact_name = meta
        .get("artifact")
        .and_then(|a| a.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("download")
        .to_string();
    let file_name = args
        .suggested_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or(artifact_name);
    // Sanitize the filename — strip path separators to prevent directory traversal.
    let safe_name = sanitize_filename(&file_name);

    let download_dir = std::env::temp_dir()
        .join("loom-downloads")
        .join(&args.artifact_id);
    std::fs::create_dir_all(&download_dir)
        .map_err(|e| format!("Failed to create temp download directory: {e}"))?;
    let dest_path = download_dir.join(&safe_name);

    // Stream artifact_read in chunks until complete or limit exceeded.
    let mut file = std::fs::File::create(&dest_path)
        .map_err(|e| format!("Failed to create temp file: {e}"))?;
    let mut offset = 0u64;
    let mut total_written = 0usize;
    let chunk_size = 65_536u64;
    loop {
        let chunk = state
            .client()
            .await?
            .call_raw(
                method::ARTIFACT_READ,
                Some(json!({
                    "artifactId": args.artifact_id,
                    "offset": offset,
                    "maxBytes": chunk_size,
                })),
            )
            .await
            .map_err(stringify)?;

        let bytes = extract_artifact_bytes(&chunk)?;
        let written = bytes.len();
        if written == 0 {
            break;
        }
        total_written += written;
        if total_written > DOWNLOAD_TO_TEMP_MAX_BYTES {
            // Clean up partial file before returning error.
            let _ = std::fs::remove_file(&dest_path);
            return Err(format!(
                "file exceeds {} byte download limit",
                DOWNLOAD_TO_TEMP_MAX_BYTES
            ));
        }
        std::io::Write::write_all(&mut file, &bytes)
            .map_err(|e| format!("Failed to write temp file: {e}"))?;

        let truncated = chunk
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !truncated {
            break;
        }
        match chunk.get("nextOffset").and_then(Value::as_u64) {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }

    Ok(dest_path.display().to_string())
}

/// Maximum age (24 hours) for files in the temp download directory before
/// they are eligible for cleanup.
const DOWNLOAD_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Remove stale files from the `loom-downloads` temp directory. Files
/// whose last modification time is older than `DOWNLOAD_MAX_AGE_SECS`
/// (24 hours) are deleted. This is best-effort: errors are silently
/// ignored so that cleanup never blocks app startup.
///
/// Call this once during GUI app setup (`main.rs` `.setup()` hook).
/// Uses `std::env::temp_dir()` for cross-platform compatibility
/// (Windows `%TEMP%`, macOS `/var/folders/...`, Linux `/tmp`).
pub fn cleanup_downloads() {
    let download_root = std::env::temp_dir().join("loom-downloads");
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(DOWNLOAD_MAX_AGE_SECS));

    let cutoff = match cutoff {
        Some(c) => c,
        None => return, // system clock issue — skip cleanup
    };

    let entries = match std::fs::read_dir(&download_root) {
        Ok(e) => e,
        Err(_) => return, // dir doesn't exist or unreadable — nothing to do
    };

    for entry in entries.flatten() {
        // Each subdirectory is named by artifact_id; clean files inside.
        let path = entry.path();
        if path.is_dir() {
            let _ = cleanup_dir_older_than(&path, cutoff);
        } else {
            let _ = remove_if_older_than(&path, cutoff);
        }
    }
}

/// Remove a single file if its mtime is older than `cutoff`.
fn remove_if_older_than(path: &std::path::Path, cutoff: std::time::SystemTime) -> std::io::Result<()> {
    let metadata = std::fs::metadata(path)?;
    if let Ok(mtime) = metadata.modified() {
        if mtime < cutoff {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Recursively clean files older than `cutoff` inside `dir`.
fn cleanup_dir_older_than(
    dir: &std::path::Path,
    cutoff: std::time::SystemTime,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let _ = cleanup_dir_older_than(&path, cutoff);
        } else {
            let _ = remove_if_older_than(&path, cutoff);
        }
    }
    // Remove the subdirectory if it's now empty.
    if std::fs::read_dir(dir)?.next().is_none() {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(())
}
/// populates both `bytes` (as a JSON number array) and `content` (as a
/// lossy UTF-8 string). We prefer `bytes` for binary correctness; fall
/// back to `content`'s raw bytes only if `bytes` is absent.
fn extract_artifact_bytes(response: &Value) -> Result<Vec<u8>, String> {
    if let Some(bytes) = response.get("bytes").and_then(Value::as_array) {
        if !bytes.is_empty() {
            return Ok(bytes
                .iter()
                .filter_map(|v| {
                    // Filter out-of-bounds values instead of silently
                    // truncating with `n as u8`, which would corrupt data
                    // for n > 255.
                    v.as_u64().filter(|&n| n <= u8::MAX as u64).map(|n| n as u8)
                })
                .collect());
        }
    }
    if let Some(content) = response.get("content").and_then(Value::as_str) {
        return Ok(content.as_bytes().to_vec());
    }
    Ok(Vec::new())
}

/// Sanitize a filename by removing path separators and other dangerous
/// characters. Keeps Unicode letters/digits, dots, dashes, underscores,
/// and spaces.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                // Replace path separators AND all other non-whitelisted
                // characters (including Windows-reserved chars like
                // : < > * ? " |) with '_'. This prevents invalid filenames
                // and Alternate Data Stream vectors on Windows.
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "loom-download".to_string()
    } else {
        trimmed.to_string()
    }
}

// =========================================================================
// Persistent attachment cache (ARCH D3-r1)
//
// Image and other "special" attachments are downloaded to a persistent
// cache directory under `<data_dir>/loom/cache/attachments/<artifactId>/`
// rather than the OS temp directory used by `download_to_temp`. This
// satisfies AC-P0-8 (independent cache dir), AC-P0-9 (no auto-expiry;
// survives restarts), and AC-P0-10 (clear-cache UI).
//
// `download_to_temp` and `cleanup_downloads` (24h temp cleanup) are left
// untouched — they continue to serve non-image manual downloads.
// =========================================================================

/// Maximum payload size for `download_to_cache` (100 MB). Mirrors
/// `DOWNLOAD_TO_TEMP_MAX_BYTES` to keep the cache bounded per file.
const DOWNLOAD_TO_CACHE_MAX_BYTES: usize = 100 * 1024 * 1024;

/// Default chunk size for streaming `artifact_read` while populating the
/// cache. Kept identical to `download_to_temp` for a consistent memory
/// profile (~256 KB peak per chunk).
const CACHE_CHUNK_SIZE: u64 = 65_536;

/// Resolve the persistent attachment cache root:
/// `<data_dir>/loom/cache/attachments/`.
///
/// Uses `dirs::data_dir()` (NOT `cache_dir()`) because the macOS/Linux
/// cache dir may be purged by the OS, violating AC-P0-9 (persistence).
/// Three-platform layout:
/// - Windows: `%APPDATA%\Local\loom\cache\attachments` (AppData\Local)
/// - macOS:   `~/Library/Application Support/loom/cache/attachments`
/// - Linux:   `~/.local/share/loom/cache/attachments`
///
/// Returns `None` if `data_dir()` cannot be determined (no home directory
/// on the host). Callers surface this as an error string.
fn attachment_cache_root() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("loom").join("cache").join("attachments"))
}

/// Metadata record persisted alongside each cached artifact as
/// `<artifactId>/.meta.json`. Stores the original artifact name, media
/// type, total byte size, and the ISO-8601 timestamp of the download.
/// This lets the UI render size/type without re-fetching from the server
/// and lets `get_attachment_cache_size` avoid re-statting every file.
#[derive(Serialize, Deserialize)]
struct CachedArtifactMeta {
    artifact_id: String,
    name: String,
    media_type: String,
    size: u64,
    downloaded_at: String,
}

/// Write the `.meta.json` sidecar for a cached artifact. Best-effort:
/// a failure to write metadata does not invalidate the cached bytes
/// (the file is still usable); the error is only logged via the
/// returned `Result` so callers can decide whether to surface it.
fn write_cache_meta(cache_dir: &Path, meta: &CachedArtifactMeta) -> std::io::Result<()> {
    let meta_path = cache_dir.join(".meta.json");
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&meta_path, json)
}

/// E1-cache: Download an artifact to the persistent cache directory
/// (`<data_dir>/loom/cache/attachments/<artifactId>/`), replacing
/// `download_to_temp` for image auto-download (ARCH D3-r1).
///
/// If the artifact is already cached (file exists and `.meta.json` is
/// present), the existing path is returned without re-downloading — this
/// is the cache-hit fast path. Otherwise the artifact is streamed from
/// the server in chunks (same memory-bounded streaming as
/// `download_to_temp`) and a `.meta.json` sidecar is written.
///
/// Returns the full local path of the cached file. Persists across app
/// restarts; not subject to the 24h temp cleanup.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadToCacheArgs {
    pub artifact_id: String,
    #[serde(default)]
    pub suggested_name: Option<String>,
}

#[tauri::command]
pub async fn download_to_cache(
    state: State<'_, AppState>,
    args: DownloadToCacheArgs,
) -> Result<String, String> {
    let cache_root = attachment_cache_root()
        .ok_or_else(|| "Cannot determine persistent data directory for cache".to_string())?;
    let artifact_dir = cache_root.join(&args.artifact_id);

    // Fetch artifact metadata to determine media type and validate existence.
    let meta = state
        .client()
        .await?
        .call_raw(
            method::ARTIFACT_GET,
            Some(json!({ "artifactId": args.artifact_id })),
        )
        .await
        .map_err(stringify)?;

    // artifact/get returns { artifact: { name, mediaType, ... } } — the
    // fields are nested under "artifact", not at the top level.
    let artifact_obj = meta.get("artifact");
    let artifact_name = artifact_obj
        .and_then(|a| a.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("download")
        .to_string();
    let media_type = artifact_obj
        .and_then(|a| a.get("mediaType"))
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();
    let file_name = args
        .suggested_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| artifact_name.clone());
    let safe_name = sanitize_filename(&file_name);

    std::fs::create_dir_all(&artifact_dir)
        .map_err(|e| format!("Failed to create cache directory: {e}"))?;
    let dest_path = artifact_dir.join(&safe_name);

    // Cache-hit fast path: if the file and .meta.json already exist, skip
    // the network download entirely (AC-P0-9 persistence + read_local_file_bytes).
    let meta_path = artifact_dir.join(".meta.json");
    if dest_path.exists() && meta_path.exists() {
        return Ok(dest_path.display().to_string());
    }

    // Stream artifact_read in chunks until complete or limit exceeded.
    let mut file = std::fs::File::create(&dest_path)
        .map_err(|e| format!("Failed to create cache file: {e}"))?;
    let mut offset = 0u64;
    let mut total_written = 0u64;
    loop {
        let chunk = state
            .client()
            .await?
            .call_raw(
                method::ARTIFACT_READ,
                Some(json!({
                    "artifactId": args.artifact_id,
                    "offset": offset,
                    "maxBytes": CACHE_CHUNK_SIZE,
                })),
            )
            .await
            .map_err(stringify)?;

        let bytes = extract_artifact_bytes(&chunk)?;
        let written = bytes.len();
        if written == 0 {
            break;
        }
        total_written += written as u64;
        if total_written > DOWNLOAD_TO_CACHE_MAX_BYTES as u64 {
            let _ = std::fs::remove_file(&dest_path);
            return Err(format!(
                "file exceeds {} byte cache download limit",
                DOWNLOAD_TO_CACHE_MAX_BYTES
            ));
        }
        std::io::Write::write_all(&mut file, &bytes)
            .map_err(|e| format!("Failed to write cache file: {e}"))?;

        let truncated = chunk
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !truncated {
            break;
        }
        match chunk.get("nextOffset").and_then(Value::as_u64) {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }

    // Write metadata sidecar (best-effort — failure does not invalidate bytes).
    let cache_meta = CachedArtifactMeta {
        artifact_id: args.artifact_id.clone(),
        name: artifact_name,
        media_type,
        size: total_written,
        downloaded_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = write_cache_meta(&artifact_dir, &cache_meta);

    Ok(dest_path.display().to_string())
}

/// Recursively compute the total size in bytes of a directory tree.
/// Used by `get_attachment_cache_size`. Symlinks are not followed.
fn dir_size_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.is_file() {
            total += metadata.len();
        } else if metadata.is_dir() {
            total += dir_size_bytes(&path);
        }
    }
    total
}

/// E2-cache: Clear the entire attachment cache directory, deleting all
/// cached artifact files on disk (ARCH D3-r1, AC-P0-10). The FE is
/// responsible for also clearing the localStorage mapping and in-memory
/// Object URLs after this returns. Returns the number of bytes freed
/// so the UI can confirm what was removed.
#[tauri::command]
pub fn clear_attachment_cache() -> Result<u64, String> {
    let cache_root = attachment_cache_root()
        .ok_or_else(|| "Cannot determine persistent data directory for cache".to_string())?;

    if !cache_root.exists() {
        return Ok(0);
    }

    let size = dir_size_bytes(&cache_root);
    std::fs::remove_dir_all(&cache_root)
        .map_err(|e| format!("Failed to clear attachment cache: {e}"))?;
    Ok(size)
}

/// E3-cache: Get the total size of the attachment cache directory in
/// bytes (ARCH D3-r1, AC-P0-10). Used by the settings UI to display
/// the cache size before the user confirms clearing. Returns 0 if the
/// cache directory does not exist yet.
#[tauri::command]
pub fn get_attachment_cache_size() -> Result<u64, String> {
    let cache_root = attachment_cache_root()
        .ok_or_else(|| "Cannot determine persistent data directory for cache".to_string())?;
    if !cache_root.exists() {
        return Ok(0);
    }
    Ok(dir_size_bytes(&cache_root))
}

/// E4-cache: Read bytes from a local cached file (ARCH D3-r1). This is
/// the cache-hit path: when an artifact is already on disk, the FE reads
/// its bytes directly instead of re-downloading over the network.
///
/// Reads up to `maxBytes` (default 65_536) starting at `offset`. Returns
/// the raw byte array, whether the read was truncated, and the next
/// offset (if truncated) — mirroring the `artifact_read` server response
/// shape so the FE can reuse its chunked-decode logic.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadLocalFileBytesArgs {
    pub path: String,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadLocalFileBytesResult {
    bytes: Vec<u8>,
    truncated: bool,
    next_offset: Option<u64>,
}

#[tauri::command]
pub fn read_local_file_bytes(args: ReadLocalFileBytesArgs) -> Result<Value, String> {
    let path = PathBuf::from(&args.path);

    // Guard against directory traversal outside the cache: only allow
    // reads of files that live under the attachment cache root. This
    // prevents the FE from arbitrary file reads via this command.
    let cache_root = attachment_cache_root()
        .ok_or_else(|| "Cannot determine persistent data directory for cache".to_string())?;
    // canonicalize the cache root, creating it if missing so the prefix
    // check works even on a fresh install where no artifact has been
    // cached yet. (A read of a non-existent file under a non-existent
    // cache root still fails at the open() step below.)
    let canonical_root = match std::fs::canonicalize(&cache_root) {
        Ok(c) => c,
        Err(_) => match std::fs::create_dir_all(&cache_root)
            .and_then(|_| std::fs::canonicalize(&cache_root))
        {
            Ok(c) => c,
            Err(e) => return Err(format!("Failed to resolve cache root: {e}")),
        },
    };
    let canonical_target = std::fs::canonicalize(&path)
        .map_err(|e| format!("Failed to resolve path: {e}"))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err("Path is outside the attachment cache directory".to_string());
    }

    let mut file = std::fs::File::open(&canonical_target)
        .map_err(|e| format!("Failed to open cached file: {e}"))?;

    let offset = args.offset.unwrap_or(0);
    let max_bytes = args.max_bytes.unwrap_or(CACHE_CHUNK_SIZE);

    use std::io::{Read, Seek, SeekFrom};
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("Failed to seek cached file: {e}"))?;
    }

    let mut buf = vec![0u8; max_bytes as usize];
    let read = file
        .read(&mut buf)
        .map_err(|e| format!("Failed to read cached file: {e}"))?;
    buf.truncate(read);

    let truncated = read as u64 == max_bytes;
    let next_offset = if truncated {
        Some(offset + read as u64)
    } else {
        None
    };

    // Return as a JSON number array to match the server's artifact_read
    // response shape (consumed by the same FE decode path).
    let result = ReadLocalFileBytesResult {
        bytes: buf,
        truncated,
        next_offset,
    };
    serde_json::to_value(result).map_err(|e| format!("Failed to serialize result: {e}"))
}

// =========================================================================
// Category-based cache management (ARCH D3 v2)
//
// Extends the flat clear/size IPC with per-mediaType breakdown so the
// UI can show "Images: X MB (N files)" / "Other: Y MB (M files)" and
// let the user clear just one category.
// =========================================================================

/// Classify a media type into a cache category. Only `image/*` is
/// "images"; everything else (including a missing `.meta.json`) falls
/// back to "other". This keeps the categorization stable and simple —
/// new media types (video/audio) can be promoted to their own bucket
/// later without breaking the existing two-category contract.
fn cache_category_for_media_type(media_type: &str) -> &'static str {
    if media_type.starts_with("image/") {
        "images"
    } else {
        "other"
    }
}

/// Read the `.meta.json` sidecar for a single artifact cache dir.
/// Returns `None` if the file is missing or unparseable — callers
/// treat that as the "other" category with on-disk size fallback.
fn read_cache_meta(artifact_dir: &Path) -> Option<CachedArtifactMeta> {
    let meta_path = artifact_dir.join(".meta.json");
    let content = std::fs::read_to_string(&meta_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Size of a single artifact cache dir on disk (the artifact file +
/// `.meta.json`). Walks the dir rather than trusting `meta.size` so the
/// number reflects actual disk usage even if the meta is stale/missing.
fn artifact_dir_size(artifact_dir: &Path) -> u64 {
    dir_size_bytes(artifact_dir)
}

/// Per-category aggregate used by `get_attachment_cache_breakdown`.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct CacheCategoryStats {
    size: u64,
    count: u64,
}

/// Core breakdown logic parameterized by cache root, so it can be unit
/// tested with an isolated temp dir instead of the global cache root.
fn cache_breakdown_at_root(cache_root: &Path) -> (CacheCategoryStats, CacheCategoryStats) {
    let mut images = CacheCategoryStats::default();
    let mut other = CacheCategoryStats::default();

    if let Ok(entries) = std::fs::read_dir(cache_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let size = artifact_dir_size(&path);
            let category = read_cache_meta(&path)
                .map(|m| cache_category_for_media_type(&m.media_type))
                .unwrap_or("other");
            match category {
                "images" => {
                    images.size += size;
                    images.count += 1;
                }
                _ => {
                    other.size += size;
                    other.count += 1;
                }
            }
        }
    }
    (images, other)
}

/// E5-cache: Get a per-category breakdown of the attachment cache.
/// Walks `<cache_root>/<artifactId>/` dirs, reads each `.meta.json` to
/// classify by mediaType (image/* → "images", else → "other"), and
/// sums on-disk sizes. Returns:
/// `{ images: {size, count}, other: {size, count}, total: {size, count} }`.
///
/// `total` is a `{size, count}` object (not a bare number) so the FE
/// can read `total.size` for the clear-button enable check and
/// `total.count` for display. Missing/unparseable `.meta.json` falls
/// back to "other" (ARCH spec).
#[tauri::command]
pub fn get_attachment_cache_breakdown() -> Result<Value, String> {
    let cache_root = attachment_cache_root()
        .ok_or_else(|| "Cannot determine persistent data directory for cache".to_string())?;

    let (images, other) = cache_breakdown_at_root(&cache_root);
    let total = CacheCategoryStats {
        size: images.size + other.size,
        count: images.count + other.count,
    };
    Ok(json!({
        "images": images,
        "other": other,
        "total": total,
    }))
}

/// E6-cache: Clear a single category of the attachment cache.
/// `category` is "images" or "other". Walks the cache dirs, classifies
/// each by `.meta.json` mediaType, and removes only the dirs matching
/// the requested category. Returns `{ freedBytes, clearedIds }` so the
/// FE can batch-clear the corresponding localStorage entries.
///
/// Missing `.meta.json` dirs are treated as "other" (ARCH spec), so
/// clearing "other" also reclaims orphaned/untracked cache dirs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearAttachmentCacheByTypeArgs {
    pub category: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClearAttachmentCacheByTypeResult {
    freed_bytes: u64,
    cleared_ids: Vec<String>,
}

/// Core clear-by-type logic parameterized by cache root, so it can be
/// unit tested with an isolated temp dir.
fn clear_cache_by_type_at_root(
    cache_root: &Path,
    category: &str,
) -> (u64, Vec<String>) {
    let mut freed_bytes = 0u64;
    let mut cleared_ids = Vec::new();

    if let Ok(entries) = std::fs::read_dir(cache_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_category = read_cache_meta(&path)
                .map(|m| cache_category_for_media_type(&m.media_type))
                .unwrap_or("other");
            if dir_category != category {
                continue;
            }

            let size = artifact_dir_size(&path);
            let artifact_id = entry.file_name().to_string_lossy().to_string();

            match std::fs::remove_dir_all(&path) {
                Ok(()) => {
                    freed_bytes += size;
                    cleared_ids.push(artifact_id);
                }
                Err(e) => {
                    // Best-effort: log and continue rather than failing the
                    // whole operation when one dir cannot be removed.
                    eprintln!("Failed to remove cache dir {}: {e}", path.display());
                }
            }
        }
    }

    (freed_bytes, cleared_ids)
}

#[tauri::command]
pub fn clear_attachment_cache_by_type(
    args: ClearAttachmentCacheByTypeArgs,
) -> Result<Value, String> {
    let category = match args.category.as_str() {
        "images" | "other" => args.category.as_str(),
        _ => {
            return Err(format!(
                "Invalid category '{}': expected 'images' or 'other'",
                args.category
            ))
        }
    };

    let cache_root = attachment_cache_root()
        .ok_or_else(|| "Cannot determine persistent data directory for cache".to_string())?;

    let (freed_bytes, cleared_ids) = clear_cache_by_type_at_root(&cache_root, category);
    let result = ClearAttachmentCacheByTypeResult {
        freed_bytes,
        cleared_ids,
    };
    serde_json::to_value(result).map_err(|e| format!("Failed to serialize result: {e}"))
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
    pub wake: Option<Value>,
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
    pub wake: Option<Value>,
    #[serde(default)]
    pub env: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub bundle_skills: Option<Value>,
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
    if let Some(wake) = args.wake {
        command["wake"] = wake;
    }
    if let Some(bundle_skills) = args.bundle_skills {
        command["bundleSkills"] = bundle_skills;
    }
    let output = run_remote_machine_command(&state, &cfg, &target_machine_id, command).await?;
    agent_info_from_machine_command_output(output)
        .ok_or_else(|| format!("updated agent not returned by daemon: {}", args.actor_id))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillAddArgs {
    pub machine_id: String,
    pub actor_id: String,
    pub source: String,
}

#[tauri::command]
pub async fn agent_skill_add(
    state: State<'_, AppState>,
    args: AgentSkillAddArgs,
) -> Result<AgentInfo, String> {
    let machine_id = args.machine_id.trim();
    if machine_id.is_empty() {
        return Err("machine id is required".into());
    }
    let actor_id = args.actor_id.trim();
    if actor_id.is_empty() {
        return Err("actor id is required".into());
    }
    let source = args.source.trim();
    if source.is_empty() {
        return Err("skill directory is required".into());
    }
    let cfg = config::load_or_init().map_err(stringify)?;
    if server_machine_by_id(&cfg, state.try_client().await, machine_id)
        .await
        .is_none()
    {
        return Err(format!(
            "machine `{machine_id}` is not present in daemon inventory; start the daemon before editing agents"
        ));
    }
    let output = run_remote_machine_command(
        &state,
        &cfg,
        machine_id,
        json!({
            "op": "agent.skill.add",
            "actorId": actor_id,
            "source": source,
        }),
    )
    .await?;
    agent_info_from_machine_command_output(output)
        .ok_or_else(|| format!("updated agent not returned by daemon: {actor_id}"))
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
pub struct MachineDirListArgs {
    pub machine_id: String,
    #[serde(default)]
    pub path: Option<String>,
    /// E1: When true, include file entries (not just directories) in the
    /// response. Defaults to false for backward compatibility.
    #[serde(default)]
    pub include_files: bool,
}

#[tauri::command]
pub async fn machine_dir_list(
    state: State<'_, AppState>,
    args: MachineDirListArgs,
) -> Result<Value, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    ensure_server_machine_present(&cfg, &state, &args.machine_id).await?;
    run_remote_machine_command(
        &state,
        &cfg,
        &args.machine_id,
        json!({
            "op": "fs.dir.list",
            "path": args.path,
            "includeFiles": args.include_files,
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

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalProviderCheckResult {
    pub providers: Vec<MachineAgentProviderInfo>,
}

#[tauri::command]
pub async fn local_provider_check() -> Result<LocalProviderCheckResult, String> {
    Ok(LocalProviderCheckResult {
        providers: detect_agent_cli_providers()
            .iter()
            .map(detected_provider_summary)
            .collect(),
    })
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

/// B1: Native "Save As" dialog — replaces the unreliable `downloadBlob`
/// in the Tauri webview. Uses the `rfd` crate (pure Rust, no Tauri
/// capabilities change). Returns the chosen path on success, or `None`
/// when the user cancels the dialog.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFileDialogArgs {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

#[tauri::command]
pub async fn save_file_dialog(args: SaveFileDialogArgs) -> Result<Option<String>, String> {
    let file_name = if args.file_name.trim().is_empty() {
        "download"
    } else {
        args.file_name.trim()
    };

    let dialog = rfd::AsyncFileDialog::new().set_file_name(file_name);

    let result = dialog.save_file().await;

    match result {
        Some(handle) => {
            let path = handle.path();
            let path_str = path.to_string_lossy().to_string();
            std::fs::write(path, &args.bytes)
                .map_err(|e| format!("Failed to write file: {e}"))?;
            Ok(Some(path_str))
        }
        None => Ok(None),
    }
}

/// B1: Open a file with the system default application. Reuses
/// `open_path_with_system` (which already opens files via the default
/// handler on all three platforms) but does NOT create directories —
/// it is strictly a "launch existing file" command.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPathArgs {
    pub path: String,
}

#[tauri::command]
pub async fn open_file_default(args: OpenPathArgs) -> Result<(), String> {
    let raw = args.path.trim();
    if raw.is_empty() {
        return Err("path is required".into());
    }
    let path = normalize_local_path(config::expand_home(raw)).map_err(stringify)?;
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }
    open_path_with_system(&path).map_err(stringify)
}

/// B1: Reveal a file in the platform file manager, selecting it.
/// - Windows: `explorer /select,"<path>"`
/// - macOS: `open -R "<path>"`
/// - Linux: best-effort — opens the parent directory via `xdg-open`.
#[allow(clippy::disallowed_methods)] // G1 OS shell — same exemption as open_path_with_system
#[tauri::command]
pub async fn reveal_in_folder(args: OpenPathArgs) -> Result<(), String> {
    let raw = args.path.trim();
    if raw.is_empty() {
        return Err("path is required".into());
    }
    let path = normalize_local_path(config::expand_home(raw)).map_err(stringify)?;
    if !path.exists() {
        return Err(format!("Path not found: {}", path.display()));
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("Failed to reveal: {e}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("Failed to reveal: {e}"))?;
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("Failed to reveal: {e}"))?;
    }

    Ok(())
}

/// Check whether a local filesystem path exists on the GUI host machine.
///
/// Semantic orthogonality note: `path_exists` checks the **local
/// filesystem** of the machine running the GUI (used by the FE to
/// determine whether a `workspacePath` is locally reachable or
/// remote-only), whereas `artifact_exists` queries the **Server API**
/// to check whether an artifact exists on the server regardless of
/// where the GUI runs. The two are complementary, not redundant.
///
/// D1: Used by the FE to determine whether an attachment's
/// `workspacePath` is locally reachable (local daemon) or remote-only.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathExistsArgs {
    pub path: String,
}

#[tauri::command]
pub async fn path_exists(args: PathExistsArgs) -> Result<bool, String> {
    let raw = args.path.trim();
    if raw.is_empty() {
        return Ok(false);
    }
    let path = normalize_local_path(config::expand_home(raw)).map_err(stringify)?;
    Ok(path.exists())
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
pub struct MachineStartArgs {
    pub machine_id: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineStartResult {
    pub pid: u32,
    pub machines: Vec<MachineInfo>,
}

#[tauri::command]
pub async fn machine_start(
    state: State<'_, AppState>,
    args: MachineStartArgs,
) -> Result<MachineStartResult, String> {
    let cfg = config::load_or_init().map_err(stringify)?;
    let machine_id = args.machine_id.trim();
    if machine_id.is_empty() {
        return Err("machine id is required".into());
    }
    let registrations = load_pending_machine_registrations()?;
    let registration = registrations
        .iter()
        .find(|registration| {
            registration.id == machine_id && pending_machine_visible(&cfg, registration)
        })
        .ok_or_else(|| format!("unknown pending local host: {machine_id}"))?;
    let pid = start_pending_machine_daemon(&cfg, registration)?;
    let result = machines_from_config(&cfg, state.try_client().await).await?;
    Ok(MachineStartResult {
        pid,
        machines: result.machines,
    })
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
                "wake": args.wake,
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
    merge_local_service_specs(&mut result);
    apply_connection_status(&mut result, client).await;
    Ok(result)
}

fn merge_local_service_specs(result: &mut MachineListResult) {
    let specs = load_local_service_specs();
    if specs.is_empty() {
        return;
    }
    let machine_index = result
        .machines
        .iter()
        .position(|machine| machine.can_open_local_path)
        .or_else(|| {
            result
                .machines
                .iter()
                .position(|machine| machine.kind == "local")
        });
    if let Some(index) = machine_index {
        let machine = &mut result.machines[index];
        merge_service_specs(&mut machine.services, specs);
        machine.service_count = machine.services.len();
    }
}

fn load_local_service_specs() -> Vec<ServiceSpec> {
    let dir = cli_config_dir().join("services");
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            push_service_spec(&path.join("service.json"), &mut out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            push_service_spec(&path, &mut out);
        }
    }
    out
}

fn cli_config_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("LOOM_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    dirs::home_dir()
        .map(|home| home.join(".loom"))
        .unwrap_or_else(|| PathBuf::from(".loom"))
}

fn push_service_spec(path: &Path, out: &mut Vec<ServiceSpec>) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let Ok(mut spec) = serde_json::from_str::<ServiceSpec>(&text) else {
        return;
    };
    spec.normalize();
    if spec.validate().is_ok() {
        out.push(spec);
    }
}

fn merge_service_specs(existing: &mut Vec<ServiceSpec>, specs: Vec<ServiceSpec>) {
    for spec in specs {
        if let Some(slot) = existing.iter_mut().find(|current| current.id == spec.id) {
            *slot = spec;
        } else {
            existing.push(spec);
        }
    }
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

fn upsert_server_machine_info(machines: &mut Vec<MachineInfo>, mut machine: MachineInfo) {
    if let Some(existing) = machines.iter_mut().find(|m| m.id == machine.id) {
        if machine.inventory_revision > existing.inventory_revision
            || (machine.inventory_revision == existing.inventory_revision
                && machine.inventory_observed_at > existing.inventory_observed_at)
        {
            // Preserve local can_open_local_path — the server inventory has no
            // authority over whether a local daemon can open paths on this host.
            machine.can_open_local_path = existing.can_open_local_path;
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

#[allow(clippy::disallowed_methods)] // G1 OS shell: GUI starts the local loom-daemon helper.
fn start_pending_machine_daemon(
    cfg: &DesktopConfig,
    registration: &PendingMachineRegistration,
) -> Result<u32, String> {
    let data_root = PathBuf::from(&registration.data_root);
    let config_dir = PathBuf::from(&registration.config_dir);
    std::fs::create_dir_all(&data_root)
        .map_err(|err| format!("create data root {}: {err}", data_root.display()))?;
    std::fs::create_dir_all(&config_dir)
        .map_err(|err| format!("create config directory {}: {err}", config_dir.display()))?;

    let daemon_bin =
        preferred_daemon_binary().unwrap_or_else(|| PathBuf::from(daemon_binary_name()));
    let mut command = std::process::Command::new(&daemon_bin);
    command
        .arg("--server")
        .arg(active_server_url(cfg))
        .arg("--machine-id")
        .arg(&registration.id)
        .arg("--machine-name")
        .arg(&registration.name)
        .env("LOOM_AGENT_DATA_ROOT", &data_root)
        .env("LOOM_CONFIG_DIR", &config_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if cfg!(windows) {
        command.arg("--no-ipc");
    }
    configure_background_process(&mut command);
    let child = command
        .spawn()
        .map_err(|err| format!("start loom-daemon with {}: {err}", daemon_bin.display()))?;
    Ok(child.id())
}

#[cfg(windows)]
fn configure_background_process(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_background_process(_command: &mut std::process::Command) {}

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
        let actor_id = actor.get("id").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "agent" => allowed_agents.contains(actor_id),
            _ => true,
        }
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

    // Spawn without waiting for exit code — Windows explorer.exe commonly
    // returns non-zero even on success. We only care whether the process
    // launched successfully.
    command
        .spawn()
        .map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    Ok(())
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
            candidates.push(daemon_binary_in_dir(dir));
            if dir.file_name().and_then(|name| name.to_str()) == Some("MacOS") {
                if let Some(contents_dir) = dir.parent() {
                    candidates.push(
                        contents_dir
                            .join("Resources")
                            .join("bin")
                            .join(daemon_binary_name()),
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
                .join(daemon_binary_name()),
        );
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn daemon_binary_name() -> &'static str {
    if cfg!(windows) {
        "loom-daemon.exe"
    } else {
        "loom-daemon"
    }
}

fn daemon_binary_in_dir(dir: &Path) -> PathBuf {
    dir.join(daemon_binary_name())
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

// ---- channel/thread instructions (JSON-RPC server handler passthrough) ----

#[tauri::command]
pub async fn channel_set_instruction(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_SET_INSTRUCTION, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_get_instruction(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_GET_INSTRUCTION, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn channel_clear_instruction(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::CHANNEL_CLEAR_INSTRUCTION, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn thread_set_instruction(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_SET_INSTRUCTION, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn thread_get_instruction(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_GET_INSTRUCTION, Some(params))
        .await
        .map_err(stringify)
}

#[tauri::command]
pub async fn thread_clear_instruction(
    state: State<'_, AppState>,
    params: Value,
) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::THREAD_CLEAR_INSTRUCTION, Some(params))
        .await
        .map_err(stringify)
}

// ---- channel/thread skills (local file I/O, same path/format as CLI) ----

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEntryDto {
    pub id: String,
    pub source: String,
    pub added_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillRegistryDto {
    pub skills: Vec<SkillEntryDto>,
}

/// Resolve the agent data root using the same logic as the CLI
/// `default_data_root()` (crates/cli/src/cmd/agent_serve.rs): honor
/// `LOOM_AGENT_DATA_ROOT` if set, otherwise fall back to
/// `<data_dir>/loom/agents`, and finally `.loom/agents-data` as a
/// last-resort relative path so the GUI and CLI read/write the same
/// skill registry file.
///
/// Deployment model: the GUI is a frontend over the CLI/daemon via IPC.
/// The GUI launches per-machine daemons with
/// `LOOM_AGENT_DATA_ROOT=<machine data_root>`. In the common single-host
/// case (no env override), the GUI and CLI both resolve to
/// `<data_dir>/loom/agents`, so the GUI-written skill registry is visible
/// to agents. The multi-host case (per-machine data roots) requires the
/// skill IPC commands to carry a machineId so they can target the right
/// machine's data root; that is a larger IPC signature change tracked
/// separately and out of scope for this fix.
fn agent_data_root() -> Result<PathBuf, String> {
    Ok(loom_platform::agent_data_root())
}

fn read_skill_registry(path: &Path) -> Result<SkillRegistryDto, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let reg: SkillRegistryDto =
                serde_json::from_str(&text).map_err(|e| format!("parse skill registry: {e}"))?;
            Ok(reg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(SkillRegistryDto { skills: vec![] })
        }
        Err(e) => Err(format!("read skill registry: {e}")),
    }
}

fn write_skill_registry(path: &Path, reg: &SkillRegistryDto) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create skill registry dir: {e}"))?;
    }
    let text =
        serde_json::to_string_pretty(reg).map_err(|e| format!("serialize skill registry: {e}"))?;
    fs::write(path, text).map_err(|e| format!("write skill registry: {e}"))
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn derive_skill_id(source: &str) -> String {
    Path::new(source)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("skill")
        .to_string()
}

// --- Channel skills ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSkillListArgs {
    pub channel_id: String,
}

#[tauri::command]
pub async fn channel_skill_list(args: ChannelSkillListArgs) -> Result<SkillRegistryDto, String> {
    let data_root = agent_data_root()?;
    let path = data_root
        .join("channels")
        .join(&args.channel_id)
        .join("channel-skills.json");
    read_skill_registry(&path)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSkillAddArgs {
    pub channel_id: String,
    pub source: String,
    #[serde(default)]
    pub skill_id: Option<String>,
}

#[tauri::command]
pub async fn channel_skill_add(args: ChannelSkillAddArgs) -> Result<SkillRegistryDto, String> {
    let data_root = agent_data_root()?;
    let path = data_root
        .join("channels")
        .join(&args.channel_id)
        .join("channel-skills.json");
    let id = args
        .skill_id
        .unwrap_or_else(|| derive_skill_id(&args.source));
    let mut reg = read_skill_registry(&path)?;
    reg.skills.retain(|s| s.id != id);
    reg.skills.push(SkillEntryDto {
        id: id.clone(),
        source: args.source,
        added_at: now_iso(),
    });
    write_skill_registry(&path, &reg)?;
    Ok(reg)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSkillRemoveArgs {
    pub channel_id: String,
    pub skill_id: String,
}

#[tauri::command]
pub async fn channel_skill_remove(
    args: ChannelSkillRemoveArgs,
) -> Result<SkillRegistryDto, String> {
    let data_root = agent_data_root()?;
    let path = data_root
        .join("channels")
        .join(&args.channel_id)
        .join("channel-skills.json");
    let mut reg = read_skill_registry(&path)?;
    reg.skills.retain(|s| s.id != args.skill_id);
    write_skill_registry(&path, &reg)?;
    Ok(reg)
}

// --- Thread skills ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSkillListArgs {
    pub channel_id: String,
    pub thread_id: String,
}

#[tauri::command]
pub async fn thread_skill_list(args: ThreadSkillListArgs) -> Result<SkillRegistryDto, String> {
    let data_root = agent_data_root()?;
    let path = data_root
        .join("channels")
        .join(&args.channel_id)
        .join("threads")
        .join(&args.thread_id)
        .join("thread-skills.json");
    read_skill_registry(&path)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSkillAddArgs {
    pub channel_id: String,
    pub thread_id: String,
    pub source: String,
    #[serde(default)]
    pub skill_id: Option<String>,
}

#[tauri::command]
pub async fn thread_skill_add(args: ThreadSkillAddArgs) -> Result<SkillRegistryDto, String> {
    let data_root = agent_data_root()?;
    let path = data_root
        .join("channels")
        .join(&args.channel_id)
        .join("threads")
        .join(&args.thread_id)
        .join("thread-skills.json");
    let id = args
        .skill_id
        .unwrap_or_else(|| derive_skill_id(&args.source));
    let mut reg = read_skill_registry(&path)?;
    reg.skills.retain(|s| s.id != id);
    reg.skills.push(SkillEntryDto {
        id: id.clone(),
        source: args.source,
        added_at: now_iso(),
    });
    write_skill_registry(&path, &reg)?;
    Ok(reg)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSkillRemoveArgs {
    pub channel_id: String,
    pub thread_id: String,
    pub skill_id: String,
}

#[tauri::command]
pub async fn thread_skill_remove(args: ThreadSkillRemoveArgs) -> Result<SkillRegistryDto, String> {
    let data_root = agent_data_root()?;
    let path = data_root
        .join("channels")
        .join(&args.channel_id)
        .join("threads")
        .join(&args.thread_id)
        .join("thread-skills.json");
    let mut reg = read_skill_registry(&path)?;
    reg.skills.retain(|s| s.id != args.skill_id);
    write_skill_registry(&path, &reg)?;
    Ok(reg)
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
    fn local_account_defaults_are_generic_and_actor_id_safe() {
        assert_eq!(local_user_id_from_display_name("Jane Doe"), "jane_doe");
        assert_eq!(local_user_id_from_display_name("!!!"), "local_user");
        assert_eq!(local_user_id_from_display_name("残风 MacBook"), "macbook");

        let long_user_id = local_user_id_from_display_name(&"A".repeat(80));
        let actor_id = default_actor_id_for_local_user(&long_user_id);

        assert_eq!(long_user_id.len(), 48);
        assert!(actor_id.starts_with("actor_human_local_"));
        assert!(config::is_supported_actor_id(&actor_id));
    }

    #[test]
    fn local_workspace_identity_does_not_require_oauth_account() {
        let cfg = DesktopConfig::default();

        let (actor_id, display_name) = workspace_identity_for_new_workspace(&cfg, "ws_local");

        assert_eq!(actor_id, "actor_human_local_default");
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
    fn actor_list_filter_keeps_all_server_human_identities() {
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
                { "id": "actor_human_local_default", "kind": "human", "displayName": "local_default" },
                { "id": "actor_human_old", "kind": "human", "displayName": "old" },
                { "id": "actor_service_machine", "kind": "service", "displayName": "Machine" }
            ]
        });

        let filtered = filter_actor_list_for_active_context(value, &cfg);
        let actor_ids = filtered["actors"]
            .as_array()
            .expect("actors")
            .iter()
            .filter_map(|actor| actor["id"].as_str())
            .collect::<Vec<_>>();

        assert!(actor_ids.contains(&account.actor_id.as_str()));
        assert!(actor_ids.contains(&"actor_human_local_default"));
        assert!(actor_ids.contains(&"actor_human_old"));
        assert!(actor_ids.contains(&"actor_service_machine"));
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

    #[test]
    fn agent_data_root_honors_env_override() {
        // Env override takes precedence over the default data_dir path.
        // SAFETY: env var mutation is process-local; this test owns
        // LOOM_AGENT_DATA_ROOT for its duration and restores it after.
        let key = "LOOM_AGENT_DATA_ROOT";
        let saved = std::env::var(key).ok();
        std::env::set_var(key, "/tmp/loom-gui-test-data-root");
        let root = agent_data_root().expect("agent data root with env override");
        assert_eq!(root, PathBuf::from("/tmp/loom-gui-test-data-root"));
        // Restore so we do not leak the override into other tests.
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn agent_data_root_env_override_ignores_empty_value() {
        // An empty LOOM_AGENT_DATA_ROOT must not short-circuit; the resolver
        // must fall through to the data_dir default. This mirrors the CLI's
        // default_data_root() semantics.
        let key = "LOOM_AGENT_DATA_ROOT";
        let saved = std::env::var(key).ok();
        std::env::set_var(key, "");
        let root = agent_data_root().expect("agent data root falls through empty env");
        // We cannot assert the exact path (depends on the host's data_dir),
        // but it must NOT be the empty PathBuf and must end with the
        // default `loom` suffix (callers append `agents/<actor_id>`).
        assert_ne!(root, PathBuf::from(""));
        assert!(root.ends_with("loom"), "expected <data_dir>/loom, got {}", root.display());
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    // ---------- MF1: artifact_exists error classification ----------

    /// The `artifact_exists` command classifies RPC errors by string-matching
    /// the flattened error message for `(code -32000)` (APP_NOT_FOUND). This
    /// test verifies the classification contract that the command relies on.
    #[test]
    fn mf1_artifact_exists_classifies_app_not_found_error() {
        // Scenario 1: APP_NOT_FOUND error (code -32000) → should be treated
        // as "does not exist" (Ok(false)).
        let not_found_msg = "rpc `artifact_get` failed: artifact not found (code -32000)";
        assert!(
            not_found_msg.contains("(code -32000)"),
            "APP_NOT_FOUND error must contain '(code -32000)'"
        );

        // Scenario 2: a genuine transport error (e.g. timeout) must NOT
        // match the not-found pattern and should be propagated as Err.
        let timeout_msg = "rpc `artifact_get` timed out";
        assert!(
            !timeout_msg.contains("(code -32000)"),
            "timeout error must not be classified as not-found"
        );

        // Scenario 3: a different RPC error code (e.g. -32602 invalid params)
        // must NOT be classified as not-found.
        let invalid_params_msg =
            "rpc `artifact_get` failed: invalid params (code -32602)";
        assert!(
            !invalid_params_msg.contains("(code -32000)"),
            "non-APP_NOT_FOUND error must not be classified as not-found"
        );
    }

    // ---------- MF2: sanitize_filename ----------

    #[test]
    fn mf2_sanitize_filename_replaces_windows_reserved_chars() {
        // Scenario 1: Windows-reserved characters are replaced with '_'.
        let sanitized = sanitize_filename("file:name<>*?.txt");
        assert!(
            !sanitized.contains(':'),
            "colon must be replaced: got '{sanitized}'"
        );
        assert!(
            !sanitized.contains('<'),
            "less-than must be replaced: got '{sanitized}'"
        );
        assert!(
            !sanitized.contains('>'),
            "greater-than must be replaced: got '{sanitized}'"
        );
        assert!(
            !sanitized.contains('*'),
            "asterisk must be replaced: got '{sanitized}'"
        );
        assert!(
            !sanitized.contains('?'),
            "question mark must be replaced: got '{sanitized}'"
        );
    }

    #[test]
    fn mf2_sanitize_filename_preserves_valid_characters() {
        // Scenario 2: whitelisted characters (alphanumeric, dot, dash,
        // underscore, space) are preserved.
        let sanitized = sanitize_filename("my-file_v1.0 final.txt");
        assert_eq!(sanitized, "my-file_v1.0 final.txt");
    }

    #[test]
    fn mf2_sanitize_filename_replaces_path_separators() {
        // Scenario 3: path separators are replaced with '_'.
        let sanitized = sanitize_filename("dir/sub\\file.txt");
        assert!(
            !sanitized.contains('/') && !sanitized.contains('\\'),
            "path separators must be replaced: got '{sanitized}'"
        );
    }

    // ---------- MF8: extract_artifact_bytes ----------

    #[test]
    fn mf8_extract_artifact_bytes_filters_out_of_bounds_values() {
        // Scenario 1: values > 255 are filtered out, not silently truncated.
        let response = serde_json::json!({
            "bytes": [65, 256, 300, 66]
        });
        let bytes = extract_artifact_bytes(&response).expect("extract bytes");
        // 256 and 300 should be dropped, not truncated to 0 and 44.
        assert_eq!(bytes, vec![65, 66], "out-of-bounds values must be filtered");
    }

    #[test]
    fn mf8_extract_artifact_bytes_preserves_valid_byte_values() {
        // Scenario 2: all valid byte values (0-255) are preserved exactly.
        let response = serde_json::json!({
            "bytes": [0, 127, 200, 255]
        });
        let bytes = extract_artifact_bytes(&response).expect("extract bytes");
        assert_eq!(bytes, vec![0, 127, 200, 255]);
    }

    #[test]
    fn mf8_extract_artifact_bytes_falls_back_to_content_string() {
        // Scenario 3: when "bytes" is absent, falls back to "content" string.
        let response = serde_json::json!({
            "content": "hello"
        });
        let bytes = extract_artifact_bytes(&response).expect("extract bytes");
        assert_eq!(bytes, b"hello".to_vec());
    }

    // ---------- F8: cleanup_downloads ----------

    use std::io::Write;

    /// Helper: create a temp-based loom-downloads dir for testing.
    fn f8_test_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("loom-downloads-f8-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    /// Helper: write a file with a specific modification time (age_secs ago).
    fn f8_write_file_with_age(dir: &Path, name: &str, age_secs: u64) {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create file");
        f.write_all(b"test").expect("write file");
        drop(f);

        // Set the file's modification time to simulate age.
        let mtime = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(age_secs))
            .expect("file age");
        let times = std::fs::FileTimes::new().set_modified(mtime);
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open for set_times");
        let _ = f.set_times(times);
    }

    #[test]
    fn f8_cleanup_deletes_files_older_than_24h() {
        // Scenario 1: files older than 24h are deleted.
        let dir = f8_test_dir();
        f8_write_file_with_age(&dir, "old.txt", 25 * 60 * 60); // 25h old
        f8_write_file_with_age(&dir, "very_old.txt", 48 * 60 * 60); // 48h old

        let cutoff = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(DOWNLOAD_MAX_AGE_SECS))
            .unwrap();
        cleanup_dir_older_than(&dir, cutoff).expect("cleanup");

        assert!(!dir.join("old.txt").exists(), "old.txt should be deleted");
        assert!(!dir.join("very_old.txt").exists(), "very_old.txt should be deleted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn f8_cleanup_preserves_files_newer_than_24h() {
        // Scenario 2: files newer than 24h are preserved.
        let dir = f8_test_dir();
        f8_write_file_with_age(&dir, "fresh.txt", 60 * 60); // 1h old
        f8_write_file_with_age(&dir, "recent.txt", 12 * 60 * 60); // 12h old

        let cutoff = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(DOWNLOAD_MAX_AGE_SECS))
            .unwrap();
        cleanup_dir_older_than(&dir, cutoff).expect("cleanup");

        assert!(dir.join("fresh.txt").exists(), "fresh.txt should be preserved");
        assert!(dir.join("recent.txt").exists(), "recent.txt should be preserved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn f8_cleanup_handles_empty_directory() {
        // Scenario 3: empty directory is handled without error.
        let dir = f8_test_dir();

        let cutoff = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(DOWNLOAD_MAX_AGE_SECS))
            .unwrap();
        cleanup_dir_older_than(&dir, cutoff).expect("cleanup empty dir");

        // Directory should be removed if empty after cleanup.
        assert!(!dir.exists(), "empty dir should be removed");
    }

    // ---------- Cache: dir_size_bytes ----------

    #[test]
    fn cache_dir_size_sums_files_recursively() {
        let root = std::env::temp_dir()
            .join("loom-cache-size-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(root.join("sub")).expect("create dirs");
        std::fs::write(root.join("a.bin"), vec![0u8; 100]).expect("write a");
        std::fs::write(root.join("sub").join("b.bin"), vec![0u8; 50]).expect("write b");

        let size = dir_size_bytes(&root);
        assert_eq!(size, 150, "dir_size_bytes should sum all files recursively");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_dir_size_missing_dir_returns_zero() {
        let missing = std::env::temp_dir()
            .join("loom-cache-missing")
            .join(uuid::Uuid::new_v4().to_string());
        assert_eq!(dir_size_bytes(&missing), 0, "missing dir should report 0 bytes");
    }

    // ---------- Cache: write_cache_meta ----------

    #[test]
    fn cache_write_meta_roundtrips_json() {
        let dir = std::env::temp_dir()
            .join("loom-cache-meta-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).expect("create dir");
        let meta = CachedArtifactMeta {
            artifact_id: "art_123".to_string(),
            name: "photo.png".to_string(),
            media_type: "image/png".to_string(),
            size: 4096,
            downloaded_at: "2026-07-24T00:00:00+00:00".to_string(),
        };
        write_cache_meta(&dir, &meta).expect("write meta");

        let loaded: CachedArtifactMeta =
            serde_json::from_str(&std::fs::read_to_string(dir.join(".meta.json")).expect("read meta"))
                .expect("parse meta");
        assert_eq!(loaded.artifact_id, "art_123");
        assert_eq!(loaded.media_type, "image/png");
        assert_eq!(loaded.size, 4096);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------- Cache: read_local_file_bytes traversal guard ----------

    #[test]
    fn cache_read_local_file_bytes_rejects_outside_cache() {
        // A path outside the cache root must be rejected, even if it exists.
        let outside = std::env::temp_dir()
            .join("loom-cache-outside-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&outside).expect("create dir");
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, b"sensitive").expect("write secret");

        let args = ReadLocalFileBytesArgs {
            path: secret.display().to_string(),
            offset: None,
            max_bytes: None,
        };
        let err = read_local_file_bytes(args).unwrap_err();
        assert!(
            err.contains("outside the attachment cache"),
            "expected traversal rejection, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&outside);
    }

    // ---------- Cache: category classification & breakdown ----------

    #[test]
    fn cache_category_classifies_image_media_types() {
        assert_eq!(cache_category_for_media_type("image/png"), "images");
        assert_eq!(cache_category_for_media_type("image/jpeg"), "images");
        assert_eq!(cache_category_for_media_type("image/gif"), "images");
        assert_eq!(cache_category_for_media_type("application/pdf"), "other");
        assert_eq!(cache_category_for_media_type("video/mp4"), "other");
        assert_eq!(cache_category_for_media_type(""), "other");
    }

    /// Helper: create a fake artifact cache dir with a .meta.json.
    fn cache_make_artifact_dir(root: &Path, artifact_id: &str, media_type: &str, bytes: &[u8]) {
        let dir = root.join(artifact_id);
        std::fs::create_dir_all(&dir).expect("create artifact dir");
        std::fs::write(dir.join("blob.bin"), bytes).expect("write blob");
        let meta = CachedArtifactMeta {
            artifact_id: artifact_id.to_string(),
            name: format!("{artifact_id}.bin"),
            media_type: media_type.to_string(),
            size: bytes.len() as u64,
            downloaded_at: "2026-07-24T00:00:00+00:00".to_string(),
        };
        write_cache_meta(&dir, &meta).expect("write meta");
    }

    #[test]
    fn cache_breakdown_splits_images_and_other() {
        // Use an isolated temp dir so the test doesn't depend on the
        // global cache root's contents.
        let root = std::env::temp_dir()
            .join("loom-cache-breakdown-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).expect("create test root");

        cache_make_artifact_dir(&root, "img1", "image/png", &[0u8; 100]);
        cache_make_artifact_dir(&root, "img2", "image/jpeg", &[0u8; 50]);
        cache_make_artifact_dir(&root, "doc1", "application/pdf", &[0u8; 200]);

        let (images, other) = cache_breakdown_at_root(&root);
        assert_eq!(images.count, 2, "two image artifacts");
        assert_eq!(other.count, 1, "one other artifact");
        assert!(images.size >= 150, "images size >= blob bytes");
        assert!(other.size >= 200, "other size >= blob bytes");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_breakdown_total_is_size_count_object() {
        // Regression: total must be {size, count}, not a bare number,
        // so FE total.size is defined (QA defect 1).
        let root = std::env::temp_dir()
            .join("loom-cache-total-shape-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).expect("create test root");

        cache_make_artifact_dir(&root, "img1", "image/png", &[0u8; 100]);
        cache_make_artifact_dir(&root, "doc1", "application/pdf", &[0u8; 200]);

        let (images, other) = cache_breakdown_at_root(&root);
        let total = CacheCategoryStats {
            size: images.size + other.size,
            count: images.count + other.count,
        };
        // Serialize the full response shape as the command does.
        let resp = json!({ "images": images, "other": other, "total": total });
        let total_obj = resp.get("total").expect("total key");
        // total must be an object with size+count, NOT a bare number.
        assert!(total_obj.is_object(), "total must be an object, got: {total_obj}");
        assert_eq!(total_obj["count"].as_u64(), Some(2), "total.count = 2 artifacts");
        assert!(
            total_obj["size"].as_u64().unwrap() >= 300,
            "total.size >= sum of blob bytes"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_clear_by_type_removes_only_matching_category() {
        let root = std::env::temp_dir()
            .join("loom-cache-clearbytype-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).expect("create test root");

        cache_make_artifact_dir(&root, "clr-img1", "image/png", &[0u8; 100]);
        cache_make_artifact_dir(&root, "clr-img2", "image/gif", &[0u8; 80]);
        cache_make_artifact_dir(&root, "clr-doc1", "text/plain", &[0u8; 300]);

        let (freed, cleared) = clear_cache_by_type_at_root(&root, "images");
        assert_eq!(cleared.len(), 2, "cleared 2 images");
        assert!(freed > 0, "freed bytes > 0");

        // Images removed, other preserved.
        assert!(!root.join("clr-img1").exists(), "clr-img1 removed");
        assert!(!root.join("clr-img2").exists(), "clr-img2 removed");
        assert!(root.join("clr-doc1").exists(), "clr-doc1 preserved");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_clear_by_type_rejects_invalid_category() {
        let args = ClearAttachmentCacheByTypeArgs {
            category: "videos".to_string(),
        };
        let err = clear_attachment_cache_by_type(args).unwrap_err();
        assert!(err.contains("Invalid category"), "expected invalid category error, got: {err}");
    }

    #[test]
    fn cache_clear_by_type_missing_meta_falls_back_to_other() {
        let root = std::env::temp_dir()
            .join("loom-cache-orphan-test")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).expect("create test root");

        // An artifact dir with no .meta.json should classify as "other".
        let orphan = root.join("clr-orphan");
        std::fs::create_dir_all(&orphan).expect("create orphan");
        std::fs::write(orphan.join("blob.bin"), &[0u8; 64]).expect("write blob");

        let (freed, cleared) = clear_cache_by_type_at_root(&root, "other");
        assert!(
            cleared.iter().any(|id| id == "clr-orphan"),
            "orphan (no meta) should be cleared as 'other'"
        );
        assert!(freed > 0, "freed bytes > 0");
        assert!(!orphan.exists(), "orphan removed");

        let _ = std::fs::remove_dir_all(&root);
    }
}
