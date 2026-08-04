//! Tauri command surface, split by domain.
//!
//! Every command is re-exported from here, so `commands::<name>` paths (and the
//! `generate_handler!` list in `lib.rs`) stay valid regardless of which module a
//! command lives in. This module itself holds only what the domain modules
//! share: the operation-registry keys, [`AppState`], and the profile/config
//! helpers they are all built on. Submodules reach those via `use super::*`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use borg_core::archive::ArchiveEntry;
use borg_core::borg::{ArchiveInfo, BorgClient, CancelToken, CheckMode, DiffEntry};
use borg_core::config::RepoConfig;
use borg_core::error::BorgError;
use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::archive_naming::{self, TemplateContext};
use crate::diagnostics::{self, ImportPreview};
use crate::history::{self, BackupEvent, IntegrityEvent};
use crate::keychain;
use crate::profiles::{self, Profile, ProfilesData};

mod archives;
mod backup;
mod hardening;
mod integrity;
mod passphrase;
mod policy;
mod profile_mgmt;
mod recovery_key;
mod repo;
mod reports;
mod restore;
mod retention;
mod schedule;
mod ssh;
mod support;

pub use archives::*;
pub use backup::*;
pub use hardening::*;
pub use integrity::*;
pub use passphrase::*;
pub use policy::*;
pub use profile_mgmt::*;
pub use recovery_key::*;
pub use repo::*;
pub use reports::*;
pub use restore::*;
pub use retention::*;
pub use schedule::*;
pub use ssh::*;
pub use support::*;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
