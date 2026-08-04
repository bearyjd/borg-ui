//! Scheduled-backup configuration, status, and autostart.

use super::*;

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
    let mut data = read_profiles(&app).await?;
    let profile = data.require_active_mut()?;
    profile.schedule = Some(config.clone());
    let wake_to_run = profile.resource_policy.wake_for_backup;
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
            wake_to_run,
        )
        .await
        .map_err(|e| e.to_string())?;
    } else {
        let _ = borg_platform_win::scheduler::unschedule_backup("BorgUI-Backup").await;
    }

    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ScheduledBackupStatus {
    pub last_attempt: Option<crate::history::ScheduledAttempt>,
    pub missed: bool,
    pub grace_seconds: u64,
    pub task_registered: bool,
}

#[tauri::command]
pub async fn scheduled_backup_status(
    app: tauri::AppHandle,
) -> Result<ScheduledBackupStatus, String> {
    let data = read_profiles(&app).await?;
    let Some(profile) = data.active() else {
        return Ok(ScheduledBackupStatus {
            last_attempt: None,
            missed: false,
            grace_seconds: 0,
            task_registered: false,
        });
    };
    let schedule = profile
        .schedule
        .as_ref()
        .filter(|schedule| schedule.enabled);
    let grace_seconds = match schedule.map(|schedule| &schedule.schedule) {
        Some(borg_platform_win::scheduler::Schedule::Hourly) => 90 * 60,
        Some(borg_platform_win::scheduler::Schedule::Daily { .. }) => 36 * 60 * 60,
        None => 0,
    };
    let dir = config_dir(&app).await?;
    let last_attempt = history::latest_scheduled_attempt(&dir, &profile.id).await?;
    let missed = last_attempt.as_ref().is_some_and(|attempt| {
        crate::scheduled::is_missed(&attempt.timestamp, grace_seconds, chrono::Utc::now())
    });
    let task_registered = if schedule.is_some() {
        borg_platform_win::scheduler::task_exists("BorgUI-Backup")
            .await
            .unwrap_or(false)
    } else {
        false
    };
    Ok(ScheduledBackupStatus {
        last_attempt,
        missed,
        grace_seconds,
        task_registered,
    })
}

/// Whether BorgUI is registered to start at login (reads the Windows `Run` key).
#[tauri::command]
pub async fn get_autostart() -> Result<bool, String> {
    Ok(
        borg_platform_win::autostart::is_enabled(borg_platform_win::autostart::AUTOSTART_VALUE)
            .await,
    )
}

/// Register or unregister BorgUI to start (minimized to the tray) at login.
#[tauri::command]
pub async fn set_autostart(enabled: bool) -> Result<(), String> {
    let value = borg_platform_win::autostart::AUTOSTART_VALUE;
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_str = exe.to_string_lossy().to_string();
        borg_platform_win::autostart::enable(value, &exe_str)
            .await
            .map_err(|e| e.to_string())
    } else {
        borg_platform_win::autostart::disable(value)
            .await
            .map_err(|e| e.to_string())
    }
}
