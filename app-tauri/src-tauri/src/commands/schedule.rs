//! Scheduled-backup configuration, status, and autostart.

use super::*;
use crate::task_contract::{BACKUP_TASK, SCHEDULED_BACKUP_FLAG};

#[tauri::command]
pub async fn load_schedule_config(
    app: tauri::AppHandle,
) -> Result<Option<borg_platform_win::scheduler::ScheduleConfig>, String> {
    let data = read_profiles(&app).await?;
    Ok(data.active().and_then(|p| p.schedule.clone()))
}

/// What the machine-wide backup task should look like for the active profile.
enum TaskPlan {
    Register {
        schedule: borg_platform_win::scheduler::Schedule,
        wake_to_run: bool,
    },
    Remove,
}

fn plan_backup_task(active: Option<&Profile>) -> TaskPlan {
    match active.and_then(|p| p.schedule.as_ref().filter(|s| s.enabled).map(|s| (p, s))) {
        Some((profile, config)) => TaskPlan::Register {
            schedule: config.schedule.clone(),
            wake_to_run: profile.resource_policy.wake_for_backup,
        },
        None => TaskPlan::Remove,
    }
}

/// Make the machine-wide Task Scheduler entry match the active profile's
/// schedule. The single path for every change to the active profile, its
/// schedule, or its wake policy, so the task never drifts from profile state.
pub(super) async fn reconcile_backup_task(active: Option<&Profile>) -> Result<(), String> {
    match plan_backup_task(active) {
        TaskPlan::Register {
            schedule,
            wake_to_run,
        } => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            borg_platform_win::scheduler::schedule_backup(
                BACKUP_TASK,
                &exe.to_string_lossy(),
                SCHEDULED_BACKUP_FLAG,
                &schedule,
                wake_to_run,
            )
            .await
            .map_err(|e| e.to_string())
        }
        TaskPlan::Remove => {
            if borg_platform_win::scheduler::task_exists(BACKUP_TASK)
                .await
                .unwrap_or(false)
            {
                borg_platform_win::scheduler::unschedule_backup(BACKUP_TASK)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }
}

fn integrity_task_wanted(active: Option<&Profile>) -> bool {
    active
        .and_then(|p| p.integrity_schedule.as_ref())
        .is_some_and(|s| s.enabled)
}

fn restore_drill_task_wanted(active: Option<&Profile>) -> bool {
    active
        .and_then(|p| p.restore_drill_schedule.as_ref())
        .is_some_and(|s| s.enabled)
}

fn report_task_wanted(active: Option<&Profile>) -> bool {
    active.is_some_and(|p| p.reporting.enabled && p.reporting.daily_digest)
}

async fn remove_task_if_present(task: &str) -> Result<(), String> {
    if borg_platform_win::scheduler::task_exists(task)
        .await
        .unwrap_or(false)
    {
        borg_platform_win::scheduler::unschedule_backup(task)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn reconcile_monthly_task(task: &str, flag: &str, wanted: bool) -> Result<(), String> {
    if !wanted {
        return remove_task_if_present(task).await;
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    borg_platform_win::scheduler::schedule_monthly_check(task, &exe.to_string_lossy(), flag)
        .await
        .map_err(|e| e.to_string())
}

pub(super) async fn reconcile_integrity_task(active: Option<&Profile>) -> Result<(), String> {
    reconcile_monthly_task(
        crate::task_contract::INTEGRITY_TASK,
        crate::task_contract::SCHEDULED_INTEGRITY_FLAG,
        integrity_task_wanted(active),
    )
    .await
}

pub(super) async fn reconcile_restore_drill_task(active: Option<&Profile>) -> Result<(), String> {
    reconcile_monthly_task(
        crate::task_contract::RESTORE_DRILL_TASK,
        crate::task_contract::SCHEDULED_RESTORE_DRILL_FLAG,
        restore_drill_task_wanted(active),
    )
    .await
}

pub(super) async fn reconcile_report_task(active: Option<&Profile>) -> Result<(), String> {
    const TASK: &str = crate::task_contract::HEALTH_REPORT_TASK;
    if !report_task_wanted(active) {
        return remove_task_if_present(TASK).await;
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    borg_platform_win::scheduler::schedule_backup(
        TASK,
        &exe.to_string_lossy(),
        crate::task_contract::SCHEDULED_HEALTH_REPORT_FLAG,
        &borg_platform_win::scheduler::Schedule::Daily { hour: 9, minute: 0 },
        false,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Reconcile every machine-wide task against the active profile. Attempts all
/// of them even if one fails, and reports every failure.
pub(super) async fn reconcile_all_tasks(active: Option<&Profile>) -> Result<(), String> {
    let results = [
        reconcile_backup_task(active).await,
        reconcile_integrity_task(active).await,
        reconcile_restore_drill_task(active).await,
        reconcile_report_task(active).await,
    ];
    let errors: Vec<String> = results.into_iter().filter_map(Result::err).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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
    write_profiles(&app, &data).await?;

    reconcile_backup_task(data.active()).await?;
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
        borg_platform_win::scheduler::task_exists(BACKUP_TASK)
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

#[cfg(test)]
mod tests {
    use super::*;
    use borg_platform_win::scheduler::Schedule;

    fn profile(extra: serde_json::Value) -> Profile {
        let mut base = serde_json::json!({
            "id": "p",
            "name": "P",
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
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn no_active_profile_removes_task() {
        assert!(matches!(plan_backup_task(None), TaskPlan::Remove));
    }

    #[test]
    fn profile_without_schedule_removes_task() {
        assert!(matches!(
            plan_backup_task(Some(&profile(serde_json::json!({})))),
            TaskPlan::Remove
        ));
    }

    #[test]
    fn disabled_schedule_removes_task() {
        let p = profile(serde_json::json!({
            "schedule": {"enabled": false, "schedule": {"type": "hourly"}}
        }));
        assert!(matches!(plan_backup_task(Some(&p)), TaskPlan::Remove));
    }

    #[test]
    fn enabled_schedule_registers_task_with_wake_policy() {
        let p = profile(serde_json::json!({
            "schedule": {"enabled": true, "schedule": {"type": "daily", "hour": 3, "minute": 30}}
        }));
        match plan_backup_task(Some(&p)) {
            TaskPlan::Register { schedule, .. } => {
                assert!(matches!(
                    schedule,
                    Schedule::Daily {
                        hour: 3,
                        minute: 30
                    }
                ));
            }
            TaskPlan::Remove => panic!("expected Register"),
        }
    }

    #[test]
    fn auxiliary_tasks_absent_without_active_profile() {
        assert!(!integrity_task_wanted(None));
        assert!(!restore_drill_task_wanted(None));
        assert!(!report_task_wanted(None));
    }

    #[test]
    fn auxiliary_tasks_absent_by_default() {
        let p = profile(serde_json::json!({}));
        assert!(!integrity_task_wanted(Some(&p)));
        assert!(!restore_drill_task_wanted(Some(&p)));
        assert!(!report_task_wanted(Some(&p)));
    }

    #[test]
    fn auxiliary_tasks_follow_profile_settings() {
        let p = profile(serde_json::json!({
            "integrity_schedule": {"enabled": true},
            "restore_drill_schedule": {"enabled": true},
            "reporting": {"enabled": true, "daily_digest": true}
        }));
        assert!(integrity_task_wanted(Some(&p)));
        assert!(restore_drill_task_wanted(Some(&p)));
        assert!(report_task_wanted(Some(&p)));
        let off = profile(serde_json::json!({
            "integrity_schedule": {"enabled": false},
            "restore_drill_schedule": {"enabled": false},
            "reporting": {"enabled": true, "daily_digest": false}
        }));
        assert!(!integrity_task_wanted(Some(&off)));
        assert!(!restore_drill_task_wanted(Some(&off)));
        assert!(!report_task_wanted(Some(&off)));
    }
}
