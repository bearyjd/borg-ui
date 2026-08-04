//! Repository lifecycle: version, info, creation, forecast, and config.

use super::*;

#[tauri::command]
pub async fn get_borg_version(state: State<'_, AppState>) -> Result<String, String> {
    state.borg.version().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_repo_info(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<serde_json::Value, String> {
    precheck_repo(&repo).await?;
    let pass = lookup_passphrase(&repo);
    let value = state
        .borg
        .info(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(stored_size) = value
        .pointer("/cache/stats/total_size")
        .or_else(|| value.pointer("/repository/stats/total_size"))
        .and_then(serde_json::Value::as_u64)
    {
        let data = read_profiles(&app).await?;
        if let Some(profile) = data.active() {
            let destination = if profile.repo.location() == repo.location() {
                Some("primary")
            } else if profile
                .secondary_repo
                .as_ref()
                .is_some_and(|secondary| secondary.location() == repo.location())
            {
                Some("secondary")
            } else {
                None
            };
            if let Some(destination) = destination {
                let dir = config_dir(&app).await?;
                let _ =
                    history::update_latest_stored_size(&dir, &profile.id, destination, stored_size)
                        .await;
            }
        }
    }
    Ok(value)
}

#[tauri::command]
pub async fn storage_forecast(
    app: tauri::AppHandle,
    destination: Option<String>,
) -> Result<crate::forecast::StorageForecast, String> {
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?;
    let destination = destination.unwrap_or_else(|| "primary".into());
    if destination != "primary" && destination != "secondary" {
        return Err("destination must be primary or secondary".into());
    }
    let repo = if destination == "secondary" {
        profile
            .secondary_repo
            .as_ref()
            .ok_or_else(|| "secondary destination is not configured".to_string())?
    } else {
        &profile.repo
    };
    let dir = config_dir(&app).await?;
    let metrics = history::repository_metrics(&dir, &profile.id, &destination).await?;
    let free_space = repo
        .is_local()
        .then(|| {
            let path = PathBuf::from(&repo.repo_path);
            borg_platform_win::cloud_files::free_space(&path).ok()
        })
        .flatten();
    Ok(crate::forecast::calculate(&metrics, free_space, None))
}

#[tauri::command]
pub async fn save_storage_warnings(
    app: tauri::AppHandle,
    thresholds: crate::profiles::StorageWarningThresholds,
) -> Result<(), String> {
    if thresholds.minimum_free_space_bytes == 0 || thresholds.capacity_warning_days == 0 {
        return Err("storage warning thresholds must be greater than zero".into());
    }
    let mut data = read_profiles(&app).await?;
    data.require_active_mut()?.storage_warnings = thresholds;
    write_profiles(&app, &data).await
}

/// Whether creating a repository in this mode requires a passphrase.
///
/// Only `none` is genuinely passphrase-free. The `authenticated` modes do not
/// encrypt the *data*, but they still have a key protected by a passphrase —
/// verified against borg 1.4.4: a repo created with an empty passphrase opens
/// ONLY with the empty one, and `key change-passphrase` works there. Treating
/// them as passphrase-free created repositories with a silently empty
/// passphrase, so the first "Set passphrase" stored something the repository
/// would never accept.
fn encryption_needs_passphrase(mode: &str) -> bool {
    mode != "none"
}

#[tauri::command]
pub async fn init_repo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    encryption: String,
    passphrase: Option<String>,
) -> Result<(), String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_encryption_mode(&encryption).map_err(|e| e.to_string())?;

    // Also drives `recovery.encrypted_repository` below: the readiness steps it
    // gates are "a passphrase is stored" and "the key was exported", both of
    // which apply to the `authenticated` modes even though those do not encrypt
    // the data — they still have a key, and it still has a passphrase.
    let needs_pass = encryption_needs_passphrase(&encryption);
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

    let mut data = read_profiles(&app).await?;
    if let Some(profile) = data
        .profiles
        .iter_mut()
        .find(|profile| profile.repo.location() == repo.location())
    {
        profile.recovery.encrypted_repository = needs_pass;
        let profile_id = profile.id.clone();
        write_profiles(&app, &data).await?;
        if needs_pass {
            let dir = config_dir(&app).await?;
            history::append_readiness_event(
                &dir,
                history::ReadinessEvent {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    profile_id,
                    kind: "passphrase".into(),
                    outcome: "success".into(),
                },
            )
            .await?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_archive(
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
) -> Result<(), String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .delete_archive(&repo, &archive_name, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn diff_archives(
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_a: String,
    archive_b: String,
) -> Result<Vec<DiffEntry>, String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_a).map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_b).map_err(|e| e.to_string())?;
    if archive_a == archive_b {
        return Err("choose two different archives to compare".into());
    }
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .diff_archives(&repo, &archive_a, &archive_b, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn compact_repo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<String, String> {
    precheck_repo(&repo).await?;
    if read_profiles(&app).await?.active().is_some_and(|profile| {
        profile.repo.location() == repo.location() && profile.hardening.append_only_declared
    }) {
        return Err(
            "compact is disabled for declared append-only backup access; run physical cleanup with trusted server-side maintenance credentials".into(),
        );
    }
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .compact(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
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
            secondary_repo: None,
            backup_selection: Default::default(),
            schedule: None,
            integrity_schedule: None,
            restore_drill_schedule: None,
            resource_policy: Default::default(),
            hardening: Default::default(),
            reporting: Default::default(),
            placeholder_policy: Default::default(),
            storage_warnings: Default::default(),
            recovery: Default::default(),
            retention: None,
            archive_template: None,
            pre_backup: None,
            post_backup: None,
        };
        data.active_id = Some(profile.id.clone());
        data.profiles.push(profile);
    }
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn save_secondary_repository(
    app: tauri::AppHandle,
    repo: Option<RepoConfig>,
    passphrase: Option<String>,
) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    let profile = data.require_active_mut()?;
    if let Some(repo) = &repo {
        repo.validate().map_err(|error| error.to_string())?;
        if repo.location() == profile.repo.location() {
            return Err("secondary repository must differ from primary".into());
        }
        if let Some(passphrase) = passphrase.as_deref().filter(|value| !value.is_empty()) {
            keychain::set_passphrase(&repo.ssh_url(), passphrase)?;
        }
    } else if let Some(previous) = &profile.secondary_repo {
        keychain::clear_passphrase(&previous.ssh_url())?;
    }
    profile.secondary_repo = repo;
    write_profiles(&app, &data).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verified against borg 1.4.4: `authenticated` repos created with an empty
    /// passphrase open only with the empty one, so every mode but `none` must
    /// demand a real passphrase at creation time.
    #[test]
    fn only_unencrypted_mode_skips_the_passphrase_requirement() {
        assert!(!encryption_needs_passphrase("none"));
        for mode in [
            "authenticated",
            "authenticated-blake2",
            "repokey",
            "keyfile",
            "repokey-blake2",
            "keyfile-blake2",
        ] {
            assert!(encryption_needs_passphrase(mode), "{mode}");
        }
    }
}
