//! Recovery-key export and import.

use super::*;

#[tauri::command]
pub async fn export_recovery_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    recovery_passphrase: String,
) -> Result<(), String> {
    if recovery_passphrase.is_empty() {
        return Err("recovery passphrase cannot be empty".into());
    }
    let destination = PathBuf::from(path);
    if destination.exists() {
        return Err("destination already exists; choose a new file name".into());
    }

    let data = read_profiles(&app).await?;
    let profile = data.require_active()?.clone();
    precheck_repo(&profile.repo).await?;
    let passphrase = lookup_passphrase(&profile.repo);
    let info = state
        .borg
        .info(&profile.repo, passphrase.as_deref())
        .await
        .map_err(|error| error.detail())?;
    let repository_id = info
        .pointer("/repository/id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Borg did not return a repository identifier".to_string())?
        .to_string();

    let dir = config_dir(&app).await?;
    let plain = crate::recovery::restrictive_temp(&dir)?;
    let plain_path = plain.path().to_path_buf();
    let export_result = state
        .borg
        .export_key(&profile.repo, &plain_path, passphrase.as_deref())
        .await;
    if let Err(error) = export_result {
        let _ = crate::recovery::secure_remove(plain);
        return Err(error.detail());
    }

    let read_result = tokio::fs::read(&plain_path).await;
    let cleanup = crate::recovery::secure_remove(plain);
    let mut key = read_result.map_err(|error| error.to_string())?;
    if let Err(error) = cleanup {
        use zeroize::Zeroize;
        key.zeroize();
        return Err(format!("could not securely remove temporary key: {error}"));
    }
    let envelope = crate::recovery::encrypt(key, repository_id, recovery_passphrase)?;
    let encoded = serde_json::to_vec_pretty(&envelope).map_err(|error| error.to_string())?;
    tokio::task::spawn_blocking(move || crate::recovery::write_exclusive(&destination, &encoded))
        .await
        .map_err(|error| error.to_string())??;
    history::append_readiness_event(
        &dir,
        history::ReadinessEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            profile_id: profile.id.clone(),
            kind: "key_export".into(),
            outcome: "success".into(),
        },
    )
    .await?;
    let mut data = read_profiles(&app).await?;
    // Deliberately NOT `require_active()`. This is a different failure from the
    // one that message describes: an active profile was present at the top of
    // this command, and the export has already succeeded — the key file is on
    // disk. Reaching here means the profile was deleted or switched while borg
    // was running, so only the "exported" bookkeeping flag is lost. Telling the
    // user to "configure repository first" would imply nothing happened, and
    // they would re-run an export that already completed.
    data.active_mut()
        .ok_or_else(|| "active profile disappeared".to_string())?
        .hardening
        .recovery_key_exported = true;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn import_recovery_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    recovery_passphrase: String,
) -> Result<String, String> {
    let source = tokio::fs::read(path)
        .await
        .map_err(|error| error.to_string())?;
    let envelope = crate::recovery::parse(&source)?;
    let mut key = crate::recovery::decrypt(&envelope, recovery_passphrase)?;
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?.clone();
    precheck_repo(&profile.repo).await?;

    let dir = config_dir(&app).await?;
    let mut plain = crate::recovery::restrictive_temp(&dir)?;
    use std::io::Write;
    use zeroize::Zeroize;
    if let Err(error) = plain.write_all(&key) {
        key.zeroize();
        let _ = crate::recovery::secure_remove(plain);
        return Err(error.to_string());
    }
    key.zeroize();
    if let Err(error) = plain.as_file_mut().sync_all() {
        let _ = crate::recovery::secure_remove(plain);
        return Err(error.to_string());
    }
    let plain_path = plain.path().to_path_buf();
    let repo_passphrase = lookup_passphrase(&profile.repo);
    let result = state
        .borg
        .import_key(&profile.repo, &plain_path, repo_passphrase.as_deref())
        .await;
    let cleanup = crate::recovery::secure_remove(plain);
    cleanup.map_err(|error| format!("could not securely remove temporary key: {error}"))?;
    result.map_err(|error| error.detail())?;

    // The imported key carries whatever passphrase it was exported under, so an
    // import can silently move the repository *backwards* past a later rotation.
    // Record it: readiness treats an import as proof the key on hand opens the
    // repo, exactly as an export does, so the key step stops reading "stale".
    if let Err(e) = record_key_import(&app, &profile).await {
        tracing::warn!("could not record recovery-key import for readiness: {e}");
    }

    // The dangerous half: if the import reverted the repository to an earlier
    // passphrase, the copy in Credential Manager no longer opens it — and the
    // user is mid-recovery, which is the worst moment to discover that by
    // watching backups fail. Say so plainly instead of reporting a bare success.
    let stored = match repo_passphrase.as_deref() {
        Some(passphrase) => check_passphrase(&state.borg, &profile.repo, passphrase).await,
        None => PassphraseCheck::Undetermined,
    };
    Ok(match stored {
        PassphraseCheck::Wrong => KEY_IMPORT_PASSPHRASE_STALE.to_string(),
        PassphraseCheck::Opens => {
            "Repository key imported and validated by Borg. The stored passphrase still opens it."
                .into()
        }
        PassphraseCheck::Undetermined => {
            "Repository key imported and validated by Borg. The stored passphrase could not be \
             checked — if backups start failing to unlock, the imported key may predate your \
             current passphrase."
                .into()
        }
    })
}

/// Emitted when a recovery-key import succeeded but left the stored passphrase
/// unable to open the repository. Not a failure — the import worked — so it is
/// returned as an `Ok` status the UI renders as a warning.
const KEY_IMPORT_PASSPHRASE_STALE: &str = "Repository key imported, but the saved passphrase no longer opens this repository. The \
     imported key was exported before the passphrase was last changed, so the repository now \
     expects the OLDER passphrase. Open Repository Passphrase, choose Change passphrase, tick \
     \"Only update the stored copy\", and enter the passphrase that was current when this key was \
     exported.";

async fn record_key_import(app: &tauri::AppHandle, profile: &Profile) -> Result<(), String> {
    let dir = config_dir(app).await?;
    history::append_readiness_event(
        &dir,
        history::ReadinessEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            profile_id: profile.id.clone(),
            kind: "key_import".into(),
            outcome: "success".into(),
        },
    )
    .await
}
