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
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;

use borg_core::borg::{BorgClient, CancelToken, CheckMode};
use borg_core::config::{BackupProfile, Compression, RepoConfig};
use borg_core::hooks::HookContext;

use crate::archive_naming::{self, TemplateContext};
use crate::history::{self, BackupEvent};
use crate::keychain;
use crate::profiles::{self, Profile};

pub async fn run_restore_drill(config_dir: &Path, borg: &BorgClient) -> Result<(), String> {
    let started = Instant::now();
    let data = profiles::load(config_dir).await?;
    let profile = data
        .active()
        .ok_or_else(|| "no active profile".to_string())?;
    if !profile
        .restore_drill_schedule
        .as_ref()
        .is_some_and(|schedule| schedule.enabled)
    {
        return Err("restore drill is not enabled".into());
    }
    let event_id = format!("restore-drill-{}", Utc::now().timestamp_millis());
    let run = async {
        let passphrase = keychain::get_passphrase(&profile.repo.ssh_url())
            .ok()
            .flatten();
        let archive = borg
            .list_archives(&profile.repo, passphrase.as_deref())
            .await?
            .into_iter()
            .max_by(|left, right| left.start.cmp(&right.start))
            .ok_or_else(|| borg_core::error::BorgError::InvalidConfig {
                message: "repository has no archives to drill".into(),
            })?;
        let sample = Arc::new(Mutex::new(Vec::new()));
        let sink = sample.clone();
        let cancel = CancelToken::new();
        borg.list_contents_streaming(
            &profile.repo,
            &archive.name,
            passphrase.as_deref(),
            &cancel,
            move |entries| {
                let mut sample = sink.lock().expect("restore drill sample poisoned");
                let remaining = 10usize.saturating_sub(sample.len());
                sample.extend(
                    entries
                        .into_iter()
                        .filter(|entry| {
                            entry.entry_type == "f"
                                && borg_core::archive::validate_restore_path(&entry.path).is_ok()
                        })
                        .take(remaining),
                );
            },
        )
        .await?;
        let sample = std::mem::take(
            &mut *sample
                .lock()
                .expect("restore drill sample poisoned after listing"),
        );
        if sample.is_empty() {
            return Err(borg_core::error::BorgError::InvalidConfig {
                message: "latest archive contains no regular files".into(),
            });
        }
        let temp = tempfile::Builder::new()
            .prefix(".borgui-restore-drill-")
            .tempdir_in(config_dir)?;
        let paths: Vec<_> = sample.iter().map(|entry| entry.path.clone()).collect();
        borg.extract(
            &profile.repo,
            &archive.name,
            temp.path(),
            &paths,
            passphrase.as_deref(),
            &cancel,
            |_| {},
        )
        .await?;
        for entry in &sample {
            let restored = temp.path().join(&entry.path);
            let metadata = std::fs::metadata(restored).map_err(borg_core::error::BorgError::Io)?;
            if !metadata.is_file() || metadata.len() != entry.size {
                return Err(borg_core::error::BorgError::InvalidConfig {
                    message: "restored sample failed size/readability verification".into(),
                });
            }
            std::fs::File::open(temp.path().join(&entry.path))
                .map_err(borg_core::error::BorgError::Io)?;
        }
        Ok::<u8, borg_core::error::BorgError>(sample.len() as u8)
    }
    .await;
    let (outcome, files_checked, error_message) = match &run {
        Ok(count) => ("success", *count, None),
        Err(_) => (
            "failure",
            0,
            Some("restore drill failed; no filenames or temporary paths recorded".into()),
        ),
    };
    history::append_restore_drill(
        config_dir,
        history::RestoreDrillEvent {
            id: event_id,
            timestamp: Utc::now().to_rfc3339(),
            profile_id: profile.id.clone(),
            outcome: outcome.into(),
            files_checked,
            duration_seconds: started.elapsed().as_secs(),
            error_message,
        },
    )
    .await?;
    run.map(|_| ()).map_err(|error| error.to_string())
}

/// Outcome of a headless scheduled run. Drives the process exit code and the
/// notification shown to the user.
pub struct RunReport {
    /// The archive that was (attempted to be) created, once one was named.
    pub archive_name: Option<String>,
    /// Non-fatal warnings (skipped files, prune/post-hook issues).
    pub warnings: Vec<String>,
    /// Set when the run failed; `None` means success.
    pub error: Option<String>,
    /// Set when the scheduled run intentionally did no backup work.
    pub skipped_reason: Option<String>,
    pub destination_successes: u8,
    pub destination_failures: u8,
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
            skipped_reason: None,
            destination_successes: 0,
            destination_failures: 0,
        }
    }

    fn skipped(reason: String) -> Self {
        Self {
            archive_name: None,
            warnings: Vec::new(),
            error: None,
            skipped_reason: Some(reason),
            destination_successes: 0,
            destination_failures: 0,
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

const RETRY_DELAYS_SECONDS: [u64; 2] = [30, 120];
const METERED_SKIP_REASON: &str = "Skipped because the active network is marked as metered.";
const SNOOZE_SKIP_REASON: &str = "Skipped because automatic backups are snoozed.";
const BATTERY_SKIP_REASON: &str = "Skipped because this PC is running on battery.";
const WIFI_SKIP_REASON: &str = "Skipped because the active Wi-Fi network is not allowed.";

fn is_transient(error: &borg_core::error::BorgError) -> bool {
    use borg_core::error::BorgError;
    match error {
        BorgError::Timeout { .. } | BorgError::SshFailed { .. } => true,
        BorgError::Io(error) => matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::BrokenPipe
        ),
        BorgError::ProcessFailed { stderr, .. } => {
            let message = stderr.to_ascii_lowercase();
            let permanent = [
                "permission denied",
                "authentication",
                "repository does not exist",
                "repository not found",
                "integrity",
                "passphrase",
                "config",
            ];
            !permanent.iter().any(|needle| message.contains(needle))
                && [
                    "timed out",
                    "timeout",
                    "connection reset",
                    "connection refused",
                    "network is unreachable",
                    "temporary failure",
                    "broken pipe",
                    "remote host closed",
                ]
                .iter()
                .any(|needle| message.contains(needle))
        }
        _ => false,
    }
}

async fn retry_delay(index: usize) {
    #[cfg(not(test))]
    tokio::time::sleep(std::time::Duration::from_secs(RETRY_DELAYS_SECONDS[index])).await;
    #[cfg(test)]
    let _ = index;
}

pub fn is_missed(last_attempt: &str, grace_seconds: u64, now: chrono::DateTime<Utc>) -> bool {
    chrono::DateTime::parse_from_rfc3339(last_attempt)
        .map(|timestamp| {
            now.signed_duration_since(timestamp.with_timezone(&Utc))
                .num_seconds()
                > grace_seconds as i64
        })
        .unwrap_or(false)
}

fn should_skip_for_metered_network(
    schedule: &borg_platform_win::scheduler::ScheduleConfig,
    cost: crate::network::NetworkCost,
) -> bool {
    schedule.skip_metered_networks && cost.is_metered()
}

fn should_skip_for_battery(
    policy: &profiles::ResourcePolicy,
    source: borg_platform_win::resource::PowerSource,
) -> bool {
    policy.skip_on_battery && source == borg_platform_win::resource::PowerSource::Battery
}

fn wifi_allowed(policy: &profiles::ResourcePolicy, current: Option<&str>) -> bool {
    policy.allowed_wifi_names.is_empty()
        || current.is_some_and(|current| {
            policy
                .allowed_wifi_names
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(current))
        })
}

async fn record_attempt(
    config_dir: &Path,
    run_id: &str,
    profile_id: &str,
    attempt: u8,
    result: &borg_core::error::Result<borg_core::borg::OpOutcome>,
) {
    let transient = result.as_ref().err().is_some_and(is_transient);
    let event = history::ScheduledAttempt {
        run_id: run_id.into(),
        profile_id: profile_id.into(),
        attempt,
        timestamp: Utc::now().to_rfc3339(),
        outcome: if result.is_ok() {
            "success".into()
        } else {
            "failure".into()
        },
        transient,
        error_message: result.as_ref().err().map(|error| error.detail()),
    };
    let _ = history::append_scheduled_attempt(config_dir, event).await;
}

async fn record_skipped_attempt(config_dir: &Path, profile_id: &str, reason: &str) {
    let event = history::ScheduledAttempt {
        run_id: Utc::now().timestamp_millis().to_string(),
        profile_id: profile_id.into(),
        attempt: 1,
        timestamp: Utc::now().to_rfc3339(),
        outcome: "skipped".into(),
        transient: false,
        error_message: Some(reason.into()),
    };
    let _ = history::append_scheduled_attempt(config_dir, event).await;
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
    finish_as(config_dir, archive_name, started, result, None).await
}

async fn finish_as(
    config_dir: &Path,
    archive_name: &str,
    started: Instant,
    result: Result<Vec<String>, String>,
    success_outcome: Option<&str>,
) -> RunReport {
    let duration_seconds = started.elapsed().as_secs();
    let (outcome, warnings, error) = match result {
        Ok(warnings) => (success_outcome.unwrap_or("success"), warnings, None),
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
    let _ = history::append(config_dir, event).await;

    RunReport {
        archive_name: Some(archive_name.to_string()),
        warnings,
        error,
        skipped_reason: None,
        destination_successes: 0,
        destination_failures: 0,
    }
}

/// Run one backup from the active profile's schedule configuration. Never
/// panics; all failures are returned in the [`RunReport`].
pub async fn run_scheduled_backup(config_dir: &Path, borg: &BorgClient) -> RunReport {
    run_automatic_backup(config_dir, borg, true).await
}

pub async fn run_removable_backup(config_dir: &Path, borg: &BorgClient) -> RunReport {
    run_automatic_backup(config_dir, borg, false).await
}

async fn run_automatic_backup(
    config_dir: &Path,
    borg: &BorgClient,
    require_schedule: bool,
) -> RunReport {
    let started = Instant::now();

    let profile = match load_active_profile(config_dir).await {
        Ok(p) => p,
        Err(e) => return RunReport::preflight(e),
    };

    let schedule = match profile.schedule.clone().filter(|schedule| schedule.enabled) {
        Some(schedule) => schedule,
        None if !require_schedule && profile.resource_policy.removable_destination_trigger => {
            borg_platform_win::scheduler::ScheduleConfig {
                enabled: true,
                schedule: borg_platform_win::scheduler::Schedule::Hourly,
                skip_metered_networks: false,
            }
        }
        None => return RunReport::preflight("active profile has no enabled schedule".into()),
    };

    // Validate inputs the same way the manual backup path does.
    if let Err(e) = profile.repo.validate() {
        return RunReport::preflight(e.to_string());
    }
    if let Some(secondary) = &profile.secondary_repo {
        if let Err(error) = secondary.validate() {
            return RunReport::preflight(error.to_string());
        }
        if secondary.location() == profile.repo.location() {
            return RunReport::preflight("secondary repository must differ from primary".into());
        }
    }
    if let Err(e) = borg_core::config::validate_source_paths(&profile.backup_selection.source_paths)
    {
        return RunReport::preflight(e.to_string());
    }
    if let Err(e) = borg_core::config::validate_exclude_patterns(&profile.backup_selection.excludes)
    {
        return RunReport::preflight(e.to_string());
    }

    if crate::snooze::load(config_dir)
        .await
        .ok()
        .flatten()
        .is_some_and(|snooze| snooze.active(Utc::now()))
    {
        record_skipped_attempt(config_dir, &profile.id, SNOOZE_SKIP_REASON).await;
        return RunReport::skipped(SNOOZE_SKIP_REASON.into());
    }
    if borg_platform_win::resource::power_source()
        .ok()
        .is_some_and(|source| should_skip_for_battery(&profile.resource_policy, source))
    {
        record_skipped_attempt(config_dir, &profile.id, BATTERY_SKIP_REASON).await;
        return RunReport::skipped(BATTERY_SKIP_REASON.into());
    }
    if !profile.resource_policy.allowed_wifi_names.is_empty() {
        let current = borg_platform_win::resource::current_wifi_name()
            .ok()
            .flatten();
        let allowed = wifi_allowed(&profile.resource_policy, current.as_deref());
        if !allowed {
            record_skipped_attempt(config_dir, &profile.id, WIFI_SKIP_REASON).await;
            return RunReport::skipped(WIFI_SKIP_REASON.into());
        }
    }

    if schedule.skip_metered_networks {
        match crate::network::active_connection_cost() {
            Ok(cost) if should_skip_for_metered_network(&schedule, cost) => {
                let reason = METERED_SKIP_REASON.to_string();
                record_skipped_attempt(config_dir, &profile.id, &reason).await;
                return RunReport::skipped(reason);
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!("could not determine active network cost: {error}");
            }
        }
    }

    let archive_name = build_archive_name(&profile);
    if let Err(e) = borg_core::config::validate_archive_name(&archive_name) {
        return RunReport::preflight(format!("invalid archive name '{archive_name}': {e}"));
    }

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

    let raw_paths: Vec<PathBuf> = profile
        .backup_selection
        .source_paths
        .iter()
        .map(PathBuf::from)
        .collect();
    let _sleep_guard = match borg_platform_win::resource::SleepGuard::acquire(
        profile.resource_policy.prevent_sleep,
    ) {
        Ok(guard) => guard,
        Err(error) => {
            return finish(config_dir, &archive_name, started, Err(error)).await;
        }
    };

    // Scheduled (unattended) runs benefit from VSS most — files are likely in
    // use. Snapshot the source volume and back up from a junction mount so borg
    // stores clean, restorable paths; falls back to live files when VSS can't
    // run (multi-volume / non-admin / non-Windows). See commands.rs and
    // crates/borg-platform-win/src/vss.rs.
    let vss = borg_platform_win::vss::prepare_snapshot(&raw_paths).await;

    let cancel = CancelToken::new();
    let run_id = Utc::now().timestamp_millis().to_string();
    let mut destinations = vec![("primary", profile.repo.clone())];
    if let Some(secondary) = profile.secondary_repo.clone() {
        destinations.push(("secondary", secondary));
    }
    let has_secondary = destinations.len() > 1;
    let mut warnings = Vec::new();
    let mut successes = 0_u8;
    let mut failures = 0_u8;
    for (destination_name, destination) in destinations {
        let pass = lookup_passphrase(&destination);
        let backup_profile = BackupProfile {
            name: profile.name.clone(),
            source_paths: vss.source_paths.clone(),
            excludes: profile.backup_selection.excludes.clone(),
            compression: Compression::default(),
            repo: destination.clone(),
            upload_limit_kib: profile.resource_policy.upload_limit_kib,
        };
        let mut create_result = None;
        for attempt in 1_u8..=3 {
            let result = borg
                .create(
                    &backup_profile,
                    &archive_name,
                    vss.cwd.as_deref(),
                    pass.as_deref(),
                    &cancel,
                    |_| {},
                )
                .await;
            record_attempt(config_dir, &run_id, &profile.id, attempt, &result).await;
            let retry = result.as_ref().err().is_some_and(is_transient) && attempt < 3;
            create_result = Some(result);
            if !retry {
                break;
            }
            retry_delay(usize::from(attempt - 1)).await;
        }
        match create_result.expect("retry loop always runs") {
            Ok(outcome) => {
                successes += 1;
                warnings.extend(outcome.warnings);
                record_destination(
                    config_dir,
                    &run_id,
                    &profile.id,
                    destination_name,
                    "success",
                )
                .await;
                if let Some(retention) = profile.retention.as_ref()
                    && retention.validate().is_ok()
                {
                    match borg.prune(&destination, retention, pass.as_deref()).await {
                        Ok(outcome) => warnings.extend(outcome.warnings),
                        Err(_) => warnings.push(format!(
                            "retention failed for the {destination_name} destination"
                        )),
                    }
                }
            }
            Err(borg_core::error::BorgError::Cancelled) => {
                record_destination(
                    config_dir,
                    &run_id,
                    &profile.id,
                    destination_name,
                    "cancelled",
                )
                .await;
                if destination_name == "primary" && has_secondary {
                    record_destination(config_dir, &run_id, &profile.id, "secondary", "skipped")
                        .await;
                }
                failures += 1;
                break;
            }
            Err(_) => {
                failures += 1;
                record_destination(
                    config_dir,
                    &run_id,
                    &profile.id,
                    destination_name,
                    "failure",
                )
                .await;
                warnings.push(format!("{destination_name} destination backup failed"));
            }
        }
    }
    // Release the snapshot + junction regardless of how the backup ended.
    vss.release().await;

    // The backup succeeded; a failing post-backup hook is only a warning.
    if successes > 0
        && let Some(cmd) = nonempty(&profile.post_backup)
        && let Err(e) = borg_core::hooks::run("post-backup", cmd, &hook_ctx).await
    {
        warnings.push(format!("post-backup command failed: {}", e.detail()));
    }

    let result = if successes > 0 {
        Ok(warnings)
    } else {
        Err("all backup destinations failed".into())
    };
    let success_outcome = (successes > 0 && failures > 0).then_some("partial_success");
    let mut report = finish_as(config_dir, &archive_name, started, result, success_outcome).await;
    report.destination_successes = successes;
    report.destination_failures = failures;
    report
}

async fn record_destination(
    config_dir: &Path,
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
            timestamp: Utc::now().to_rfc3339(),
            outcome: outcome.into(),
            error_message: (outcome == "failure")
                .then(|| "destination backup failed; details omitted".into()),
        },
    )
    .await;
}

/// Run the opt-in monthly metadata-only repository check for the active profile.
pub async fn run_scheduled_integrity_check(
    config_dir: &Path,
    borg: &BorgClient,
) -> Result<crate::history::IntegrityEvent, String> {
    let data = crate::profiles::load(config_dir).await?;
    let profile = data
        .active()
        .ok_or_else(|| "no active profile".to_string())?;
    if !profile
        .integrity_schedule
        .as_ref()
        .is_some_and(|schedule| schedule.enabled)
    {
        return Err("monthly integrity check is not enabled".into());
    }

    let started = std::time::Instant::now();
    let passphrase = crate::keychain::get_passphrase(&profile.repo.ssh_url())
        .ok()
        .flatten();
    let result = borg
        .check(
            &profile.repo,
            CheckMode::Repository,
            passphrase.as_deref(),
            &CancelToken::new(),
            |_| {},
        )
        .await;
    let warnings = result
        .as_ref()
        .ok()
        .map(|outcome| outcome.warnings.clone())
        .unwrap_or_default();
    let event = crate::history::IntegrityEvent {
        id: Utc::now().timestamp_millis().to_string(),
        timestamp: Utc::now().to_rfc3339(),
        profile_id: profile.id.clone(),
        mode: "repository".into(),
        outcome: if result.is_ok() && warnings.is_empty() {
            "success".into()
        } else {
            "failure".into()
        },
        duration_seconds: started.elapsed().as_secs(),
        error_message: result
            .as_ref()
            .err()
            .map(|error| error.detail())
            .or_else(|| (!warnings.is_empty()).then(|| warnings.join("\n"))),
    };
    history::append_integrity(config_dir, event.clone()).await?;
    result.map_err(|error| error.detail())?;
    if !warnings.is_empty() {
        return Err(format!(
            "repository check completed with warnings: {}",
            warnings.join("; ")
        ));
    }
    Ok(event)
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
            secondary_repo: None,
            backup_selection: profiles::BackupSelection {
                source_paths: sources,
                ..Default::default()
            },
            schedule: Some(ScheduleConfig {
                enabled,
                schedule: Schedule::Hourly,
                skip_metered_networks: false,
            }),
            integrity_schedule: None,
            restore_drill_schedule: None,
            resource_policy: Default::default(),
            hardening: Default::default(),
            reporting: Default::default(),
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
        let events = history::load(&config_dir).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, "success");
        assert_eq!(events[0].kind, "backup");
        let attempt = history::latest_scheduled_attempt(&config_dir, "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attempt.attempt, 1);
        assert_eq!(attempt.outcome, "success");
        assert!(!attempt.transient);
    }

    #[tokio::test]
    async fn restore_drill_extracts_verifies_and_cleans_up() {
        let Some(borg) = borg_or_skip() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        let repo = local_repo(&tmp.path().join("repo"));
        let source = tmp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("sample.txt"), b"verified bytes").unwrap();
        borg.init_repo(&repo, "none", None).await.unwrap();
        let profile = BackupProfile {
            name: "drill".into(),
            source_paths: vec![source],
            excludes: vec![],
            compression: Compression::default(),
            repo: repo.clone(),
            upload_limit_kib: None,
        };
        borg.create(
            &profile,
            "drill-archive",
            None,
            None,
            &CancelToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let mut saved = profile_with_schedule(repo, vec!["unused".into()], false);
        saved.restore_drill_schedule = Some(profiles::RestoreDrillSchedule { enabled: true });
        write_profile(&config_dir, saved).await;

        run_restore_drill(&config_dir, &borg).await.unwrap();
        let event = history::latest_restore_drill(&config_dir, "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.outcome, "success");
        assert_eq!(event.files_checked, 1);
        assert!(
            std::fs::read_dir(&config_dir)
                .unwrap()
                .flatten()
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .contains("restore-drill"))
        );
    }

    #[tokio::test]
    async fn secondary_destination_uses_same_archive_name() {
        let Some(borg) = borg_or_skip() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join("config");
        let source = tmp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("same.txt"), b"one snapshot").unwrap();
        let primary = local_repo(&tmp.path().join("primary"));
        let secondary = local_repo(&tmp.path().join("secondary"));
        borg.init_repo(&primary, "none", None).await.unwrap();
        borg.init_repo(&secondary, "none", None).await.unwrap();
        let mut profile =
            profile_with_schedule(primary.clone(), vec![source.to_string_lossy().into()], true);
        profile.secondary_repo = Some(secondary.clone());
        write_profile(&config_dir, profile).await;

        let report = run_scheduled_backup(&config_dir, &borg).await;
        assert!(report.succeeded());
        assert_eq!(report.destination_successes, 2);
        let primary_name = &borg.list_archives(&primary, None).await.unwrap()[0].name;
        let secondary_name = &borg.list_archives(&secondary, None).await.unwrap()[0].name;
        assert_eq!(primary_name, secondary_name);
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
        let events = history::load(&config_dir).await.unwrap();
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

    #[test]
    fn retry_classification_only_accepts_transport_failures() {
        use borg_core::error::BorgError;
        assert!(is_transient(&BorgError::Timeout { seconds: 30 }));
        assert!(is_transient(&BorgError::ProcessFailed {
            message: "create".into(),
            exit_code: Some(2),
            stderr: "Connection reset by peer".into(),
        }));
        for stderr in [
            "Permission denied (publickey)",
            "Repository does not exist",
            "Incorrect passphrase",
            "Data integrity error",
        ] {
            assert!(!is_transient(&BorgError::ProcessFailed {
                message: "create".into(),
                exit_code: Some(2),
                stderr: stderr.into(),
            }));
        }
    }

    #[test]
    fn retry_delays_are_fixed() {
        assert_eq!(RETRY_DELAYS_SECONDS, [30, 120]);
    }

    #[test]
    fn skipped_report_is_successful_without_archive() {
        let report = RunReport::skipped("metered".into());
        assert!(report.succeeded());
        assert_eq!(report.archive_name, None);
        assert_eq!(report.skipped_reason.as_deref(), Some("metered"));
    }

    #[test]
    fn metered_skip_decision_requires_opt_in_and_metered_cost() {
        let mut profile =
            profile_with_schedule(local_repo(Path::new("/repo")), vec!["/src".into()], true);
        let schedule = profile.schedule.as_mut().unwrap();
        assert!(!should_skip_for_metered_network(
            schedule,
            crate::network::NetworkCost::Metered
        ));

        schedule.skip_metered_networks = true;
        assert!(should_skip_for_metered_network(
            schedule,
            crate::network::NetworkCost::Metered
        ));
        assert!(!should_skip_for_metered_network(
            schedule,
            crate::network::NetworkCost::Unrestricted
        ));
        assert!(!should_skip_for_metered_network(
            schedule,
            crate::network::NetworkCost::Unknown
        ));
    }

    #[test]
    fn battery_and_wifi_policy_decisions_are_conservative() {
        let mut policy = profiles::ResourcePolicy::default();
        assert!(!should_skip_for_battery(
            &policy,
            borg_platform_win::resource::PowerSource::Battery
        ));
        policy.skip_on_battery = true;
        assert!(should_skip_for_battery(
            &policy,
            borg_platform_win::resource::PowerSource::Battery
        ));
        assert!(!should_skip_for_battery(
            &policy,
            borg_platform_win::resource::PowerSource::Ac
        ));

        policy.allowed_wifi_names = vec!["Home Wi-Fi".into()];
        assert!(wifi_allowed(&policy, Some("home wi-fi")));
        assert!(!wifi_allowed(&policy, Some("Cafe")));
        assert!(!wifi_allowed(&policy, None));
    }

    #[test]
    fn missed_run_respects_grace_boundary() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 6, 29, 12, 0, 0).unwrap();
        assert!(!is_missed("2026-06-29T10:30:00Z", 90 * 60, now));
        assert!(is_missed("2026-06-29T10:29:59Z", 90 * 60, now));
        assert!(!is_missed("not-a-timestamp", 90 * 60, now));
    }
}
