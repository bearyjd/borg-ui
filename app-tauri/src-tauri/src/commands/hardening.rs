//! Append-only posture, hardening checklist, and protection health.

use super::*;

#[tauri::command]
pub async fn recovery_readiness(
    app: tauri::AppHandle,
) -> Result<crate::readiness::RecoveryReadiness, String> {
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?;
    let dir = config_dir(&app).await?;
    let key_export = history::latest_readiness_event(&dir, &profile.id, "key_export").await?;
    let key_import = history::latest_readiness_event(&dir, &profile.id, "key_import").await?;
    let rotation =
        history::latest_readiness_event(&dir, &profile.id, "passphrase_rotation").await?;
    let integrity = history::latest_integrity(&dir, &profile.id).await?;
    let drill = history::latest_restore_drill(&dir, &profile.id).await?;
    let passphrase_available = keychain::get_passphrase(&profile.repo.ssh_url())
        .ok()
        .flatten()
        .is_some();
    Ok(crate::readiness::evaluate(
        profile.recovery.encrypted_repository,
        passphrase_available,
        key_export.as_ref(),
        key_import.as_ref(),
        rotation.as_ref(),
        integrity.as_ref(),
        drill.as_ref(),
        chrono::Utc::now(),
    ))
}

#[tauri::command]
pub async fn generate_append_only_instructions(
    app: tauri::AppHandle,
) -> Result<crate::hardening::AuthorizedKeysInstructions, String> {
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?;
    if profile.repo.is_local() {
        return Err("append-only SSH instructions apply only to SSH repositories".into());
    }
    let key_path = profile
        .repo
        .ssh_key_path
        .as_deref()
        .ok_or_else(|| "configure or generate the backup SSH key first".to_string())?;
    let public_key = borg_core::ssh::read_public_key(key_path)
        .await
        .map_err(|error| error.to_string())?;
    crate::hardening::generate_authorized_keys_line(&public_key, &profile.repo.repo_path)
}

#[tauri::command]
pub async fn save_hardening_posture(
    app: tauri::AppHandle,
    posture: profiles::HardeningPosture,
) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    let profile = data.require_active_mut()?;
    if profile.repo.is_local() && (posture.append_only_declared || posture.restricted_ssh_declared)
    {
        return Err("SSH hardening declarations do not apply to local repositories".into());
    }
    profile.hardening = posture;
    write_profiles(&app, &data).await
}

#[derive(Debug, Serialize)]
pub struct HardeningCheck {
    pub id: &'static str,
    pub label: &'static str,
    pub complete: bool,
}

#[tauri::command]
pub async fn hardening_checklist(app: tauri::AppHandle) -> Result<Vec<HardeningCheck>, String> {
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?;
    let dir = config_dir(&app).await?;
    let integrity = history::latest_integrity(&dir, &profile.id).await?;
    let drill = history::latest_restore_drill(&dir, &profile.id).await?;
    Ok(vec![
        HardeningCheck {
            id: "encryption",
            label: "Repository encryption declared",
            complete: profile.hardening.encrypted_repository_declared,
        },
        HardeningCheck {
            id: "recovery_key",
            label: "Encrypted recovery key exported",
            complete: profile.hardening.recovery_key_exported,
        },
        HardeningCheck {
            id: "restricted_ssh",
            label: "Restricted append-only SSH access declared",
            complete: profile.repo.is_local()
                || (profile.hardening.restricted_ssh_declared
                    && profile.hardening.append_only_declared),
        },
        HardeningCheck {
            id: "integrity",
            label: "Latest integrity check succeeded",
            complete: integrity.is_some_and(|event| event.outcome == "success"),
        },
        HardeningCheck {
            id: "restore_drill",
            label: "Latest restore drill succeeded",
            complete: drill.is_some_and(|event| event.outcome == "success"),
        },
        HardeningCheck {
            id: "server_maintenance",
            label: "Server maintenance and recovery documented",
            complete: profile.repo.is_local() || profile.hardening.server_maintenance_documented,
        },
    ])
}

#[tauri::command]
pub async fn protection_health(
    app: tauri::AppHandle,
) -> Result<crate::health::ProtectionHealth, String> {
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?.clone();
    let dir = config_dir(&app).await?;
    let events = history::load(&dir).await?;
    let scheduled = history::latest_scheduled_attempt(&dir, &profile.id).await?;
    let integrity = history::latest_integrity(&dir, &profile.id).await?;
    let drill = history::latest_restore_drill(&dir, &profile.id).await?;
    let primary_attempt = history::latest_destination_attempt(&dir, &profile.id, "primary").await?;
    let secondary_attempt =
        history::latest_destination_attempt(&dir, &profile.id, "secondary").await?;
    let unavailable_sources = tokio::task::spawn_blocking({
        let paths = profile.backup_selection.source_paths.clone();
        move || {
            paths
                .iter()
                .filter(|path| !PathBuf::from(path).is_dir())
                .count() as u32
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    let repository_reachable = if profile.repo.is_local() {
        PathBuf::from(&profile.repo.repo_path).is_dir()
    } else {
        precheck_repo(&profile.repo).await.is_ok()
    };
    let grace_seconds = match profile.schedule.as_ref().map(|schedule| &schedule.schedule) {
        Some(borg_platform_win::scheduler::Schedule::Hourly) => 90 * 60,
        Some(borg_platform_win::scheduler::Schedule::Daily { .. }) => 36 * 60 * 60,
        None => u64::MAX,
    };
    let missed = scheduled.as_ref().is_some_and(|attempt| {
        crate::scheduled::is_missed(&attempt.timestamp, grace_seconds, chrono::Utc::now())
    });
    Ok(crate::health::aggregate(crate::health::HealthInputs {
        profile: &profile,
        events: &events,
        scheduled: scheduled.as_ref(),
        missed,
        unavailable_sources,
        repository_reachable,
        integrity: integrity.as_ref(),
        drill: drill.as_ref(),
        primary_attempt: primary_attempt.as_ref(),
        secondary_attempt: secondary_attempt.as_ref(),
        passphrase_available: keychain::get_passphrase(&profile.repo.ssh_url())
            .ok()
            .flatten()
            .is_some(),
        now: chrono::Utc::now(),
    }))
}
