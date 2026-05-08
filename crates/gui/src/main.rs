// On Windows, hide the console window that would otherwise pop up alongside
// the GUI. Harmless on mac/linux.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
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
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,joi_gui=debug")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            inner: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            ipc::workspaces_list,
            ipc::workspaces_save,
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
            ipc::thread_delete,
            ipc::scope_subscribe,
            ipc::scope_unsubscribe,
            ipc::scope_read,
            ipc::event_append,
            ipc::turn_close,
            ipc::actor_list,
            ipc::agent_list,
            ipc::agent_create,
            ipc::agent_update,
            ipc::agent_remove,
            ipc::machine_list,
            ipc::machine_check,
            ipc::machine_create,
            ipc::machine_remove,
            ipc::machine_agent_create,
            ipc::machine_agent_remove,
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            if let Some(win) = app.get_webview_window("main") {
                win.open_devtools();
            }
            let _ = app;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri app failed to start");
}
