mod archive_naming;
mod commands;
mod history;
mod keychain;
mod profiles;
mod tray;

use borg_core::borg::BorgClient;
use commands::AppState;
use tauri::WindowEvent;
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("borg_ui=debug".parse().expect("valid tracing directive")),
        )
        .init();

    let borg_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default()
        .join("borg.exe");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            tray::setup(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .manage(AppState {
            borg: BorgClient::new(borg_path),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_borg_version,
            commands::test_ssh_connection,
            commands::get_repo_info,
            commands::list_archives,
            commands::list_archive_contents,
            commands::init_repo,
            commands::delete_archive,
            commands::prune_repo,
            commands::load_retention_config,
            commands::save_retention_config,
            commands::create_backup,
            commands::restore_archive,
            commands::load_repo_config,
            commands::save_repo_config,
            commands::load_schedule_config,
            commands::save_schedule_config,
            commands::record_backup_event,
            commands::load_backup_history,
            commands::clear_backup_history,
            commands::set_repo_passphrase,
            commands::clear_repo_passphrase,
            commands::has_repo_passphrase,
            commands::list_profiles,
            commands::set_active_profile,
            commands::create_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::set_profile_template,
            commands::preview_archive_name,
            commands::export_profile,
            commands::import_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
