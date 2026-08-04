//! Running backups, source discovery/selection, and backup history.

use super::*;

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
    pre_backup: Option<String>,
    post_backup: Option<String>,
) -> Result<LogicalBackupResult, String> {
    precheck_repo(&repo).await?;
    let compression = borg_core::config::Compression::default();
    compression.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    let data = read_profiles(&app).await?;
    let active = data.require_active()?;
    let mut selection = active.backup_selection.clone();
    let resource_policy = active.resource_policy.clone();
    let profile_id = active.id.clone();
    let profile_name = active.name.clone();
    let archive_template = active.archive_template.clone();
    let retention = active.retention.clone();
    if repo.location() != active.repo.location() {
        return Err("active primary repository changed; reload the backup page".into());
    }
    let mut destinations = vec![("primary", repo.clone())];
    if let Some(secondary) = active.secondary_repo.clone() {
        if secondary.location() == repo.location() {
            return Err("secondary repository must differ from primary".into());
        }
        secondary.validate().map_err(|error| error.to_string())?;
        destinations.push(("secondary", secondary));
    }
    borg_core::config::validate_source_paths(&selection.source_paths).map_err(|e| e.to_string())?;
    borg_core::config::validate_exclude_patterns(&selection.excludes).map_err(|e| e.to_string())?;

    // Pre/post-backup hooks run the user's own shell commands; `$repo_url` and
    // `$archive_name` expand to already-validated values.
    let repo_url = repo.location();
    let hook_ctx = borg_core::hooks::HookContext {
        repo_url: &repo_url,
        archive_name: &archive_name,
    };
    let trimmed = |c: Option<String>| c.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let pre_backup = trimmed(pre_backup);
    let post_backup = trimmed(post_backup);

    // A failed pre-backup hook aborts before borg runs: if the prep step (e.g. a
    // DB dump) failed, backing up stale/partial data would be worse than nothing.
    if let Some(cmd) = pre_backup.as_deref() {
        borg_core::hooks::run("pre-backup", cmd, &hook_ctx)
            .await
            .map_err(|e| e.to_string())?;
    }

    let raw_paths: Vec<PathBuf> = selection
        .source_paths
        .into_iter()
        .map(PathBuf::from)
        .collect();

    // Register the cancel slot before taking a snapshot, so a concurrent backup
    // is rejected up front and never leaves a VSS snapshot/junction behind.
    let cancel = state.try_register_cancel(BACKUP_OP, "a backup is already running")?;
    let _sleep_guard =
        borg_platform_win::resource::SleepGuard::acquire(resource_policy.prevent_sleep)?;
    let placeholder_plan =
        crate::placeholders::prepare(&raw_paths, &active.placeholder_policy, &cancel).await?;
    selection.excludes.extend(placeholder_plan.exclusions);

    // VSS (Windows, admin, single-volume): snapshot the source volume and back
    // up from a read-only junction mount so borg stores clean, restorable paths
    // and exclusively-locked files are still captured. Multi-volume, non-admin,
    // or any failure transparently falls back to live-file backup; no-op off
    // Windows. See crates/borg-platform-win/src/vss.rs.
    let vss = borg_platform_win::vss::prepare_snapshot(&raw_paths).await;

    let run_id = format!("manual-{}", chrono::Utc::now().timestamp_millis());
    let dir = config_dir(&app).await?;
    let mut attempts = Vec::new();
    let mut warnings = if placeholder_plan.count > 0 {
        vec![format!(
            "{} cloud placeholders were handled according to policy",
            placeholder_plan.count
        )]
    } else {
        Vec::new()
    };
    let mut cancelled = false;
    for (index, (destination_name, destination)) in destinations.iter().enumerate() {
        if cancel.is_cancelled() {
            cancelled = true;
            for (remaining_name, _) in destinations.iter().skip(index) {
                record_destination_attempt(&dir, &run_id, &profile_id, remaining_name, "skipped")
                    .await;
                attempts.push(DestinationAttemptResult {
                    destination: (*remaining_name).into(),
                    outcome: "skipped".into(),
                    warnings: Vec::new(),
                });
            }
            break;
        }
        if index > 0 && precheck_repo(destination).await.is_err() {
            record_destination_attempt(&dir, &run_id, &profile_id, destination_name, "failure")
                .await;
            attempts.push(DestinationAttemptResult {
                destination: (*destination_name).into(),
                outcome: "failure".into(),
                warnings: Vec::new(),
            });
            continue;
        }
        let pass = lookup_passphrase(destination);
        let backup_profile = borg_core::config::BackupProfile {
            name: MANUAL_PROFILE_NAME.into(),
            source_paths: vss.source_paths.clone(),
            excludes: selection.excludes.clone(),
            compression: compression.clone(),
            repo: destination.clone(),
            upload_limit_kib: resource_policy.upload_limit_kib,
        };
        let progress_app = app.clone();
        let metric_totals = Arc::new(Mutex::new(crate::forecast::MetricTotals::default()));
        let progress_metrics = Arc::clone(&metric_totals);
        let destination_started = std::time::Instant::now();
        let result = state
            .borg
            .create(
                &backup_profile,
                &archive_name,
                vss.cwd.as_deref(),
                pass.as_deref(),
                &cancel,
                move |event| {
                    if let Ok(mut totals) = progress_metrics.lock() {
                        totals.observe(&event);
                    }
                    let _ = progress_app.emit("backup-progress", &event);
                },
            )
            .await;
        match result {
            Ok(outcome) => {
                let totals = metric_totals.lock().map(|value| *value).unwrap_or_default();
                let metric = totals.into_metric(
                    profile_id.clone(),
                    (*destination_name).into(),
                    destination_started.elapsed().as_secs(),
                );
                let _ = history::append_repository_metric(&dir, metric).await;
                let mut destination_warnings = outcome.warnings;
                if let Some(retention) = &retention {
                    match crate::pruning::prune_scoped(
                        &state.borg,
                        destination,
                        retention,
                        pass.as_deref(),
                        archive_template.as_deref(),
                        &profile_name,
                    )
                    .await
                    {
                        Ok(outcome) => destination_warnings.extend(outcome.warnings),
                        Err(_) => destination_warnings
                            .push("retention failed for this destination".into()),
                    }
                }
                record_destination_attempt(&dir, &run_id, &profile_id, destination_name, "success")
                    .await;
                warnings.extend(destination_warnings.clone());
                attempts.push(DestinationAttemptResult {
                    destination: (*destination_name).into(),
                    outcome: "success".into(),
                    warnings: destination_warnings,
                });
            }
            Err(borg_core::error::BorgError::Cancelled) => {
                cancelled = true;
                record_destination_attempt(
                    &dir,
                    &run_id,
                    &profile_id,
                    destination_name,
                    "cancelled",
                )
                .await;
                attempts.push(DestinationAttemptResult {
                    destination: (*destination_name).into(),
                    outcome: "cancelled".into(),
                    warnings: Vec::new(),
                });
            }
            Err(_) => {
                record_destination_attempt(&dir, &run_id, &profile_id, destination_name, "failure")
                    .await;
                attempts.push(DestinationAttemptResult {
                    destination: (*destination_name).into(),
                    outcome: "failure".into(),
                    warnings: Vec::new(),
                });
            }
        }
    }
    state.unregister_cancel(BACKUP_OP);
    // Release the snapshot + junction regardless of how the backup ended.
    vss.release().await;

    if cancelled {
        return Err("operation cancelled".into());
    }
    let successes = attempts
        .iter()
        .filter(|attempt| attempt.outcome == "success")
        .count();
    let outcome = logical_backup_outcome(&attempts);

    // The backup itself succeeded; a failing post-backup hook is reported as a
    // warning rather than turning the whole backup into a failure.
    if successes > 0
        && let Some(cmd) = post_backup.as_deref()
        && let Err(e) = borg_core::hooks::run("post-backup", cmd, &hook_ctx).await
    {
        warnings.push(format!("post-backup command failed: {e}"));
    }

    Ok(LogicalBackupResult {
        outcome: outcome.into(),
        warnings,
        attempts,
    })
}

fn logical_backup_outcome(attempts: &[DestinationAttemptResult]) -> &'static str {
    let successes = attempts
        .iter()
        .filter(|attempt| attempt.outcome == "success")
        .count();
    if successes == attempts.len() {
        "success"
    } else if successes > 0 {
        "partial_success"
    } else {
        "failure"
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DestinationAttemptResult {
    pub destination: String,
    pub outcome: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogicalBackupResult {
    pub outcome: String,
    pub warnings: Vec<String>,
    pub attempts: Vec<DestinationAttemptResult>,
}

async fn record_destination_attempt(
    config_dir: &std::path::Path,
    run_id: &str,
    profile_id: &str,
    destination: &str,
    outcome: &str,
) {
    let _ = history::append_destination_attempt(
        config_dir,
        history::DestinationAttempt {
            run_id: run_id.into(),
            profile_id: profile_id.into(),
            destination: destination.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            outcome: outcome.into(),
            error_message: (outcome == "failure")
                .then(|| "destination backup failed; details omitted".into()),
        },
    )
    .await;
}

#[tauri::command]
pub async fn discover_backup_sources() -> Result<Vec<crate::coverage::KnownFolder>, String> {
    tokio::task::spawn_blocking(crate::coverage::discover_known_folders)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scan_backup_sources(
    state: State<'_, AppState>,
    source_paths: Vec<String>,
) -> Result<crate::coverage::CoverageScan, String> {
    borg_core::config::validate_source_paths(&source_paths).map_err(|e| e.to_string())?;
    let cancel =
        state.try_register_cancel(COVERAGE_SCAN_OP, "a coverage scan is already running")?;
    let scan_cancel = cancel.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::coverage::scan_sources(source_paths, &scan_cancel)
    })
    .await
    .map_err(|e| e.to_string());
    state.unregister_cancel(COVERAGE_SCAN_OP);
    result
}

#[tauri::command]
pub fn cancel_backup_source_scan(state: State<'_, AppState>) -> bool {
    state.signal_cancel(COVERAGE_SCAN_OP)
}

#[tauri::command]
pub async fn load_backup_selection(
    app: tauri::AppHandle,
) -> Result<profiles::BackupSelection, String> {
    let data = read_profiles(&app).await?;
    Ok(data.require_active()?.backup_selection.clone())
}

#[tauri::command]
pub async fn save_backup_selection(
    app: tauri::AppHandle,
    selection: profiles::BackupSelection,
    reviewed: bool,
) -> Result<(), String> {
    borg_core::config::validate_source_paths(&selection.source_paths).map_err(|e| e.to_string())?;
    borg_core::config::validate_exclude_patterns(&selection.excludes).map_err(|e| e.to_string())?;
    let scan = tokio::task::spawn_blocking({
        let paths = selection.source_paths.clone();
        move || crate::coverage::scan_sources(paths, &CancelToken::new())
    })
    .await
    .map_err(|e| e.to_string())?;
    if scan.needs_review && !reviewed {
        return Err(
            "coverage gaps or duplicate roots require explicit review before saving".into(),
        );
    }
    let mut data = read_profiles(&app).await?;
    data.require_active_mut()?.backup_selection = selection;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub fn standard_backup_excludes() -> Vec<&'static str> {
    crate::coverage::STANDARD_EXCLUDES.to_vec()
}

/// Cancel a running backup. Returns true if a backup was in progress.
#[tauri::command]
pub async fn cancel_backup(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.signal_cancel(BACKUP_OP))
}

#[tauri::command]
pub async fn record_backup_event(app: tauri::AppHandle, event: BackupEvent) -> Result<(), String> {
    let dir = config_dir(&app).await?;
    history::append(&dir, event).await
}

#[tauri::command]
pub async fn load_backup_history(app: tauri::AppHandle) -> Result<Vec<BackupEvent>, String> {
    let dir = config_dir(&app).await?;
    history::load(&dir).await
}

#[tauri::command]
pub async fn clear_backup_history(app: tauri::AppHandle) -> Result<(), String> {
    let dir = config_dir(&app).await?;
    history::clear(&dir).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_backup_result_distinguishes_partial_failure() {
        let attempt = |destination: &str, outcome: &str| DestinationAttemptResult {
            destination: destination.into(),
            outcome: outcome.into(),
            warnings: Vec::new(),
        };
        assert_eq!(
            logical_backup_outcome(&[
                attempt("primary", "success"),
                attempt("secondary", "failure")
            ]),
            "partial_success"
        );
        assert_eq!(
            logical_backup_outcome(&[
                attempt("primary", "failure"),
                attempt("secondary", "failure")
            ]),
            "failure"
        );
    }
}
