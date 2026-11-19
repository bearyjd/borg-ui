//! Scheduled-backup configuration, status, and autostart.

use super::*;

#[tauri::command]
pub async fn load_schedule_config(
    app: tauri::AppHandle,
) -> Result<Option<borg_platform_win::scheduler::ScheduleConfig>, String> {
    let data = read_profiles(&app).await?;
    Ok(data.active().and_then(|p| p.schedule.clone()))
}

const BACKUP_TASK: &str = "BorgUI-Backup";

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
                "--scheduled-backup",
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

const INTEGRITY_TASK: &str = "BorgUI-Integrity-Check";
const RESTORE_DRILL_TASK: &str = "BorgUI-Monthly-Restore-Drill";
const HEALTH_REPORT_TASK: &str = "BorgUI-Daily-Health-Report";

enum AuxKind {
    Monthly,
    DailyDigest,
}

struct AuxTask {
    name: &'static str,
    args: &'static str,
    kind: AuxKind,
}

enum AuxPlan {
    Register(AuxTask),
    Remove(&'static str),
}

fn aux_plan(enabled: bool, task: AuxTask) -> AuxPlan {
    if enabled {
        AuxPlan::Register(task)
    } else {
        AuxPlan::Remove(task.name)
    }
}

/// What the integrity, restore-drill and daily-digest tasks should look like
/// for the active profile. No active profile means all three are removed.
fn plan_aux_tasks(active: Option<&Profile>) -> Vec<AuxPlan> {
    let integrity = active
        .and_then(|p| p.integrity_schedule.as_ref())
        .is_some_and(|s| s.enabled);
    let drill = active
        .and_then(|p| p.restore_drill_schedule.as_ref())
        .is_some_and(|s| s.enabled);
    let digest = active.is_some_and(|p| p.reporting.enabled && p.reporting.daily_digest);
    vec![
        aux_plan(
            integrity,
            AuxTask {
                name: INTEGRITY_TASK,
                args: "--scheduled-integrity-check",
                kind: AuxKind::Monthly,
            },
        ),
        aux_plan(
            drill,
            AuxTask {
                name: RESTORE_DRILL_TASK,
                args: "--scheduled-restore-drill",
                kind: AuxKind::Monthly,
            },
        ),
        aux_plan(
            digest,
            AuxTask {
                name: HEALTH_REPORT_TASK,
                args: "--scheduled-health-report",
                kind: AuxKind::DailyDigest,
            },
        ),
    ]
}

async fn apply_aux_plan(plan: AuxPlan) -> Result<(), String> {
    match plan {
        AuxPlan::Register(task) => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe = exe.to_string_lossy();
            match task.kind {
                AuxKind::Monthly => {
                    borg_platform_win::scheduler::schedule_monthly_check(task.name, &exe, task.args)
                        .await
                }
                AuxKind::DailyDigest => {
                    borg_platform_win::scheduler::schedule_backup(
                        task.name,
                        &exe,
                        task.args,
                        &borg_platform_win::scheduler::Schedule::Daily { hour: 9, minute: 0 },
                        false,
                    )
                    .await
                }
            }
            .map_err(|e| format!("{}: {e}", task.name))
        }
        AuxPlan::Remove(name) => {
            if borg_platform_win::scheduler::task_exists(name)
                .await
                .unwrap_or(false)
            {
                borg_platform_win::scheduler::unschedule_backup(name)
                    .await
                    .map_err(|e| format!("{name}: {e}"))?;
            }
            Ok(())
        }
    }
}

/// Reconcile every machine-wide task (backup, integrity, restore drill, daily
/// digest) with the active profile. Every task is attempted; failures are
/// joined into one error.
pub(super) async fn reconcile_all_tasks(active: Option<&Profile>) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(e) = reconcile_backup_task(active).await {
        errors.push(e);
    }
    for plan in plan_aux_tasks(active) {
        if let Err(e) = apply_aux_plan(plan).await {
            errors.push(e);
        }
    }
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

    fn aux_plan(p: Option<&Profile>) -> Vec<(&'static str, bool)> {
        plan_aux_tasks(p)
            .into_iter()
            .map(|plan| match plan {
                AuxPlan::Register(task) => (task.name, true),
                AuxPlan::Remove(name) => (name, false),
            })
            .collect()
    }

    const AUX_NAMES: [&str; 3] = [INTEGRITY_TASK, RESTORE_DRILL_TASK, HEALTH_REPORT_TASK];

    #[test]
    fn aux_no_active_profile_removes_all() {
        let plan = aux_plan(None);
        assert_eq!(plan.len(), 3);
        for (name, registered) in plan {
            assert!(AUX_NAMES.contains(&name));
            assert!(!registered);
        }
    }

    #[test]
    fn aux_flags_off_removes_all() {
        let p = profile(serde_json::json!({
            "integrity_schedule": {"enabled": false},
            "restore_drill_schedule": {"enabled": false},
            "reporting": {"enabled": true, "daily_digest": false}
        }));
        assert!(aux_plan(Some(&p)).iter().all(|(_, on)| !on));
    }

    #[test]
    fn aux_flags_on_registers_each() {
        let p = profile(serde_json::json!({
            "integrity_schedule": {"enabled": true},
            "restore_drill_schedule": {"enabled": true},
            "reporting": {"enabled": true, "daily_digest": true}
        }));
        assert!(aux_plan(Some(&p)).iter().all(|(_, on)| *on));
    }

    #[test]
    fn aux_digest_requires_reporting_enabled() {
        let p = profile(serde_json::json!({
            "reporting": {"enabled": false, "daily_digest": true}
        }));
        assert!(aux_plan(Some(&p)).iter().all(|(_, on)| !on));
    }

    #[test]
    fn aux_switch_from_on_profile_to_off_profile_removes() {
        let a = profile(serde_json::json!({
            "integrity_schedule": {"enabled": true},
            "restore_drill_schedule": {"enabled": true}
        }));
        let b = profile(serde_json::json!({
            "integrity_schedule": {"enabled": false}
        }));
        assert!(aux_plan(Some(&a)).iter().any(|(_, on)| *on));
        assert!(aux_plan(Some(&b)).iter().all(|(_, on)| !on));
    }
}
