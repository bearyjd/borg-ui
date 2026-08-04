//! Repository integrity checks and their history.

use super::*;

#[tauri::command]
pub async fn check_repository(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    verify_data: bool,
) -> Result<IntegrityEvent, String> {
    let data = read_profiles(&app).await?;
    let profile = data.require_active()?.clone();
    precheck_repo(&profile.repo).await?;

    let mode = if verify_data {
        CheckMode::VerifyData
    } else {
        CheckMode::Repository
    };
    let started = std::time::Instant::now();
    let cancel = state.try_register_cancel(CHECK_OP, "an integrity check is already running")?;
    let pass = lookup_passphrase(&profile.repo);
    let progress_app = app.clone();
    let result = state
        .borg
        .check(
            &profile.repo,
            mode,
            pass.as_deref(),
            &cancel,
            move |event| {
                let _ = progress_app.emit("integrity-check-progress", &event);
            },
        )
        .await;
    state.unregister_cancel(CHECK_OP);

    let cancelled = matches!(result, Err(borg_core::error::BorgError::Cancelled));
    let warnings = result
        .as_ref()
        .ok()
        .map(|outcome| outcome.warnings.clone())
        .unwrap_or_default();
    let error_message = result
        .as_ref()
        .err()
        .map(|error| error.detail())
        .or_else(|| (!warnings.is_empty()).then(|| warnings.join("\n")));
    let event = IntegrityEvent {
        id: chrono::Utc::now().timestamp_millis().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        profile_id: profile.id.clone(),
        mode: if verify_data {
            "verify_data".into()
        } else {
            "repository".into()
        },
        outcome: if result.is_ok() && warnings.is_empty() {
            "success".into()
        } else if cancelled {
            "cancelled".into()
        } else {
            "failure".into()
        },
        duration_seconds: started.elapsed().as_secs(),
        error_message,
    };
    let dir = config_dir(&app).await?;
    history::append_integrity(&dir, event.clone()).await?;
    result.map_err(|error| error.detail())?;
    Ok(event)
}

#[tauri::command]
pub async fn cancel_repository_check(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.signal_cancel(CHECK_OP))
}

#[tauri::command]
pub async fn latest_integrity_check(
    app: tauri::AppHandle,
) -> Result<Option<IntegrityEvent>, String> {
    let data = read_profiles(&app).await?;
    let Some(profile) = data.active() else {
        return Ok(None);
    };
    let dir = config_dir(&app).await?;
    history::latest_integrity(&dir, &profile.id).await
}

#[tauri::command]
pub async fn set_monthly_integrity_check(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    const TASK: &str = "BorgUI-Integrity-Check";
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        borg_platform_win::scheduler::schedule_monthly_check(
            TASK,
            &exe.to_string_lossy(),
            "--scheduled-integrity-check",
        )
        .await
        .map_err(|e| e.to_string())?;
    } else {
        borg_platform_win::scheduler::unschedule_backup(TASK)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut data = read_profiles(&app).await?;
    let profile = data.require_active_mut()?;
    profile.integrity_schedule = Some(crate::profiles::IntegritySchedule { enabled });
    write_profiles(&app, &data).await
}
