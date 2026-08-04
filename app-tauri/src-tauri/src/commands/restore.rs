//! Restore search, conflict preview, extraction, and restore drills.

use super::*;

#[derive(Debug, Serialize)]
pub struct RestoreSearchMatch {
    pub archive_name: String,
    pub archive_start: String,
    pub entry: ArchiveEntry,
}

#[derive(Debug, Serialize)]
pub struct RestoreSearchBatch {
    pub matches: Vec<RestoreSearchMatch>,
    pub archives_scanned: usize,
}

#[tauri::command]
pub async fn search_restore_files(
    state: State<'_, AppState>,
    repo: RepoConfig,
    query: String,
    request_id: String,
    on_batch: tauri::ipc::Channel<RestoreSearchBatch>,
) -> Result<usize, String> {
    precheck_repo(&repo).await?;
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err("restore search query cannot be empty".into());
    }
    if request_id.trim().is_empty() {
        return Err("restore search request id cannot be empty".into());
    }
    state.cancel_prefix(RESTORE_SEARCH_PREFIX);
    let op_key = format!("{RESTORE_SEARCH_PREFIX}{request_id}");
    let cancel =
        state.try_register_cancel(&op_key, "restore search request id is already active")?;
    let pass = lookup_passphrase(&repo);
    let archives = state
        .borg
        .list_archives(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let mut total = 0usize;
    for (archive_index, archive) in archives.into_iter().rev().enumerate() {
        if cancel.is_cancelled() {
            state.unregister_cancel(&op_key);
            return Err("operation cancelled".into());
        }
        let archive_name = archive.name.clone();
        let archive_start = archive.start.clone();
        let query = query.clone();
        let send_cancel = cancel.clone();
        let result = state
            .borg
            .list_contents_streaming(&repo, &archive.name, pass.as_deref(), &cancel, |entries| {
                let matches: Vec<_> = entries
                    .into_iter()
                    .filter(|entry| entry.path.to_lowercase().contains(&query))
                    .map(|entry| RestoreSearchMatch {
                        archive_name: archive_name.clone(),
                        archive_start: archive_start.clone(),
                        entry,
                    })
                    .collect();
                if !matches.is_empty()
                    && on_batch
                        .send(RestoreSearchBatch {
                            matches,
                            archives_scanned: archive_index + 1,
                        })
                        .is_err()
                {
                    send_cancel.cancel();
                }
            })
            .await;
        match result {
            Ok(_) => {}
            Err(error) => {
                state.unregister_cancel(&op_key);
                return Err(error.to_string());
            }
        }
        total += 1;
    }
    state.unregister_cancel(&op_key);
    Ok(total)
}

#[tauri::command]
pub fn cancel_restore_search(state: State<'_, AppState>) -> bool {
    state.cancel_prefix(RESTORE_SEARCH_PREFIX)
}

#[derive(Debug, Serialize)]
pub struct RestoreConflict {
    pub path: String,
    pub exists: bool,
}

#[tauri::command]
pub async fn preview_restore_conflicts(
    destination: String,
    paths: Vec<String>,
) -> Result<Vec<RestoreConflict>, String> {
    let destination = PathBuf::from(destination);
    if !destination.is_dir() {
        return Err("restore destination does not exist".into());
    }
    let canonical_destination = destination.canonicalize().map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|path| {
                borg_core::archive::validate_restore_path(&path)?;
                let relative = PathBuf::from(&path);
                Ok(RestoreConflict {
                    exists: canonical_destination.join(&relative).exists(),
                    path,
                })
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Cancel a running restore. Returns true if a restore was in progress.
#[tauri::command]
pub async fn cancel_restore(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.signal_cancel(RESTORE_OP))
}

#[tauri::command]
pub async fn set_monthly_restore_drill(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    const TASK: &str = "BorgUI-Monthly-Restore-Drill";
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        borg_platform_win::scheduler::schedule_monthly_check(
            TASK,
            &exe.to_string_lossy(),
            "--scheduled-restore-drill",
        )
        .await
        .map_err(|e| e.to_string())?;
    } else {
        borg_platform_win::scheduler::unschedule_backup(TASK)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut data = read_profiles(&app).await?;
    data.require_active_mut()?.restore_drill_schedule =
        Some(crate::profiles::RestoreDrillSchedule { enabled });
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn latest_restore_drill(
    app: tauri::AppHandle,
) -> Result<Option<history::RestoreDrillEvent>, String> {
    let data = read_profiles(&app).await?;
    let profile_id = data.require_active()?.id.clone();
    history::latest_restore_drill(&config_dir(&app).await?, &profile_id).await
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub warnings: Vec<String>,
    pub destination: String,
}

#[tauri::command]
pub async fn restore_archive(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
    destination: String,
    paths: Option<Vec<String>>,
    overwrite: bool,
) -> Result<RestoreResult, String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;

    let destination_root = PathBuf::from(&destination);
    if !destination_root.is_dir() {
        return Err(format!("destination does not exist: {}", destination));
    }
    let dest_path = if overwrite {
        destination_root
    } else {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H%M%S");
        let path = destination_root.join(format!("BorgUI Restore {timestamp}"));
        tokio::fs::create_dir(&path)
            .await
            .map_err(|e| format!("cannot create restore folder: {e}"))?;
        path
    };

    let paths = paths.unwrap_or_default();
    for p in &paths {
        borg_core::archive::validate_restore_path(p)?;
    }

    let pass = lookup_passphrase(&repo);
    let cancel = state.try_register_cancel(RESTORE_OP, "a restore is already running")?;
    let result = state
        .borg
        .extract(
            &repo,
            &archive_name,
            &dest_path,
            &paths,
            pass.as_deref(),
            &cancel,
            move |event| {
                let _ = app.emit("restore-progress", &event);
            },
        )
        .await;
    state.unregister_cancel(RESTORE_OP);

    result
        .map(|outcome| RestoreResult {
            warnings: outcome.warnings,
            destination: dest_path.to_string_lossy().into_owned(),
        })
        .map_err(|e| e.to_string())
}
