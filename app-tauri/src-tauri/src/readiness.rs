use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::history::{IntegrityEvent, ReadinessEvent, RestoreDrillEvent};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReadinessStep {
    pub id: String,
    pub label: String,
    pub complete: bool,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RecoveryReadiness {
    pub ready: bool,
    pub steps: Vec<ReadinessStep>,
}

pub fn evaluate(
    encrypted: bool,
    passphrase_available: bool,
    key_export: Option<&ReadinessEvent>,
    integrity: Option<&IntegrityEvent>,
    drill: Option<&RestoreDrillEvent>,
    now: DateTime<Utc>,
) -> RecoveryReadiness {
    let recent_success = |timestamp: &str, outcome: &str| {
        outcome == "success"
            && DateTime::parse_from_rfc3339(timestamp)
                .ok()
                .is_some_and(|time| {
                    now.signed_duration_since(time.with_timezone(&Utc)) <= Duration::days(90)
                })
    };
    let passphrase_complete = !encrypted || passphrase_available;
    let export_complete = !encrypted || key_export.is_some_and(|event| event.outcome == "success");
    let integrity_complete =
        integrity.is_some_and(|event| recent_success(&event.timestamp, &event.outcome));
    let drill_complete =
        drill.is_some_and(|event| recent_success(&event.timestamp, &event.outcome));
    let steps = vec![
        ReadinessStep {
            id: "passphrase".into(),
            label: "Repository passphrase is available in Windows Credential Manager".into(),
            complete: passphrase_complete,
            required: encrypted,
        },
        ReadinessStep {
            id: "key_export".into(),
            label: "Encrypted repository key was exported successfully".into(),
            complete: export_complete,
            required: encrypted,
        },
        ReadinessStep {
            id: "integrity".into(),
            label: "Integrity verification succeeded within 90 days".into(),
            complete: integrity_complete,
            required: true,
        },
        ReadinessStep {
            id: "restore".into(),
            label: "Sample restore succeeded within 90 days".into(),
            complete: drill_complete,
            required: true,
        },
    ];
    RecoveryReadiness {
        ready: steps.iter().all(|step| !step.required || step.complete),
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integrity(timestamp: &str) -> IntegrityEvent {
        IntegrityEvent {
            id: "i".into(),
            timestamp: timestamp.into(),
            profile_id: "p".into(),
            mode: "repository".into(),
            outcome: "success".into(),
            duration_seconds: 1,
            error_message: None,
        }
    }

    fn drill(timestamp: &str) -> RestoreDrillEvent {
        RestoreDrillEvent {
            id: "d".into(),
            timestamp: timestamp.into(),
            profile_id: "p".into(),
            outcome: "success".into(),
            files_checked: 1,
            duration_seconds: 1,
            error_message: None,
        }
    }

    #[test]
    fn encrypted_repository_requires_every_step() {
        let now = DateTime::parse_from_rfc3339("2026-07-03T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(
            !evaluate(
                true,
                true,
                None,
                Some(&integrity("2026-07-02T00:00:00Z")),
                Some(&drill("2026-07-02T00:00:00Z")),
                now
            )
            .ready
        );
    }

    #[test]
    fn unencrypted_repository_omits_key_requirements_and_rejects_stale_checks() {
        let now = DateTime::parse_from_rfc3339("2026-07-03T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let state = evaluate(
            false,
            false,
            None,
            Some(&integrity("2025-01-01T00:00:00Z")),
            Some(&drill("2026-07-02T00:00:00Z")),
            now,
        );
        assert!(!state.ready);
        assert!(!state.steps[0].required);
        assert!(!state.steps[1].required);
    }
}
