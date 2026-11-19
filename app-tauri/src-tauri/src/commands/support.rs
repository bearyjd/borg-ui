//! Log folder, support bundle, and configuration import/export.

use super::*;

#[tauri::command]
pub async fn open_log_folder(app: tauri::AppHandle) -> Result<(), String> {
    let path = app.path().app_log_dir().map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        let result = std::process::Command::new("explorer").arg(&path).spawn();
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("open").arg(&path).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let result = std::process::Command::new("xdg-open").arg(&path).spawn();
        result.map(|_| ()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_support_bundle(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let config_dir = config_dir(&app).await?;
    let log_dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    diagnostics::export_support_bundle(&config_dir, &log_dir, &PathBuf::from(path)).await
}

#[tauri::command]
pub async fn export_configuration(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let config_dir = config_dir(&app).await?;
    diagnostics::export_configuration(&config_dir, &PathBuf::from(path)).await
}

#[tauri::command]
pub async fn preview_configuration_import(
    app: tauri::AppHandle,
    path: String,
) -> Result<ImportPreview, String> {
    let config_dir = config_dir(&app).await?;
    diagnostics::preview_import(&config_dir, &PathBuf::from(path)).await
}

/// Result of `import_configuration`. The configuration is already saved when
/// this is returned; `schedule_warning` reports a failed backup-task sync so the
/// UI can reload profiles and offer a scheduler-only retry.
#[derive(serde::Serialize)]
pub struct ConfigurationImportOutcome {
    pub schedule_warning: Option<String>,
}

#[tauri::command]
pub async fn import_configuration(
    app: tauri::AppHandle,
    path: String,
) -> Result<ConfigurationImportOutcome, String> {
    let config_dir = config_dir(&app).await?;
    diagnostics::import_configuration(&config_dir, &PathBuf::from(path)).await?;
    // The import replaced every profile, so the OS task may now be stale.
    let data = profiles::load(&config_dir).await?;
    let schedule_warning = schedule::reconcile_backup_task(data.active())
        .await
        .err()
        .map(|e| {
            format!("Configuration imported, but the backup schedule could not be updated: {e}")
        });
    Ok(ConfigurationImportOutcome { schedule_warning })
}
