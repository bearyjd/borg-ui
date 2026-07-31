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

/// Whether a recovery-key export is still usable, given any later passphrase
/// rotation.
///
/// An exported key carries the passphrase that was current when it was written.
/// Rotating the repository passphrase afterwards does not update that file, so
/// importing it restores the *old* key and the new passphrase stops working —
/// verified against borg 1.4.4 for both `repokey` and `keyfile`. A stale export
/// must therefore not count towards readiness, or the UI cheerfully reports
/// "ready" with a key that would lock the user out.
fn export_predates_rotation(
    key_export: Option<&ReadinessEvent>,
    rotation: Option<&ReadinessEvent>,
) -> bool {
    let (Some(export), Some(rotation)) = (key_export, rotation) else {
        return false;
    };
    let parse = |ts: &str| {
        DateTime::parse_from_rfc3339(ts)
            .ok()
            .map(|t| t.with_timezone(&Utc))
    };
    match (parse(&export.timestamp), parse(&rotation.timestamp)) {
        (Some(exported_at), Some(rotated_at)) => rotated_at > exported_at,
        // An unparseable timestamp on either side means we cannot prove the
        // export is still current. Fail towards "re-export", never towards a
        // false "ready".
        _ => true,
    }
}

pub fn evaluate(
    encrypted: bool,
    passphrase_available: bool,
    key_export: Option<&ReadinessEvent>,
    passphrase_rotation: Option<&ReadinessEvent>,
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
    let export_stale = export_predates_rotation(key_export, passphrase_rotation);
    let export_complete =
        !encrypted || (key_export.is_some_and(|event| event.outcome == "success") && !export_stale);
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
            // Say *why* a previously-green step went red, or the user sees a
            // completed export marked incomplete and assumes the UI is broken.
            label: if export_stale {
                "Recovery key export is older than the current passphrase — export it again".into()
            } else {
                "Encrypted repository key was exported successfully".to_string()
            },
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

    fn readiness(kind: &str, timestamp: &str) -> ReadinessEvent {
        ReadinessEvent {
            timestamp: timestamp.into(),
            profile_id: "p".into(),
            kind: kind.into(),
            outcome: "success".into(),
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-03T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// An encrypted repo with everything green — the baseline the staleness
    /// tests below perturb one field at a time.
    fn fully_ready(
        key_export: Option<&ReadinessEvent>,
        rotation: Option<&ReadinessEvent>,
    ) -> RecoveryReadiness {
        evaluate(
            true,
            true,
            key_export,
            rotation,
            Some(&integrity("2026-07-02T00:00:00Z")),
            Some(&drill("2026-07-02T00:00:00Z")),
            now(),
        )
    }

    fn key_export_step(state: &RecoveryReadiness) -> &ReadinessStep {
        state
            .steps
            .iter()
            .find(|step| step.id == "key_export")
            .expect("key_export step exists")
    }

    #[test]
    fn encrypted_repository_requires_every_step() {
        assert!(
            !evaluate(
                true,
                true,
                None,
                None,
                Some(&integrity("2026-07-02T00:00:00Z")),
                Some(&drill("2026-07-02T00:00:00Z")),
                now()
            )
            .ready
        );
    }

    #[test]
    fn unencrypted_repository_omits_key_requirements_and_rejects_stale_checks() {
        let state = evaluate(
            false,
            false,
            None,
            None,
            Some(&integrity("2025-01-01T00:00:00Z")),
            Some(&drill("2026-07-02T00:00:00Z")),
            now(),
        );
        assert!(!state.ready);
        assert!(!state.steps[0].required);
        assert!(!state.steps[1].required);
    }

    #[test]
    fn export_then_no_rotation_stays_ready() {
        let export = readiness("key_export", "2026-07-01T00:00:00Z");
        let state = fully_ready(Some(&export), None);
        assert!(state.ready);
        assert!(key_export_step(&state).complete);
    }

    /// The bug: an exported recovery key carries the passphrase current at
    /// export time, so a later rotation makes it useless — importing it
    /// restores the old key and the new passphrase stops working. Readiness
    /// used to keep reporting "ready" against exactly that key.
    #[test]
    fn rotation_after_export_invalidates_the_export() {
        let export = readiness("key_export", "2026-07-01T00:00:00Z");
        let rotation = readiness("passphrase_rotation", "2026-07-02T00:00:00Z");
        let state = fully_ready(Some(&export), Some(&rotation));
        assert!(!state.ready);
        let step = key_export_step(&state);
        assert!(!step.complete);
        // Explain the regression, don't just flip it red.
        assert!(
            step.label.contains("older than the current passphrase"),
            "{}",
            step.label
        );
    }

    #[test]
    fn re_exporting_after_a_rotation_restores_readiness() {
        let rotation = readiness("passphrase_rotation", "2026-07-01T00:00:00Z");
        let export = readiness("key_export", "2026-07-02T00:00:00Z");
        let state = fully_ready(Some(&export), Some(&rotation));
        assert!(state.ready);
        assert!(key_export_step(&state).complete);
    }

    /// Same instant means the export cannot be proven older, so it stands —
    /// the ordering guard must be strict, not >=.
    #[test]
    fn simultaneous_export_and_rotation_keeps_the_export() {
        let export = readiness("key_export", "2026-07-01T00:00:00Z");
        let rotation = readiness("passphrase_rotation", "2026-07-01T00:00:00Z");
        assert!(fully_ready(Some(&export), Some(&rotation)).ready);
    }

    /// Timestamps are compared as instants, not strings — a `+02:00` offset
    /// that is actually *earlier* than a `Z` timestamp must not read as later.
    #[test]
    fn timestamps_compare_as_instants_not_lexically() {
        // 2026-07-01T09:00:00+02:00 == 07:00Z, i.e. before the 08:00Z export.
        let export = readiness("key_export", "2026-07-01T08:00:00Z");
        let rotation = readiness("passphrase_rotation", "2026-07-01T09:00:00+02:00");
        assert!(
            fully_ready(Some(&export), Some(&rotation)).ready,
            "a rotation before the export must not invalidate it"
        );
    }

    /// If either timestamp is unreadable we cannot prove the export is current.
    /// Fail towards "re-export", never towards a false "ready".
    #[test]
    fn unparseable_timestamps_fail_towards_re_export() {
        let export = readiness("key_export", "not-a-timestamp");
        let rotation = readiness("passphrase_rotation", "2026-07-02T00:00:00Z");
        assert!(!fully_ready(Some(&export), Some(&rotation)).ready);
    }

    /// A rotation with no export at all must not crash or accidentally satisfy
    /// the export requirement.
    #[test]
    fn rotation_without_any_export_is_still_not_ready() {
        let rotation = readiness("passphrase_rotation", "2026-07-02T00:00:00Z");
        let state = fully_ready(None, Some(&rotation));
        assert!(!state.ready);
        assert!(!key_export_step(&state).complete);
    }

    /// An unencrypted repo has no key to export, so a rotation event (which
    /// should not happen there anyway) must not make it un-ready.
    #[test]
    fn unencrypted_repository_ignores_rotation_staleness() {
        let export = readiness("key_export", "2026-07-01T00:00:00Z");
        let rotation = readiness("passphrase_rotation", "2026-07-02T00:00:00Z");
        let state = evaluate(
            false,
            false,
            Some(&export),
            Some(&rotation),
            Some(&integrity("2026-07-02T00:00:00Z")),
            Some(&drill("2026-07-02T00:00:00Z")),
            now(),
        );
        assert!(state.ready);
    }
}
