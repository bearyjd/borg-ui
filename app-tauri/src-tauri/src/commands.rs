use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use borg_core::archive::ArchiveEntry;
use borg_core::borg::{ArchiveInfo, BorgClient, CancelToken, CheckMode, DiffEntry};
use borg_core::config::RepoConfig;
use serde::Serialize;

/// Registry key for the single in-flight backup operation.
const BACKUP_OP: &str = "backup";
/// Registry key for the single in-flight restore operation.
const RESTORE_OP: &str = "restore";
const CHECK_OP: &str = "integrity-check";

/// Internal name for one-off backups invoked directly from the Backup page.
/// Borg ignores this field, but it shows up in tracing logs.
const MANUAL_PROFILE_NAME: &str = "manual";
use tauri::{Emitter, Manager, State};

use crate::archive_naming::{self, TemplateContext};
use crate::diagnostics::{self, ImportPreview};
use crate::history::{self, BackupEvent, IntegrityEvent};
use crate::keychain;
use crate::profiles::{self, Profile, ProfilesData};

fn lookup_passphrase(repo: &RepoConfig) -> Option<String> {
    keychain::get_passphrase(&repo.ssh_url()).ok().flatten()
}

/// Validate a repo and (on Windows) preflight its reachability before running
/// borg against it — surfacing both as user-facing errors. Use in every command
/// that runs borg against a repo (NOT profile/config CRUD, which must stay
/// savable even when the repo isn't reachable yet). The preflight does a loopback
/// SMB stat, so it runs off the async worker via `spawn_blocking`.
async fn precheck_repo(repo: &RepoConfig) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    let repo = repo.clone();
    tokio::task::spawn_blocking(move || repo.local_repo_preflight())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

async fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

async fn read_profiles(app: &tauri::AppHandle) -> Result<ProfilesData, String> {
    let dir = config_dir(app).await?;
    profiles::load(&dir).await
}

async fn write_profiles(app: &tauri::AppHandle, data: &ProfilesData) -> Result<(), String> {
    let dir = config_dir(app).await?;
    profiles::save(&dir, data).await
}

pub struct AppState {
    pub borg: BorgClient,
    /// Cancellation tokens for in-flight long-running operations, keyed by
    /// [`BACKUP_OP`] / [`RESTORE_OP`]. Used so the UI can stop a running
    /// backup or restore.
    cancels: Mutex<HashMap<String, CancelToken>>,
}

impl AppState {
    pub fn new(borg: BorgClient) -> Self {
        Self {
            borg,
            cancels: Mutex::new(HashMap::new()),
        }
    }

    /// Register a fresh cancel token for `key`. Fails with `busy_msg` if an
    /// operation is already registered under that key, so a second concurrent
    /// backup/restore can't orphan the first one's cancellation. The backend
    /// enforces this invariant rather than trusting the UI to gate it.
    fn try_register_cancel(&self, key: &str, busy_msg: &str) -> Result<CancelToken, String> {
        let mut map = self.cancels.lock().expect("cancel registry poisoned");
        if map.contains_key(key) {
            return Err(busy_msg.to_string());
        }
        let token = CancelToken::new();
        map.insert(key.to_string(), token.clone());
        Ok(token)
    }

    fn unregister_cancel(&self, key: &str) {
        self.cancels
            .lock()
            .expect("cancel registry poisoned")
            .remove(key);
    }

    /// Signal cancellation for `key`. Returns true if an operation was running.
    fn signal_cancel(&self, key: &str) -> bool {
        match self
            .cancels
            .lock()
            .expect("cancel registry poisoned")
            .get(key)
        {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

#[tauri::command]
pub async fn get_borg_version(state: State<'_, AppState>) -> Result<String, String> {
    state.borg.version().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_ssh_connection(
    host: String,
    port: u16,
    user: String,
    key_path: Option<String>,
) -> Result<(), String> {
    let key = key_path.map(PathBuf::from);
    borg_core::ssh::test_connection(&host, port, &user, key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Per-field pre-flight: can we reach the SSH server on this host:port?
#[tauri::command]
pub async fn check_host_reachable(host: String, port: u16) -> Result<(), String> {
    borg_core::ssh::check_reachable(&host, port)
        .await
        .map_err(|e| e.to_string())
}

/// Per-field pre-flight: validate the private-key file and return its public key.
#[tauri::command]
pub async fn validate_ssh_key(key_path: String) -> Result<String, String> {
    borg_core::ssh::validate_key(&PathBuf::from(key_path))
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct GeneratedSshKey {
    pub private_key_path: String,
    pub public_key: String,
}

/// Generate BorgUI's managed Ed25519 key without requiring Windows OpenSSH.
#[tauri::command]
pub async fn generate_ssh_key(
    app: tauri::AppHandle,
    overwrite: bool,
) -> Result<GeneratedSshKey, String> {
    let key_path = config_dir(&app)
        .await?
        .join("ssh")
        .join("id_ed25519_borgui");
    borg_core::ssh::generate_key(&key_path, overwrite)
        .await
        .map_err(|e| e.to_string())?;
    let public_key = borg_core::ssh::read_public_key(&key_path)
        .await
        .map_err(|e| e.to_string())?
        .trim()
        .to_string();
    Ok(GeneratedSshKey {
        private_key_path: key_path.to_string_lossy().into_owned(),
        public_key,
    })
}

#[tauri::command]
pub async fn get_repo_info(
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<serde_json::Value, String> {
    precheck_repo(&repo).await?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .info(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

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
    on_batch: tauri::ipc::Channel<Vec<ArchiveEntry>>,
) -> Result<usize, String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .list_contents_streaming(&repo, &archive_name, pass.as_deref(), move |batch| {
            // A send failure means the frontend dropped the channel (browser
            // closed mid-load); borg keeps running to completion but the batch
            // is simply discarded.
            let _ = on_batch.send(batch);
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_retention_config(
    app: tauri::AppHandle,
) -> Result<Option<borg_core::config::RetentionConfig>, String> {
    let data = read_profiles(&app).await?;
    Ok(data.active().and_then(|p| p.retention.clone()))
}

#[tauri::command]
pub async fn save_retention_config(
    app: tauri::AppHandle,
    config: borg_core::config::RetentionConfig,
) -> Result<(), String> {
    config.validate().map_err(|e| e.to_string())?;
    let mut data = read_profiles(&app).await?;
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
    profile.retention = Some(config);
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn prune_repo(
    state: State<'_, AppState>,
    repo: RepoConfig,
    retention: borg_core::config::RetentionConfig,
) -> Result<Vec<String>, String> {
    precheck_repo(&repo).await?;
    retention.validate().map_err(|e| e.to_string())?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .prune(&repo, &retention, pass.as_deref())
        .await
        .map(|outcome| outcome.warnings)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn init_repo(
    state: State<'_, AppState>,
    repo: RepoConfig,
    encryption: String,
    passphrase: Option<String>,
) -> Result<(), String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_encryption_mode(&encryption).map_err(|e| e.to_string())?;

    let needs_pass = encryption != "none"
        && encryption != "authenticated"
        && encryption != "authenticated-blake2";
    if needs_pass && passphrase.as_deref().unwrap_or("").is_empty() {
        return Err("passphrase required for this encryption mode".into());
    }

    state
        .borg
        .init_repo(&repo, &encryption, passphrase.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    if let Some(pass) = passphrase.as_deref() {
        keychain::set_passphrase(&repo.ssh_url(), pass)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_archive(
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
) -> Result<(), String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .delete_archive(&repo, &archive_name, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn diff_archives(
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_a: String,
    archive_b: String,
) -> Result<Vec<DiffEntry>, String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_a).map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_b).map_err(|e| e.to_string())?;
    if archive_a == archive_b {
        return Err("choose two different archives to compare".into());
    }
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .diff_archives(&repo, &archive_a, &archive_b, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn compact_repo(state: State<'_, AppState>, repo: RepoConfig) -> Result<String, String> {
    precheck_repo(&repo).await?;
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .compact(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    source_paths: Vec<String>,
    archive_name: String,
    excludes: Option<Vec<String>>,
    pre_backup: Option<String>,
    post_backup: Option<String>,
) -> Result<Vec<String>, String> {
    precheck_repo(&repo).await?;
    let compression = borg_core::config::Compression::default();
    compression.validate().map_err(|e| e.to_string())?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;
    borg_core::config::validate_source_paths(&source_paths).map_err(|e| e.to_string())?;
    let excludes = excludes.unwrap_or_default();
    borg_core::config::validate_exclude_patterns(&excludes).map_err(|e| e.to_string())?;

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

    let raw_paths: Vec<PathBuf> = source_paths.into_iter().map(PathBuf::from).collect();

    // Register the cancel slot before taking a snapshot, so a concurrent backup
    // is rejected up front and never leaves a VSS snapshot/junction behind.
    let cancel = state.try_register_cancel(BACKUP_OP, "a backup is already running")?;

    // VSS (Windows, admin, single-volume): snapshot the source volume and back
    // up from a read-only junction mount so borg stores clean, restorable paths
    // and exclusively-locked files are still captured. Multi-volume, non-admin,
    // or any failure transparently falls back to live-file backup; no-op off
    // Windows. See crates/borg-platform-win/src/vss.rs.
    let vss = borg_platform_win::vss::prepare_snapshot(&raw_paths).await;

    let pass = lookup_passphrase(&repo);

    let profile = borg_core::config::BackupProfile {
        name: MANUAL_PROFILE_NAME.into(),
        source_paths: vss.source_paths.clone(),
        excludes,
        compression,
        repo,
    };

    let result = state
        .borg
        .create(
            &profile,
            &archive_name,
            vss.cwd.as_deref(),
            pass.as_deref(),
            &cancel,
            move |event| {
                let _ = app.emit("backup-progress", &event);
            },
        )
        .await;
    state.unregister_cancel(BACKUP_OP);
    // Release the snapshot + junction regardless of how the backup ended.
    vss.release().await;

    let mut warnings = result
        .map(|outcome| outcome.warnings)
        .map_err(|e| e.to_string())?;

    // The backup itself succeeded; a failing post-backup hook is reported as a
    // warning rather than turning the whole backup into a failure.
    if let Some(cmd) = post_backup.as_deref()
        && let Err(e) = borg_core::hooks::run("post-backup", cmd, &hook_ctx).await
    {
        warnings.push(format!("post-backup command failed: {e}"));
    }

    Ok(warnings)
}

/// Cancel a running backup. Returns true if a backup was in progress.
#[tauri::command]
pub async fn cancel_backup(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.signal_cancel(BACKUP_OP))
}

/// Cancel a running restore. Returns true if a restore was in progress.
#[tauri::command]
pub async fn cancel_restore(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.signal_cancel(RESTORE_OP))
}

#[tauri::command]
pub async fn check_repository(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    verify_data: bool,
) -> Result<IntegrityEvent, String> {
    let data = read_profiles(&app).await?;
    let profile = data
        .active()
        .cloned()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
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
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
    profile.integrity_schedule = Some(crate::profiles::IntegritySchedule { enabled });
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn restore_archive(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    archive_name: String,
    destination: String,
    paths: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    precheck_repo(&repo).await?;
    borg_core::config::validate_archive_name(&archive_name).map_err(|e| e.to_string())?;

    let dest_path = PathBuf::from(&destination);
    if !dest_path.is_dir() {
        return Err(format!("destination does not exist: {}", destination));
    }

    let paths = paths.unwrap_or_default();
    for p in &paths {
        if p.trim().is_empty() {
            return Err("restore path cannot be empty".into());
        }
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
        .map(|outcome| outcome.warnings)
        .map_err(|e| e.to_string())
}

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
    borg_core::config::validate_source_paths(&config.source_paths).map_err(|e| e.to_string())?;
    borg_core::config::validate_exclude_patterns(&config.excludes).map_err(|e| e.to_string())?;

    let mut data = read_profiles(&app).await?;
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
    profile.schedule = Some(config.clone());
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

#[tauri::command]
pub async fn set_repo_passphrase(repo: RepoConfig, passphrase: String) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    if passphrase.is_empty() {
        return Err("passphrase cannot be empty".into());
    }
    keychain::set_passphrase(&repo.ssh_url(), &passphrase)
}

#[tauri::command]
pub async fn clear_repo_passphrase(repo: RepoConfig) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    keychain::clear_passphrase(&repo.ssh_url())
}

#[tauri::command]
pub async fn has_repo_passphrase(repo: RepoConfig) -> Result<bool, String> {
    repo.validate().map_err(|e| e.to_string())?;
    keychain::has_passphrase(&repo.ssh_url())
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

#[tauri::command]
pub async fn load_repo_config(app: tauri::AppHandle) -> Result<Option<RepoConfig>, String> {
    let data = read_profiles(&app).await?;
    Ok(data.active().map(|p| p.repo.clone()))
}

#[tauri::command]
pub async fn save_repo_config(app: tauri::AppHandle, repo: RepoConfig) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    let mut data = read_profiles(&app).await?;
    if let Some(profile) = data.active_mut() {
        profile.repo = repo;
    } else {
        let profile = Profile {
            id: "default".into(),
            name: "Default".into(),
            repo,
            schedule: None,
            integrity_schedule: None,
            retention: None,
            archive_template: None,
            pre_backup: None,
            post_backup: None,
        };
        data.active_id = Some(profile.id.clone());
        data.profiles.push(profile);
    }
    write_profiles(&app, &data).await
}

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
        schedule: None,
        integrity_schedule: None,
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

#[tauri::command]
pub async fn import_profile(app: tauri::AppHandle, path: String) -> Result<Profile, String> {
    let json = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())?;
    let mut imported: Profile =
        serde_json::from_str(&json).map_err(|e| format!("invalid profile JSON: {}", e))?;
    imported.repo.validate().map_err(|e| e.to_string())?;
    let name = imported.name.trim().to_string();
    if name.is_empty() {
        return Err("imported profile has empty name".into());
    }
    imported.name = name;

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

#[tauri::command]
pub async fn open_log_folder(app: tauri::AppHandle) -> Result<(), String> {
    let path = app.path().app_log_dir().map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        let result = std::process::Command::new("explorer").arg(&path).spawn();
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("open").arg(&path).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let result = std::process::Command::new("xdg-open").arg(&path).spawn();
        result.map(|_| ()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_support_bundle(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let config_dir = config_dir(&app).await?;
    let log_dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    diagnostics::export_support_bundle(&config_dir, &log_dir, &PathBuf::from(path)).await
}

#[tauri::command]
pub async fn export_configuration(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let config_dir = config_dir(&app).await?;
    diagnostics::export_configuration(&config_dir, &PathBuf::from(path)).await
}

#[tauri::command]
pub async fn preview_configuration_import(
    app: tauri::AppHandle,
    path: String,
) -> Result<ImportPreview, String> {
    let config_dir = config_dir(&app).await?;
    diagnostics::preview_import(&config_dir, &PathBuf::from(path)).await
}

#[tauri::command]
pub async fn import_configuration(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let config_dir = config_dir(&app).await?;
    diagnostics::import_configuration(&config_dir, &PathBuf::from(path)).await
}

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
    let profile = data
        .active()
        .cloned()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
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
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn import_recovery_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    recovery_passphrase: String,
) -> Result<(), String> {
    let source = tokio::fs::read(path)
        .await
        .map_err(|error| error.to_string())?;
    let envelope = crate::recovery::parse(&source)?;
    let mut key = crate::recovery::decrypt(&envelope, recovery_passphrase)?;
    let data = read_profiles(&app).await?;
    let profile = data
        .active()
        .cloned()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
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
    result.map_err(|error| error.detail())
}
