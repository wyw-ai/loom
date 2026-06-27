// On Windows, hide the console window that would otherwise pop up alongside
// the GUI. Harmless on mac/linux.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod avatar;
mod config;
mod dev_frontend;
mod forward;
mod ipc;
mod state;
mod ws;

use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::state::AppState;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,loom_gui=debug")),
        )
        .init();

    let _dev_frontend = dev_frontend::DevFrontend::start_if_needed();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            inner: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            ipc::workspaces_list,
            ipc::workspaces_save,
            ipc::account_get,
            ipc::account_auth_status,
            ipc::account_login,
            ipc::account_logout,
            ipc::avatar_cached_url,
            ipc::workspace_add,
            ipc::workspace_remove,
            ipc::set_active_workspace,
            ipc::connect,
            ipc::disconnect,
            ipc::channel_list,
            ipc::channel_create,
            ipc::channel_update,
            ipc::channel_delete,
            ipc::channel_invite,
            ipc::channel_revoke,
            ipc::channel_members,
            ipc::thread_list,
            ipc::thread_create,
            ipc::thread_update,
            ipc::thread_archive,
            ipc::thread_delete,
            ipc::scope_subscribe,
            ipc::scope_unsubscribe,
            ipc::message_list,
            ipc::message_send,
            ipc::message_read,
            ipc::message_reaction_toggle,
            ipc::task_create,
            ipc::task_get,
            ipc::task_list,
            ipc::task_update,
            ipc::task_ref_attach,
            ipc::task_ref_find,
            ipc::task_ref_list,
            ipc::task_artifact_attach,
            ipc::task_artifact_activate,
            ipc::task_artifact_list,
            ipc::task_fact_append,
            ipc::task_fact_list,
            ipc::task_projection_put,
            ipc::task_projection_get,
            ipc::task_projection_list,
            ipc::task_assignment_create,
            ipc::task_assignment_update,
            ipc::task_assignment_context,
            ipc::task_assignment_preflight,
            ipc::task_change_list,
            ipc::task_change_ack,
            ipc::inbox_list,
            ipc::delivery_ack,
            ipc::task_workspace_lease_acquire,
            ipc::task_workspace_lease_release,
            ipc::task_workspace_lease_list,
            ipc::artifact_publish,
            ipc::artifact_get,
            ipc::artifact_read,
            ipc::run_cancel,
            ipc::reminder_list,
            ipc::actor_list,
            ipc::actor_upsert,
            ipc::actor_delete,
            ipc::agent_list,
            ipc::agent_create,
            ipc::agent_update,
            ipc::agent_prompt_preview,
            ipc::agent_file_list,
            ipc::agent_file_read,
            ipc::agent_file_write,
            ipc::agent_remove,
            ipc::provider_add,
            ipc::provider_remove,
            ipc::machine_list,
            ipc::machine_check,
            ipc::open_local_path,
            ipc::machine_create,
            ipc::machine_remove,
            ipc::machine_agent_create,
            ipc::machine_agent_remove,
        ])
        .setup(|app| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
            #[cfg(debug_assertions)]
            if std::env::var_os("LOOM_GUI_OPEN_DEVTOOLS").is_some() {
                if let Some(win) = app.get_webview_window("main") {
                    win.open_devtools();
                }
            }
            let _ = app;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri app failed to start");
}
