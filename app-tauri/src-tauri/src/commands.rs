use std::path::PathBuf;

use borg_core::borg::{ArchiveInfo, BorgClient};
use borg_core::config::RepoConfig;
use tauri::{Emitter, Manager, State};

pub struct AppState {
    pub borg: BorgClient,
}

#[tauri::command]
pub async fn get_borg_version(state: State<'_, AppState>) -> Result<String, String> {
    state.borg.version().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_ssh_connection(
    host: String,
    port: u16,
    user: String,
    key_path: Option<String>,
) -> Result<bool, String> {
    let key = key_path.map(PathBuf::from);
    borg_core::ssh::test_connection(&host, port, &user, key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_repo_info(
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<serde_json::Value, String> {
    repo.validate().map_err(|e| e.to_string())?;
    state.borg.info(&repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_archives(
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<Vec<ArchiveInfo>, String> {
    repo.validate().map_err(|e| e.to_string())?;
    state
        .borg
        .list_archives(&repo)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn init_repo(
    state: State<'_, AppState>,
    repo: RepoConfig,
    encryption: String,
    passphrase: Option<String>,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_encryption_mode(&encryption).map_err(|e| e.to_string())?;

    let needs_pass = encryption != "none"
        && encryption != "authenticated"
        && encryption != "authenticated-blake2";
    if needs_pass && passphrase.as_deref().unwrap_or("").is_empty() {
        return Err("passphrase required for this encryption mode".into());
    }

    state
        .borg
        .init_repo(&repo, &encryption, passphrase.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_archive(
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    state
        .borg
        .delete_archive(&repo, &archive_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    source_paths: Vec<String>,
    archive_name: String,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    let compression = borg_core::config::Compression::default();
    compression.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    borg_core::config::validate_source_paths(&source_paths).map_err(|e| e.to_string())?;

    let raw_paths: Vec<PathBuf> = source_paths.into_iter().map(PathBuf::from).collect();
    let (backup_paths, snapshots) = borg_platform_win::vss::snapshot_sources(&raw_paths).await;

    let profile = borg_core::config::BackupProfile {
        name: "manual".into(),
        source_paths: backup_paths,
        excludes: vec![],
        compression,
        repo,
    };

    let result = state
        .borg
        .create(&profile, &archive_name, move |event| {
            let _ = app.emit("backup-progress", &event);
        })
        .await
        .map_err(|e| e.to_string());

    borg_platform_win::vss::release_all(snapshots).await;

    result
}

#[tauri::command]
pub async fn restore_archive(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
    destination: String,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;

    let dest_path = PathBuf::from(&destination);
    if !dest_path.is_dir() {
        return Err(format!("destination does not exist: {}", destination));
    }

    state
        .borg
        .extract(&repo, &archive_name, &dest_path, move |event| {
            let _ = app.emit("restore-progress", &event);
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_schedule_config(
    app: tauri::AppHandle,
) -> Result<Option<borg_platform_win::scheduler::ScheduleConfig>, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let config_path = config_dir.join("schedule.json");
    match tokio::fs::read_to_string(&config_path).await {
        Ok(data) => {
            let config: borg_platform_win::scheduler::ScheduleConfig =
                serde_json::from_str(&data).map_err(|e| e.to_string())?;
            Ok(Some(config))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn save_schedule_config(
    app: tauri::AppHandle,
    config: borg_platform_win::scheduler::ScheduleConfig,
) -> Result<(), String> {
    config.schedule.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_source_paths(&config.source_paths).map_err(|e| e.to_string())?;

    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&config_dir)
        .await
        .map_err(|e| e.to_string())?;
    let config_path = config_dir.join("schedule.json");
    let tmp_path = config_dir.join("schedule.json.tmp");
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    tokio::fs::write(&tmp_path, &data)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp_path, &config_path)
        .await
        .map_err(|e| e.to_string())?;

    if config.enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_str = exe.to_string_lossy().to_string();
        let args = "--scheduled-backup";
        borg_platform_win::scheduler::schedule_backup(
            "BorgUI-Backup",
            &exe_str,
            args,
            &config.schedule,
        )
        .await
        .map_err(|e| e.to_string())?;
    } else {
        let _ = borg_platform_win::scheduler::unschedule_backup("BorgUI-Backup").await;
    }

    Ok(())
}

#[tauri::command]
pub async fn load_repo_config(app: tauri::AppHandle) -> Result<Option<RepoConfig>, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let config_path = config_dir.join("repo.json");
    match tokio::fs::read_to_string(&config_path).await {
        Ok(data) => {
            let config: RepoConfig = serde_json::from_str(&data).map_err(|e| e.to_string())?;
            Ok(Some(config))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn save_repo_config(app: tauri::AppHandle, repo: RepoConfig) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&config_dir)
        .await
        .map_err(|e| e.to_string())?;
    let config_path = config_dir.join("repo.json");
    let tmp_path = config_dir.join("repo.json.tmp");
    let data = serde_json::to_string_pretty(&repo).map_err(|e| e.to_string())?;
    tokio::fs::write(&tmp_path, &data)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp_path, &config_path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
