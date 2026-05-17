use std::path::PathBuf;

use borg_core::borg::{ArchiveInfo, BorgClient};
use borg_core::config::RepoConfig;
use tauri::{Manager, State};

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
    state.borg.info(&repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_archives(
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<Vec<ArchiveInfo>, String> {
    state.borg.list_archives(&repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_backup(
    state: State<'_, AppState>,
    repo: RepoConfig,
    source_paths: Vec<String>,
    archive_name: String,
) -> Result<(), String> {
    let profile = borg_core::config::BackupProfile {
        name: "manual".into(),
        source_paths: source_paths.into_iter().map(PathBuf::from).collect(),
        excludes: vec![],
        compression: borg_core::config::Compression::default(),
        repo,
    };

    state
        .borg
        .create(&profile, &archive_name, |_event| {})
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_repo_config(app: tauri::AppHandle) -> Result<Option<RepoConfig>, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let config_path = config_dir.join("repo.json");
    if !config_path.exists() {
        return Ok(None);
    }
    let data = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| e.to_string())?;
    let config: RepoConfig = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(Some(config))
}

#[tauri::command]
pub async fn save_repo_config(app: tauri::AppHandle, repo: RepoConfig) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&config_dir)
        .await
        .map_err(|e| e.to_string())?;
    let config_path = config_dir.join("repo.json");
    let data = serde_json::to_string_pretty(&repo).map_err(|e| e.to_string())?;
    tokio::fs::write(&config_path, data)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
