mod archive_naming;
mod commands;
mod coverage;
mod diagnostics;
mod hardening;
mod health;
mod history;
mod keychain;
mod logging;
mod network;
mod profiles;
mod recovery;
mod redaction;
mod removable;
mod reporting;
mod scheduled;
mod snooze;
mod tray;

use borg_core::borg::BorgClient;
use commands::AppState;
use tauri::{Manager, WindowEvent};

/// CLI flag the Windows Task Scheduler entry passes to trigger a headless
/// backup (see `commands::save_schedule_config`).
const SCHEDULED_BACKUP_FLAG: &str = "--scheduled-backup";
const SCHEDULED_INTEGRITY_FLAG: &str = "--scheduled-integrity-check";
const SCHEDULED_RESTORE_DRILL_FLAG: &str = "--scheduled-restore-drill";
const SCHEDULED_HEALTH_REPORT_FLAG: &str = "--scheduled-health-report";

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
    let scheduled_restore_drill = std::env::args().any(|a| a == SCHEDULED_RESTORE_DRILL_FLAG);
    let scheduled_health_report = std::env::args().any(|a| a == SCHEDULED_HEALTH_REPORT_FLAG);
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
            if scheduled_health_report {
                start_scheduled_health_report(app.handle().clone());
            } else if scheduled_restore_drill {
                start_scheduled_restore_drill(app.handle().clone(), setup_borg_path.clone());
            } else if scheduled_integrity {
                start_scheduled_integrity_check(app.handle().clone(), setup_borg_path.clone());
            } else if scheduled {
                start_scheduled_backup(app.handle().clone(), setup_borg_path.clone());
            } else {
                tray::setup(app.handle())?;
                start_removable_monitor(app.handle().clone(), setup_borg_path.clone());
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
            commands::cancel_archive_listing,
            commands::search_restore_files,
            commands::cancel_restore_search,
            commands::preview_restore_conflicts,
            commands::diff_archives,
            commands::compact_repo,
            commands::generate_append_only_instructions,
            commands::save_hardening_posture,
            commands::hardening_checklist,
            commands::protection_health,
            commands::init_repo,
            commands::delete_archive,
            commands::prune_repo,
            commands::load_retention_config,
            commands::save_retention_config,
            commands::create_backup,
            commands::discover_backup_sources,
            commands::scan_backup_sources,
            commands::cancel_backup_source_scan,
            commands::load_backup_selection,
            commands::save_backup_selection,
            commands::standard_backup_excludes,
            commands::load_resource_policy,
            commands::save_resource_policy,
            commands::set_global_snooze,
            commands::get_global_snooze,
            commands::reporting_secret_status,
            commands::save_reporting_settings,
            commands::send_test_report,
            commands::cancel_backup,
            commands::restore_archive,
            commands::cancel_restore,
            commands::check_repository,
            commands::cancel_repository_check,
            commands::latest_integrity_check,
            commands::set_monthly_integrity_check,
            commands::set_monthly_restore_drill,
            commands::latest_restore_drill,
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

fn start_removable_monitor(app: tauri::AppHandle, borg_path: std::path::PathBuf) {
    tauri::async_runtime::spawn(async move {
        let Ok(config_dir) = app.path().app_config_dir() else {
            return;
        };
        let borg = BorgClient::new(borg_path);
        let mut trigger = removable::TriggerState::default();
        loop {
            let profile = profiles::load(&config_dir)
                .await
                .ok()
                .and_then(|data| data.active().cloned());
            let present = profile.as_ref().is_some_and(|profile| {
                profile.resource_policy.removable_destination_trigger
                    && profile.repo.is_local()
                    && removable::removable_destination_present(std::path::Path::new(
                        &profile.repo.repo_path,
                    ))
            });
            if trigger.update(present) {
                let report = scheduled::run_removable_backup(&config_dir, &borg).await;
                notify_scheduled_result(&app, &report);
            }
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        }
    });
}

fn start_scheduled_restore_drill(app: tauri::AppHandle, borg_path: std::path::PathBuf) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    tauri::async_runtime::spawn(async move {
        let code = match app.path().app_config_dir() {
            Ok(config_dir) => {
                let borg = BorgClient::new(borg_path);
                match scheduled::run_restore_drill(&config_dir, &borg).await {
                    Ok(()) => 0,
                    Err(error) => {
                        tracing::error!("scheduled restore drill failed: {error}");
                        1
                    }
                }
            }
            Err(error) => {
                tracing::error!("scheduled restore drill: cannot resolve config dir: {error}");
                1
            }
        };
        app.exit(code);
    });
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
        if let Ok(data) = profiles::load(&config_dir).await
            && let Some(profile) = data.active()
        {
            reporting::report_backup_outcome(&config_dir, profile, &report).await;
        }
        notify_scheduled_result(&app, &report);

        let code = if report.succeeded() { 0 } else { 1 };
        app.exit(code);
    });
}

fn start_scheduled_health_report(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    tauri::async_runtime::spawn(async move {
        let code = match app.path().app_config_dir() {
            Ok(config_dir) => match reporting::run_daily(&config_dir).await {
                Ok(()) => 0,
                Err(error) => {
                    tracing::error!("scheduled health report failed: {error}");
                    1
                }
            },
            Err(error) => {
                tracing::error!("scheduled health report: cannot resolve config dir: {error}");
                1
            }
        };
        app.exit(code);
    });
}

/// Surface the scheduled-run outcome as a desktop notification.
fn notify_scheduled_result(app: &tauri::AppHandle, report: &scheduled::RunReport) {
    use tauri_plugin_notification::NotificationExt;

    let archive = report.archive_name.as_deref().unwrap_or("backup");
    let (title, body) = if let Some(reason) = &report.skipped_reason {
        ("Scheduled backup skipped".to_string(), reason.clone())
    } else if let Some(error) = &report.error {
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
