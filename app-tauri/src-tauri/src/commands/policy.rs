//! Profile templates, resource/placeholder policy, and snooze.

use super::*;

#[tauri::command]
pub fn list_profile_templates() -> Vec<crate::templates::ResolvedTemplate> {
    crate::templates::list()
}

#[tauri::command]
pub async fn apply_profile_template(
    app: tauri::AppHandle,
    template_id: String,
    reviewed: bool,
) -> Result<profiles::BackupSelection, String> {
    if !reviewed {
        return Err("review the template sources and exclusions before applying".into());
    }
    let selection = crate::templates::apply(&template_id)?;
    borg_core::config::validate_source_paths(&selection.source_paths).map_err(|e| e.to_string())?;
    borg_core::config::validate_exclude_patterns(&selection.excludes).map_err(|e| e.to_string())?;
    let mut data = read_profiles(&app).await?;
    data.require_active_mut()?.backup_selection = selection.clone();
    write_profiles(&app, &data).await?;
    Ok(selection)
}

#[tauri::command]
pub async fn detach_profile_template(app: tauri::AppHandle) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    let selection = &mut data.require_active_mut()?.backup_selection;
    selection.template_id = None;
    selection.template_version = None;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn load_resource_policy(
    app: tauri::AppHandle,
) -> Result<profiles::ResourcePolicy, String> {
    Ok(read_profiles(&app)
        .await?
        .require_active()?
        .resource_policy
        .clone())
}

#[tauri::command]
pub async fn load_placeholder_policy(
    app: tauri::AppHandle,
) -> Result<profiles::PlaceholderPolicy, String> {
    Ok(read_profiles(&app)
        .await?
        .require_active()?
        .placeholder_policy
        .clone())
}

#[tauri::command]
pub async fn save_placeholder_policy(
    app: tauri::AppHandle,
    policy: profiles::PlaceholderPolicy,
) -> Result<(), String> {
    if matches!(policy.mode, profiles::PlaceholderMode::Materialize)
        && policy.minimum_free_space_reserve == 0
    {
        return Err("materialization requires a non-zero free-space reserve".into());
    }
    let mut data = read_profiles(&app).await?;
    data.require_active_mut()?.placeholder_policy = policy;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn save_resource_policy(
    app: tauri::AppHandle,
    policy: profiles::ResourcePolicy,
    autostart_consent: bool,
) -> Result<(), String> {
    if policy.upload_limit_kib == Some(0) {
        return Err("upload limit must be greater than zero or left unlimited".into());
    }
    if policy
        .allowed_wifi_names
        .iter()
        .any(|name| name.trim().is_empty())
    {
        return Err("allowed Wi-Fi names cannot be empty".into());
    }
    if policy.removable_destination_trigger
        && !borg_platform_win::autostart::is_enabled(borg_platform_win::autostart::AUTOSTART_VALUE)
            .await
    {
        if !autostart_consent {
            return Err(
                "removable destination triggers require consent to start BorgUI at login".into(),
            );
        }
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        borg_platform_win::autostart::enable(
            borg_platform_win::autostart::AUTOSTART_VALUE,
            &exe.to_string_lossy(),
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    let mut data = read_profiles(&app).await?;
    let profile = data.require_active_mut()?;
    profile.resource_policy = policy.clone();
    let schedule = profile.schedule.clone();
    write_profiles(&app, &data).await?;
    if let Some(schedule) = schedule.filter(|schedule| schedule.enabled) {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        borg_platform_win::scheduler::schedule_backup(
            "BorgUI-Backup",
            &exe.to_string_lossy(),
            "--scheduled-backup",
            &schedule.schedule,
            policy.wake_for_backup,
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_global_snooze(
    app: tauri::AppHandle,
    choice: String,
) -> Result<crate::snooze::SnoozeState, String> {
    crate::snooze::save(&config_dir(&app).await?, &choice).await
}

#[tauri::command]
pub async fn get_global_snooze(
    app: tauri::AppHandle,
) -> Result<Option<crate::snooze::SnoozeState>, String> {
    crate::snooze::load(&config_dir(&app).await?).await
}
