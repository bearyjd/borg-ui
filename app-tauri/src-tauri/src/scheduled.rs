//! Headless scheduled-backup runner.
//!
//! When the Windows Task Scheduler entry fires, it launches the app with
//! `--scheduled-backup` (see [`crate::commands::save_schedule_config`]). `lib.rs`
//! detects that flag and calls [`run_scheduled_backup`] instead of showing the
//! GUI: it performs one backup from the active profile's *schedule*
//! configuration, prunes per the retention policy, records the outcome to
//! history, and reports back so the caller can notify the user and pick an exit
//! code.
//!
//! This module is deliberately free of Tauri types so it can be tested against a
//! real borg binary with a temporary config directory (see the tests, gated on
//! `BORG_TEST_BIN`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;

use borg_core::borg::{BorgClient, CancelToken};
use borg_core::config::{BackupProfile, Compression, RepoConfig};
use borg_core::hooks::HookContext;

use crate::archive_naming::{self, TemplateContext};
use crate::history::{self, BackupEvent};
use crate::keychain;
use crate::profiles::{self, Profile};

/// Outcome of a headless scheduled run. Drives the process exit code and the
/// notification shown to the user.
pub struct RunReport {
    /// The archive that was (attempted to be) created, once one was named.
    pub archive_name: Option<String>,
    /// Non-fatal warnings (skipped files, prune/post-hook issues).
    pub warnings: Vec<String>,
    /// Set when the run failed; `None` means success.
    pub error: Option<String>,
}

impl RunReport {
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
    }

    /// A failure before borg ran (misconfiguration). No history is recorded for
    /// these — there is no archive to key an event on; the notification surfaces
    /// the reason.
    fn preflight(error: String) -> Self {
        Self {
            archive_name: None,
            warnings: Vec::new(),
            error: Some(error),
        }
    }
}

fn lookup_passphrase(repo: &RepoConfig) -> Option<String> {
    // Mirror the GUI: a keychain miss or backend error means "no passphrase".
    keychain::get_passphrase(&repo.ssh_url()).ok().flatten()
}

fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn history_path(config_dir: &Path) -> PathBuf {
    config_dir.join("history.json")
}

fn build_archive_name(profile: &Profile) -> String {
    let template = profile
        .archive_template
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(archive_naming::DEFAULT_TEMPLATE);
    let hostname = archive_naming::current_hostname();
    let random = archive_naming::random_suffix();
    let ctx = TemplateContext {
        now: Utc::now(),
        hostname: &hostname,
        profile: &profile.name,
        random: &random,
    };
    archive_naming::expand(template, &ctx)
}

async fn load_active_profile(config_dir: &Path) -> Result<Profile, String> {
    let data = profiles::load(config_dir).await?;
    data.active()
        .cloned()
        .ok_or_else(|| "no active profile configured".to_string())
}

/// Record a backup history event and build the matching report.
async fn finish(
    config_dir: &Path,
    archive_name: &str,
    started: Instant,
    result: Result<Vec<String>, String>,
) -> RunReport {
    let duration_seconds = started.elapsed().as_secs();
    let (outcome, warnings, error) = match result {
        Ok(warnings) => ("success", warnings, None),
        Err(e) => ("failure", Vec::new(), Some(e)),
    };

    let event = BackupEvent {
        id: Utc::now().timestamp_millis().to_string(),
        timestamp: Utc::now().to_rfc3339(),
        kind: "backup".into(),
        archive_name: archive_name.to_string(),
        outcome: outcome.into(),
        duration_seconds,
        file_count: None,
        original_size: None,
        error_message: error.clone(),
    };
    // Best-effort: a history write failure must not change the backup outcome.
    let _ = history::append(&history_path(config_dir), event).await;

    RunReport {
        archive_name: Some(archive_name.to_string()),
        warnings,
        error,
    }
}

/// Run one backup from the active profile's schedule configuration. Never
/// panics; all failures are returned in the [`RunReport`].
pub async fn run_scheduled_backup(config_dir: &Path, borg: &BorgClient) -> RunReport {
    let started = Instant::now();

    let profile = match load_active_profile(config_dir).await {
        Ok(p) => p,
        Err(e) => return RunReport::preflight(e),
    };

    let Some(schedule) = profile.schedule.clone().filter(|s| s.enabled) else {
        return RunReport::preflight("active profile has no enabled schedule".into());
    };

    // Validate inputs the same way the manual backup path does.
    if let Err(e) = profile.repo.validate() {
        return RunReport::preflight(e.to_string());
    }
    if let Err(e) = borg_core::config::validate_source_paths(&schedule.source_paths) {
        return RunReport::preflight(e.to_string());
    }
    if let Err(e) = borg_core::config::validate_exclude_patterns(&schedule.excludes) {
        return RunReport::preflight(e.to_string());
    }

    let archive_name = build_archive_name(&profile);
    if let Err(e) = borg_core::config::validate_archive_name(&archive_name) {
        return RunReport::preflight(format!("invalid archive name '{archive_name}': {e}"));
    }

    let pass = lookup_passphrase(&profile.repo);
    let repo_url = profile.repo.location();
    let hook_ctx = HookContext {
        repo_url: &repo_url,
        archive_name: &archive_name,
    };

    // A failed pre-backup hook aborts before borg runs (don't archive stale data
    // when the prep step failed).
    if let Some(cmd) = nonempty(&profile.pre_backup)
        && let Err(e) = borg_core::hooks::run("pre-backup", cmd, &hook_ctx).await
    {
        return finish(config_dir, &archive_name, started, Err(e.detail())).await;
    }

    let raw_paths: Vec<PathBuf> = schedule.source_paths.iter().map(PathBuf::from).collect();

    // Scheduled (unattended) runs benefit from VSS most — files are likely in
    // use. Snapshot the source volume and back up from a junction mount so borg
    // stores clean, restorable paths; falls back to live files when VSS can't
    // run (multi-volume / non-admin / non-Windows). See commands.rs and
    // crates/borg-platform-win/src/vss.rs.
    let vss = borg_platform_win::vss::prepare_snapshot(&raw_paths).await;

    let backup_profile = BackupProfile {
        name: profile.name.clone(),
        source_paths: vss.source_paths.clone(),
        excludes: schedule.excludes.clone(),
        compression: Compression::default(),
        repo: profile.repo.clone(),
    };
    let cancel = CancelToken::new();
    let create_result = borg
        .create(
            &backup_profile,
            &archive_name,
            vss.cwd.as_deref(),
            pass.as_deref(),
            &cancel,
            |_| {},
        )
        .await;
    // Release the snapshot + junction regardless of how the backup ended.
    vss.release().await;
    let mut warnings = match create_result {
        Ok(outcome) => outcome.warnings,
        Err(e) => return finish(config_dir, &archive_name, started, Err(e.detail())).await,
    };

    // The backup succeeded; a failing post-backup hook is only a warning.
    if let Some(cmd) = nonempty(&profile.post_backup)
        && let Err(e) = borg_core::hooks::run("post-backup", cmd, &hook_ctx).await
    {
        warnings.push(format!("post-backup command failed: {}", e.detail()));
    }

    // Apply the retention policy, if any. Prune failures are warnings — the
    // backup itself is already safely stored.
    if let Some(retention) = profile.retention.clone()
        && retention.validate().is_ok()
    {
        match borg.prune(&profile.repo, &retention, pass.as_deref()).await {
            Ok(outcome) => warnings.extend(outcome.warnings),
            Err(e) => warnings.push(format!("prune failed: {}", e.detail())),
        }
    }

    finish(config_dir, &archive_name, started, Ok(warnings)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::ProfilesData;
    use borg_platform_win::scheduler::{Schedule, ScheduleConfig};
    use std::path::PathBuf;

    /// The borg binary to test against, or `None` to skip (mirrors the e2e
    /// suite's `BORG_TEST_BIN` gate).
    fn borg_or_skip() -> Option<BorgClient> {
        match std::env::var("BORG_TEST_BIN") {
            Ok(p) if !p.trim().is_empty() && PathBuf::from(&p).exists() => {
                Some(BorgClient::new(PathBuf::from(p)))
            }
            _ => {
                eprintln!("SKIP: set BORG_TEST_BIN to run the scheduled-backup runner tests");
                None
            }
        }
    }

    fn local_repo(path: &Path) -> RepoConfig {
        RepoConfig {
            ssh_host: String::new(),
            ssh_port: 0,
            ssh_user: String::new(),
            repo_path: path.to_string_lossy().into_owned(),
            ssh_key_path: None,
        }
    }

    fn profile_with_schedule(repo: RepoConfig, sources: Vec<String>, enabled: bool) -> Profile {
        Profile {
            id: "default".into(),
            name: "Scheduled".into(),
            repo,
            schedule: Some(ScheduleConfig {
                enabled,
                source_paths: sources,
                schedule: Schedule::Hourly,
                excludes: Vec::new(),
            }),
            retention: None,
            archive_template: None,
            pre_backup: None,
            post_backup: None,
        }
    }

    async fn write_profile(config_dir: &Path, profile: Profile) {
        let data = ProfilesData {
            schema_version: profiles::PROFILE_SCHEMA_VERSION,
            active_id: Some(profile.id.clone()),
            profiles: vec![profile],
        };
        profiles::save(config_dir, &data).await.unwrap();
    }

    #[tokio::test]
    async fn scheduled_run_creates_archive_and_records_history() {
        let Some(borg) = borg_or_skip() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("config");
        let repo_path = tmp.path().join("repo");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"scheduled data").unwrap();

        let repo = local_repo(&repo_path);
        borg.init_repo(&repo, "none", None).await.unwrap();
        write_profile(
            &config_dir,
            profile_with_schedule(repo.clone(), vec![src.to_string_lossy().into()], true),
        )
        .await;

        let report = run_scheduled_backup(&config_dir, &borg).await;
        assert!(
            report.succeeded(),
            "scheduled run should succeed; error: {:?}",
            report.error
        );

        // The archive really exists in the repo.
        let archives = borg.list_archives(&repo, None).await.unwrap();
        assert_eq!(archives.len(), 1);
        assert_eq!(Some(archives[0].name.clone()), report.archive_name);

        // A success event was written to history.
        let events = history::load(&history_path(&config_dir)).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, "success");
        assert_eq!(events[0].kind, "backup");
    }

    #[tokio::test]
    async fn disabled_schedule_is_a_preflight_failure() {
        let Some(borg) = borg_or_skip() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("config");
        let repo = local_repo(&tmp.path().join("repo"));
        write_profile(
            &config_dir,
            profile_with_schedule(repo, vec!["/some/path".into()], false),
        )
        .await;

        let report = run_scheduled_backup(&config_dir, &borg).await;
        assert!(!report.succeeded());
        assert!(report.error.as_deref().unwrap().contains("schedule"));
        // No archive named -> no history written.
        let events = history::load(&history_path(&config_dir)).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn missing_profile_is_a_preflight_failure() {
        let Some(borg) = borg_or_skip() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("empty-config");
        let report = run_scheduled_backup(&config_dir, &borg).await;
        assert!(!report.succeeded());
        assert!(
            report
                .error
                .as_deref()
                .unwrap()
                .contains("no active profile")
        );
    }
}
