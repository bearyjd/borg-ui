use std::path::PathBuf;

use borg_core::borg::{ArchiveInfo, BorgClient};
use borg_core::config::RepoConfig;
use tauri::{Emitter, Manager, State};

use crate::archive_naming::{self, TemplateContext};
use crate::history::{self, BackupEvent};
use crate::keychain;
use crate::profiles::{self, Profile, ProfilesData};

fn lookup_passphrase(repo: &RepoConfig) -> Option<String> {
    keychain::get_passphrase(&repo.ssh_url()).ok().flatten()
}

async fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

async fn read_profiles(app: &tauri::AppHandle) -> Result<ProfilesData, String> {
    let dir = config_dir(app).await?;
    profiles::load(&dir).await
}

async fn write_profiles(app: &tauri::AppHandle, data: &ProfilesData) -> Result<(), String> {
    let dir = config_dir(app).await?;
    profiles::save(&dir, data).await
}

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
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .info(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_archives(
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<Vec<ArchiveInfo>, String> {
    repo.validate().map_err(|e| e.to_string())?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .list_archives(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_retention_config(
    app: tauri::AppHandle,
) -> Result<Option<borg_core::config::RetentionConfig>, String> {
    let data = read_profiles(&app).await?;
    Ok(data.active().and_then(|p| p.retention.clone()))
}

#[tauri::command]
pub async fn save_retention_config(
    app: tauri::AppHandle,
    config: borg_core::config::RetentionConfig,
) -> Result<(), String> {
    config.validate().map_err(|e| e.to_string())?;
    let mut data = read_profiles(&app).await?;
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
    profile.retention = Some(config);
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn prune_repo(
    state: State<'_, AppState>,
    repo: RepoConfig,
    retention: borg_core::config::RetentionConfig,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    retention.validate().map_err(|e| e.to_string())?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .prune(&repo, &retention, pass.as_deref())
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
        .map_err(|e| e.to_string())?;

    if let Some(pass) = passphrase.as_deref() {
        keychain::set_passphrase(&repo.ssh_url(), pass)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_archive(
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .delete_archive(&repo, &archive_name, pass.as_deref())
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
    excludes: Option<Vec<String>>,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    let compression = borg_core::config::Compression::default();
    compression.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    borg_core::config::validate_source_paths(&source_paths).map_err(|e| e.to_string())?;
    let excludes = excludes.unwrap_or_default();
    borg_core::config::validate_exclude_patterns(&excludes).map_err(|e| e.to_string())?;

    let raw_paths: Vec<PathBuf> = source_paths.into_iter().map(PathBuf::from).collect();
    let (backup_paths, snapshots) = borg_platform_win::vss::snapshot_sources(&raw_paths).await;

    let pass = lookup_passphrase(&repo);

    let profile = borg_core::config::BackupProfile {
        name: "manual".into(),
        source_paths: backup_paths,
        excludes,
        compression,
        repo,
    };

    let result = state
        .borg
        .create(&profile, &archive_name, pass.as_deref(), move |event| {
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

    let pass = lookup_passphrase(&repo);
    state
        .borg
        .extract(
            &repo,
            &archive_name,
            &dest_path,
            pass.as_deref(),
            move |event| {
                let _ = app.emit("restore-progress", &event);
            },
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_schedule_config(
    app: tauri::AppHandle,
) -> Result<Option<borg_platform_win::scheduler::ScheduleConfig>, String> {
    let data = read_profiles(&app).await?;
    Ok(data.active().and_then(|p| p.schedule.clone()))
}

#[tauri::command]
pub async fn save_schedule_config(
    app: tauri::AppHandle,
    config: borg_platform_win::scheduler::ScheduleConfig,
) -> Result<(), String> {
    config.schedule.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_source_paths(&config.source_paths).map_err(|e| e.to_string())?;
    borg_core::config::validate_exclude_patterns(&config.excludes).map_err(|e| e.to_string())?;

    let mut data = read_profiles(&app).await?;
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
    profile.schedule = Some(config.clone());
    write_profiles(&app, &data).await?;

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
pub async fn set_repo_passphrase(repo: RepoConfig, passphrase: String) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    if passphrase.is_empty() {
        return Err("passphrase cannot be empty".into());
    }
    keychain::set_passphrase(&repo.ssh_url(), &passphrase)
}

#[tauri::command]
pub async fn clear_repo_passphrase(repo: RepoConfig) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    keychain::clear_passphrase(&repo.ssh_url())
}

#[tauri::command]
pub async fn has_repo_passphrase(repo: RepoConfig) -> Result<bool, String> {
    repo.validate().map_err(|e| e.to_string())?;
    keychain::has_passphrase(&repo.ssh_url())
}

#[tauri::command]
pub async fn record_backup_event(app: tauri::AppHandle, event: BackupEvent) -> Result<(), String> {
    let path = history_path(&app)?;
    history::append(&path, event).await
}

#[tauri::command]
pub async fn load_backup_history(app: tauri::AppHandle) -> Result<Vec<BackupEvent>, String> {
    let path = history_path(&app)?;
    history::load(&path).await
}

#[tauri::command]
pub async fn clear_backup_history(app: tauri::AppHandle) -> Result<(), String> {
    let path = history_path(&app)?;
    history::clear(&path).await
}

fn history_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("history.json"))
}

#[tauri::command]
pub async fn load_repo_config(app: tauri::AppHandle) -> Result<Option<RepoConfig>, String> {
    let data = read_profiles(&app).await?;
    Ok(data.active().map(|p| p.repo.clone()))
}

#[tauri::command]
pub async fn save_repo_config(app: tauri::AppHandle, repo: RepoConfig) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    let mut data = read_profiles(&app).await?;
    if let Some(profile) = data.active_mut() {
        profile.repo = repo;
    } else {
        let profile = Profile {
            id: "default".into(),
            name: "Default".into(),
            repo,
            schedule: None,
            retention: None,
            archive_template: None,
        };
        data.active_id = Some(profile.id.clone());
        data.profiles.push(profile);
    }
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn list_profiles(app: tauri::AppHandle) -> Result<ProfilesData, String> {
    read_profiles(&app).await
}

#[tauri::command]
pub async fn set_active_profile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    data.set_active(&id)?;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn create_profile(
    app: tauri::AppHandle,
    name: String,
    repo: RepoConfig,
) -> Result<Profile, String> {
    repo.validate().map_err(|e| e.to_string())?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("profile name cannot be empty".into());
    }

    let mut data = read_profiles(&app).await?;
    let id = profiles::make_profile_id(&name, &data);
    let profile = Profile {
        id: id.clone(),
        name,
        repo,
        schedule: None,
        retention: None,
        archive_template: None,
    };
    data.profiles.push(profile.clone());
    if data.active_id.is_none() {
        data.active_id = Some(id);
    }
    write_profiles(&app, &data).await?;
    Ok(profile)
}

#[tauri::command]
pub async fn rename_profile(app: tauri::AppHandle, id: String, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("profile name cannot be empty".into());
    }
    let mut data = read_profiles(&app).await?;
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("profile not found: {}", id))?;
    profile.name = name;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn set_profile_template(
    app: tauri::AppHandle,
    id: String,
    template: Option<String>,
) -> Result<(), String> {
    let template = template.and_then(|t| {
        let trimmed = t.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    let mut data = read_profiles(&app).await?;
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("profile not found: {}", id))?;
    profile.archive_template = template;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn preview_archive_name(
    app: tauri::AppHandle,
    template: String,
) -> Result<String, String> {
    let template = if template.trim().is_empty() {
        archive_naming::DEFAULT_TEMPLATE.to_string()
    } else {
        template
    };
    let data = read_profiles(&app).await?;
    let profile_name = data.active().map(|p| p.name.as_str()).unwrap_or("default");
    let hostname = archive_naming::current_hostname();
    let random = archive_naming::random_suffix();
    let ctx = TemplateContext {
        now: chrono::Utc::now(),
        hostname: &hostname,
        profile: profile_name,
        random: &random,
    };
    let expanded = archive_naming::expand(&template, &ctx);
    borg_core::config::validate_archive_name(&expanded).map_err(|e| e.to_string())?;
    Ok(expanded)
}

#[tauri::command]
pub async fn delete_profile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    data.remove(&id)?;
    write_profiles(&app, &data).await
}
