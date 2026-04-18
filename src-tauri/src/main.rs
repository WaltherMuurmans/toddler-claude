#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod claude_setup;
mod commands;
mod credentials;
mod fly;
mod fly_setup;
mod github_oauth;
mod session;

use tauri::Manager;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(session::SessionState::default())
        .invoke_handler(tauri::generate_handler![
            commands::store_claude_token,
            commands::has_claude_token,
            commands::clear_claude_token,
            commands::claude_auto_setup,
            commands::github_device_start,
            commands::github_device_poll,
            commands::store_github_token,
            commands::has_github_token,
            commands::list_github_repos,
            commands::github_cli_signin,
            commands::save_config,
            commands::load_config,
            commands::store_fly_token,
            commands::has_fly_token,
            commands::fly_cli_signin,
            commands::start_session,
            commands::stop_session,
            commands::session_status,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                session::idle_reaper(handle).await;
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running toddler-claude");
}
