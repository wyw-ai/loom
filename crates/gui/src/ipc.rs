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

use std::sync::Arc;

use proto::methods::method;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::config::{self, DesktopConfig, Workspace};
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
    Ok(args.config)
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
    let id = config::generate_id();
    let ws = Workspace {
        id: id.clone(),
        name: args.name,
        server_url: args.server_url,
        actor_id: args.actor_id,
        display_name: args.display_name,
    };
    cfg.workspaces.push(ws);
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

#[tauri::command]
pub async fn agent_list(state: State<'_, AppState>) -> Result<Value, String> {
    state
        .client()
        .await?
        .call_raw(method::AGENT_LIST, None)
        .await
        .map_err(stringify)
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
