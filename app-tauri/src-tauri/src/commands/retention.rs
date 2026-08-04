//! Retention policy and pruning.

use super::*;

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
    let profile = data.require_active_mut()?;
    profile.retention = Some(config);
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn prune_repo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    retention: borg_core::config::RetentionConfig,
) -> Result<Vec<String>, String> {
    precheck_repo(&repo).await?;
    retention.validate().map_err(|e| e.to_string())?;
    // Pruning is always scoped to the owning profile's archives (shared
    // repositories hold other machines' backups), so resolve the profile the
    // UI is pruning for and refuse rather than fall back to an unscoped prune.
    let data = read_profiles(&app).await?;
    let active = data.require_active()?;
    let matches_active = repo.location() == active.repo.location()
        || active
            .secondary_repo
            .as_ref()
            .is_some_and(|secondary| repo.location() == secondary.location());
    if !matches_active {
        return Err("active profile changed; reload the settings page".into());
    }
    let pass = lookup_passphrase(&repo);
    crate::pruning::prune_scoped(
        &state.borg,
        &repo,
        &retention,
        pass.as_deref(),
        active.archive_template.as_deref(),
        &active.name,
    )
    .await
    .map(|outcome| outcome.warnings)
    .map_err(|e| e.to_string())
}
