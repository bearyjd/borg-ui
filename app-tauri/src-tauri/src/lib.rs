mod commands;

use borg_core::borg::BorgClient;
use commands::AppState;
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
        .manage(AppState {
            borg: BorgClient::new(borg_path),
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_borg_version,
            commands::test_ssh_connection,
            commands::get_repo_info,
            commands::list_archives,
            commands::init_repo,
            commands::delete_archive,
            commands::create_backup,
            commands::restore_archive,
            commands::load_repo_config,
            commands::save_repo_config,
            commands::load_schedule_config,
            commands::save_schedule_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
