use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::history::{
    BackupEvent, DestinationAttempt, IntegrityEvent, RestoreDrillEvent, ScheduledAttempt,
};
use crate::profiles::Profile;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthSeverity {
    Green,
    Amber,
    Red,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionHealth {
    pub severity: HealthSeverity,
    pub summary: String,
    pub last_success: Option<String>,
    pub next_run: Option<String>,
    pub missed_runs: u32,
    pub unavailable_sources: u32,
    pub repository_reachable: bool,
    pub integrity_status: String,
    pub restore_drill_status: String,
    pub recovery_key_ready: bool,
    pub destination_state: String,
    pub secondary_status: Option<String>,
    pub actions: Vec<HealthAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthAction {
    pub label: String,
    pub href: String,
}

pub struct HealthInputs<'a> {
    pub profile: &'a Profile,
    pub events: &'a [BackupEvent],
    pub scheduled: Option<&'a ScheduledAttempt>,
    pub missed: bool,
    pub unavailable_sources: u32,
    pub repository_reachable: bool,
    pub integrity: Option<&'a IntegrityEvent>,
    pub drill: Option<&'a RestoreDrillEvent>,
    pub primary_attempt: Option<&'a DestinationAttempt>,
    pub secondary_attempt: Option<&'a DestinationAttempt>,
    pub passphrase_available: bool,
    pub now: DateTime<Utc>,
}

pub fn aggregate(inputs: HealthInputs<'_>) -> ProtectionHealth {
    let last_success = inputs
        .events
        .iter()
        .rev()
        .find(|event| event.kind == "backup" && event.outcome == "success")
        .map(|event| event.timestamp.clone());
    let stale = last_success
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|timestamp| {
            inputs
                .now
                .signed_duration_since(timestamp.with_timezone(&Utc))
                > Duration::hours(i64::from(inputs.profile.reporting.stale_after_hours))
        });
    let consecutive_failures = inputs
        .events
        .iter()
        .rev()
        .take_while(|event| event.kind == "backup" && event.outcome == "failure")
        .count() as u32
        + u32::from(
            inputs
                .scheduled
                .is_some_and(|attempt| attempt.outcome == "failure"),
        );
    let integrity_status = status(inputs.integrity.map(|event| event.outcome.as_str()));
    let restore_drill_status = status(inputs.drill.map(|event| event.outcome.as_str()));
    let recovery_key_ready = !inputs.profile.recovery.encrypted_repository
        || (inputs.profile.hardening.recovery_key_exported && inputs.passphrase_available);
    let secondary_status = inputs.profile.secondary_repo.as_ref().map(|_| {
        match inputs.secondary_attempt {
            Some(attempt) if attempt.outcome == "success" => {
                if inputs
                    .primary_attempt
                    .is_some_and(|primary| primary.run_id != attempt.run_id)
                {
                    "lagging"
                } else {
                    "current"
                }
            }
            Some(_) => "failed",
            None => "not_run",
        }
        .to_string()
    });

    let red = !inputs.repository_reachable
        || inputs.unavailable_sources > 0
        || last_success.is_none()
        || consecutive_failures >= inputs.profile.reporting.failure_threshold.max(1);
    let amber = !red
        && (inputs.missed
            || stale
            || integrity_status != "success"
            || restore_drill_status != "success"
            || !recovery_key_ready);
    let amber = amber
        || secondary_status
            .as_deref()
            .is_some_and(|status| status != "current");
    let severity = if red {
        HealthSeverity::Red
    } else if amber {
        HealthSeverity::Amber
    } else {
        HealthSeverity::Green
    };
    let summary = match severity {
        HealthSeverity::Green => "Protection is healthy",
        HealthSeverity::Amber => "Protection needs attention",
        HealthSeverity::Red => "Protection is at risk",
    }
    .into();
    let mut actions = Vec::new();
    if inputs.unavailable_sources > 0 {
        actions.push(action("Review unavailable sources", "/backup"));
    }
    if !inputs.repository_reachable {
        actions.push(action("Check backup destination", "/settings"));
    }
    if integrity_status != "success" || restore_drill_status != "success" {
        actions.push(action("Run protection checks", "/settings"));
    }
    if !recovery_key_ready {
        actions.push(action("Export a recovery key", "/settings"));
    }

    ProtectionHealth {
        severity,
        summary,
        last_success,
        next_run: next_run(inputs.profile, inputs.now),
        missed_runs: u32::from(inputs.missed),
        unavailable_sources: inputs.unavailable_sources,
        repository_reachable: inputs.repository_reachable,
        integrity_status,
        restore_drill_status,
        recovery_key_ready,
        destination_state: if inputs.repository_reachable {
            "available".into()
        } else {
            "unavailable".into()
        },
        secondary_status,
        actions,
    }
}

fn status(outcome: Option<&str>) -> String {
    outcome.unwrap_or("not_run").to_owned()
}

fn action(label: &str, href: &str) -> HealthAction {
    HealthAction {
        label: label.into(),
        href: href.into(),
    }
}

fn next_run(profile: &Profile, now: DateTime<Utc>) -> Option<String> {
    let schedule = profile
        .schedule
        .as_ref()
        .filter(|schedule| schedule.enabled)?;
    match schedule.schedule {
        borg_platform_win::scheduler::Schedule::Hourly => {
            Some((now + Duration::hours(1)).to_rfc3339())
        }
        borg_platform_win::scheduler::Schedule::Daily { hour, minute } => {
            let local_now = now.with_timezone(&Local);
            let date = local_now.date_naive();
            let candidate = Local
                .from_local_datetime(&date.and_hms_opt(hour.into(), minute.into(), 0)?)
                .earliest()?;
            let next = if candidate <= local_now {
                candidate + Duration::days(1)
            } else {
                candidate
            };
            Some(next.with_timezone(&Utc).to_rfc3339())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borg_core::config::RepoConfig;

    fn profile() -> Profile {
        Profile {
            id: "p".into(),
            name: "P".into(),
            repo: RepoConfig {
                ssh_host: "host".into(),
                ssh_port: 22,
                ssh_user: "borg".into(),
                repo_path: "/repo".into(),
                ssh_key_path: None,
            },
            secondary_repo: None,
            backup_selection: crate::profiles::BackupSelection::default(),
            schedule: None,
            integrity_schedule: None,
            restore_drill_schedule: None,
            resource_policy: Default::default(),
            hardening: crate::profiles::HardeningPosture {
                recovery_key_exported: true,
                ..Default::default()
            },
            reporting: Default::default(),
            placeholder_policy: Default::default(),
            storage_warnings: Default::default(),
            recovery: Default::default(),
            retention: None,
            archive_template: None,
            pre_backup: None,
            post_backup: None,
        }
    }

    #[test]
    fn unavailable_repository_is_red() {
        let profile = profile();
        let health = aggregate(HealthInputs {
            profile: &profile,
            events: &[],
            scheduled: None,
            missed: false,
            unavailable_sources: 0,
            repository_reachable: false,
            integrity: None,
            drill: None,
            primary_attempt: None,
            secondary_attempt: None,
            passphrase_available: true,
            now: Utc::now(),
        });
        assert_eq!(health.severity, HealthSeverity::Red);
        assert!(!health.actions.is_empty());
    }

    #[test]
    fn complete_recent_protection_is_green() {
        let profile = profile();
        let now = Utc::now();
        let events = vec![BackupEvent {
            id: "1".into(),
            timestamp: now.to_rfc3339(),
            kind: "backup".into(),
            archive_name: "archive".into(),
            outcome: "success".into(),
            duration_seconds: 1,
            file_count: None,
            original_size: None,
            error_message: None,
        }];
        let integrity = IntegrityEvent {
            id: "i".into(),
            timestamp: now.to_rfc3339(),
            profile_id: "p".into(),
            mode: "repository".into(),
            outcome: "success".into(),
            duration_seconds: 1,
            error_message: None,
        };
        let drill = RestoreDrillEvent {
            id: "d".into(),
            timestamp: now.to_rfc3339(),
            profile_id: "p".into(),
            outcome: "success".into(),
            files_checked: 1,
            duration_seconds: 1,
            error_message: None,
        };
        let health = aggregate(HealthInputs {
            profile: &profile,
            events: &events,
            scheduled: None,
            missed: false,
            unavailable_sources: 0,
            repository_reachable: true,
            integrity: Some(&integrity),
            drill: Some(&drill),
            primary_attempt: None,
            secondary_attempt: None,
            passphrase_available: true,
            now,
        });
        assert_eq!(health.severity, HealthSeverity::Green);
    }
}
