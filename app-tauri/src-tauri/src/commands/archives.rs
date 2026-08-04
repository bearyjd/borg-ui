//! Listing archives and streaming their contents.

use super::*;

#[tauri::command]
pub async fn list_archives(
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<Vec<ArchiveInfo>, String> {
    precheck_repo(&repo).await?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .list_archives(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Stream an archive's contents to the frontend in batches over `on_batch`,
/// returning the total number of entries sent. Backs the archive browser:
/// batching keeps the IPC payload (and backend memory) bounded so a very large
/// archive — 100k+ entries — loads progressively instead of as one giant blob.
#[tauri::command]
pub async fn stream_archive_contents(
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
    request_id: String,
    on_batch: tauri::ipc::Channel<Vec<ArchiveEntry>>,
) -> Result<usize, String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    if request_id.trim().is_empty() {
        return Err("archive listing request id cannot be empty".into());
    }
    let op_key = format!("{ARCHIVE_LIST_PREFIX}{request_id}");
    let cancel = state.try_register_cancel(
        &op_key,
        "archive contents are already loading for this request",
    )?;
    let send_cancel = cancel.clone();
    let pass = lookup_passphrase(&repo);
    let result = state
        .borg
        .list_contents_streaming(
            &repo,
            &archive_name,
            pass.as_deref(),
            &cancel,
            move |batch| {
                // A send failure means the frontend dropped the channel (browser
                // closed mid-load). Cancel the borg process even if the explicit
                // cancel_archive_listing command is lost to a reload/close race.
                if on_batch.send(batch).is_err() {
                    send_cancel.cancel();
                }
            },
        )
        .await;
    state.unregister_cancel(&op_key);
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_archive_listing(
    state: State<'_, AppState>,
    request_id: String,
) -> Result<bool, String> {
    if request_id.trim().is_empty() {
        return Ok(false);
    }
    Ok(state.signal_cancel(&format!("{ARCHIVE_LIST_PREFIX}{request_id}")))
}
