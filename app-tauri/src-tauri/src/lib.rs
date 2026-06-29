mod archive_naming;
mod commands;
mod diagnostics;
mod history;
mod keychain;
mod logging;
mod profiles;
mod recovery;
mod redaction;
mod scheduled;
mod tray;

use borg_core::borg::BorgClient;
use commands::AppState;
use tauri::{Manager, WindowEvent};

/// CLI flag the Windows Task Scheduler entry passes to trigger a headless
/// backup (see `commands::save_schedule_config`).
const SCHEDULED_BACKUP_FLAG: &str = "--scheduled-backup";
const SCHEDULED_INTEGRITY_FLAG: &str = "--scheduled-integrity-check";

/// CLI flag the autostart `Run`-key entry passes so BorgUI starts hidden in the
/// tray at login instead of popping the window open (see `commands::set_autostart`).
const START_MINIMIZED_FLAG: &str = "--minimized";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let borg_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default()
        .join("borg.exe");

    // When launched by the Task Scheduler we run one backup headlessly and exit,
    // rather than showing the GUI.
    let scheduled = std::env::args().any(|a| a == SCHEDULED_BACKUP_FLAG);
    let scheduled_integrity = std::env::args().any(|a| a == SCHEDULED_INTEGRITY_FLAG);
    let start_minimized = std::env::args().any(|a| a == START_MINIMIZED_FLAG);
    let setup_borg_path = borg_path.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            let log_dir = app.path().app_log_dir()?;
            logging::initialize(&log_dir).map_err(std::io::Error::other)?;
            if scheduled_integrity {
                start_scheduled_integrity_check(app.handle().clone(), setup_borg_path);
            } else if scheduled {
                start_scheduled_backup(app.handle().clone(), setup_borg_path);
            } else {
                tray::setup(app.handle())?;
                // Autostart-at-login launches with `--minimized`: keep the window
                // hidden so BorgUI sits in the tray instead of stealing focus.
                if start_minimized && let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
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
        .manage(AppState::new(BorgClient::new(borg_path)))
        .invoke_handler(tauri::generate_handler![
            commands::get_borg_version,
            commands::test_ssh_connection,
            commands::check_host_reachable,
            commands::validate_ssh_key,
            commands::generate_ssh_key,
            commands::get_repo_info,
            commands::list_archives,
            commands::stream_archive_contents,
            commands::diff_archives,
            commands::compact_repo,
            commands::init_repo,
            commands::delete_archive,
            commands::prune_repo,
            commands::load_retention_config,
            commands::save_retention_config,
            commands::create_backup,
            commands::cancel_backup,
            commands::restore_archive,
            commands::cancel_restore,
            commands::check_repository,
            commands::cancel_repository_check,
            commands::latest_integrity_check,
            commands::set_monthly_integrity_check,
            commands::load_repo_config,
            commands::save_repo_config,
            commands::load_schedule_config,
            commands::save_schedule_config,
            commands::scheduled_backup_status,
            commands::record_backup_event,
            commands::load_backup_history,
            commands::clear_backup_history,
            commands::get_autostart,
            commands::set_autostart,
            commands::set_repo_passphrase,
            commands::clear_repo_passphrase,
            commands::has_repo_passphrase,
            commands::list_profiles,
            commands::set_active_profile,
            commands::create_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::set_profile_template,
            commands::set_profile_hooks,
            commands::preview_archive_name,
            commands::export_profile,
            commands::import_profile,
            commands::open_log_folder,
            commands::export_support_bundle,
            commands::export_configuration,
            commands::preview_configuration_import,
            commands::import_configuration,
            commands::export_recovery_key,
            commands::import_recovery_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn start_scheduled_integrity_check(app: tauri::AppHandle, borg_path: std::path::PathBuf) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    tauri::async_runtime::spawn(async move {
        let code = match app.path().app_config_dir() {
            Ok(config_dir) => {
                let borg = BorgClient::new(borg_path);
                match scheduled::run_scheduled_integrity_check(&config_dir, &borg).await {
                    Ok(_) => 0,
                    Err(error) => {
                        tracing::error!("scheduled integrity check failed: {error}");
                        1
                    }
                }
            }
            Err(error) => {
                tracing::error!("scheduled integrity check: cannot resolve config dir: {error}");
                1
            }
        };
        app.exit(code);
    });
}

/// Headless path: hide the window, run one backup from the active profile's
/// schedule, notify the user, then exit with a status code the Task Scheduler
/// can surface (0 success, 1 failure).
fn start_scheduled_backup(app: tauri::AppHandle, borg_path: std::path::PathBuf) {
    // A scheduled run is headless — keep the window out of sight.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    tauri::async_runtime::spawn(async move {
        let config_dir = match app.path().app_config_dir() {
            Ok(dir) => dir,
            Err(e) => {
                tracing::error!("scheduled backup: cannot resolve config dir: {e}");
                app.exit(1);
                return;
            }
        };

        let borg = BorgClient::new(borg_path);
        let report = scheduled::run_scheduled_backup(&config_dir, &borg).await;
        notify_scheduled_result(&app, &report);

        let code = if report.succeeded() { 0 } else { 1 };
        app.exit(code);
    });
}

/// Surface the scheduled-run outcome as a desktop notification.
fn notify_scheduled_result(app: &tauri::AppHandle, report: &scheduled::RunReport) {
    use tauri_plugin_notification::NotificationExt;

    let archive = report.archive_name.as_deref().unwrap_or("backup");
    let (title, body) = if let Some(error) = &report.error {
        // `error` is the verbose `BorgError::detail()` (full stderr tail) that the
        // history record wants; a toast wants one readable sentence, so take the
        // first line and cap it rather than dumping a borg `--log-json` blob.
        let first = error.lines().next().unwrap_or(error);
        let body: String = first.chars().take(160).collect();
        ("Scheduled backup failed".to_string(), body)
    } else if report.warnings.is_empty() {
        ("Scheduled backup complete".to_string(), archive.to_string())
    } else {
        let n = report.warnings.len();
        (
            "Scheduled backup completed with warnings".to_string(),
            format!("{archive} — {n} warning{}", if n == 1 { "" } else { "s" }),
        )
    };

    let _ = app.notification().builder().title(title).body(body).show();
}
