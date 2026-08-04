//! Storing, verifying, and rotating the repository passphrase.

use super::*;

/// How long to spend proving a passphrase before storing it. Deliberately far
/// below `QUICK_OP_TIMEOUT_SECS`: this check runs while the user waits on a
/// dialog, and a repository that is merely unreachable must not stall them for
/// two minutes — it falls through to `Undetermined` and stores anyway.
const PASSPHRASE_CHECK_TIMEOUT_SECS: u64 = 20;

/// Whether a passphrase actually opens the repository.
///
/// `pub(super)` rather than private: `recovery_key` reuses this verdict after a
/// key import, but nothing outside `commands` should see it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PassphraseCheck {
    Opens,
    /// borg said the passphrase is wrong — the one case worth blocking on.
    Wrong,
    /// No verdict: repo unreachable, not initialised yet, borg missing, timed
    /// out. Storing must still be allowed, or a passphrase could never be saved
    /// before the repository exists (the first-run setup flow depends on that).
    Undetermined,
}

/// borg's wrong-passphrase wording. Anything else — connection refused, no such
/// repository, borg not found — is deliberately *not* treated as a wrong
/// passphrase.
///
/// Integrity failures are excluded on purpose. borg reports a damaged or
/// tampered repository as an `IntegrityError`, and its wording can mention
/// decryption, which is ambiguous between "wrong key" and "corrupt data". A
/// user repairing a damaged repo would enter their genuinely correct passphrase,
/// be told it "does not open this repository", and might then discard the only
/// copy they have. Everywhere else this check fails open; this is the one place
/// it could fail closed, so it must not guess.
fn looks_like_wrong_passphrase(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("integrityerror") || lower.contains("integrity error") {
        return false;
    }
    // Confirmed against borg 1.4.4: "Passphrase supplied in BORG_PASSPHRASE, by
    // BORG_PASSCOMMAND, or via BORG_PASSPHRASE_FD is incorrect."
    (lower.contains("passphrase") && lower.contains("incorrect"))
        || lower.contains("wrong passphrase")
}

pub(super) async fn check_passphrase(
    borg: &borg_core::borg::BorgClient,
    repo: &RepoConfig,
    passphrase: &str,
) -> PassphraseCheck {
    let probe = borg.info(repo, Some(passphrase));
    match tokio::time::timeout(
        std::time::Duration::from_secs(PASSPHRASE_CHECK_TIMEOUT_SECS),
        probe,
    )
    .await
    {
        Ok(Ok(_)) => PassphraseCheck::Opens,
        // `detail()` rather than `to_string()`: the wrong-passphrase wording is
        // in borg's stderr, which `Display` deliberately omits. Used only for
        // matching — never surfaced, so no stderr reaches the UI from here.
        Ok(Err(e)) if looks_like_wrong_passphrase(&e.detail()) => PassphraseCheck::Wrong,
        Ok(Err(_)) | Err(_) => PassphraseCheck::Undetermined,
    }
}

#[tauri::command]
pub async fn set_repo_passphrase(
    state: State<'_, AppState>,
    repo: RepoConfig,
    passphrase: String,
) -> Result<(), String> {
    repo.validate().map_err(|e| e.to_string())?;
    if passphrase.is_empty() {
        return Err("passphrase cannot be empty".into());
    }
    // This command only writes the stored copy — it never changes what the
    // repository wants. Storing a passphrase that does not open the repository
    // is therefore silently useless, and it is exactly what the "Only update
    // the stored copy" repair path is for, where a typo would leave the user
    // just as locked out but with green confirmation. Verify when we can, and
    // block only on a definite verdict.
    if check_passphrase(&state.borg, &repo, &passphrase).await == PassphraseCheck::Wrong {
        return Err(
            "that passphrase does not open this repository — it was not saved. Enter the \
             passphrase this repository was created with, or use Change passphrase to set a new one."
                .into(),
        );
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
    let rotation = state
        .borg
        .change_passphrase(&repo, &old, &new_passphrase)
        .await;
    // Record BEFORE reacting to the outcome. What makes an exported recovery key
    // stale is the *repository* accepting the new passphrase — not the keychain
    // write that follows it. Recording afterwards meant the two paths where the
    // repo has (or may have) rotated but we still return Err — a failed keychain
    // write, and a timeout that borg may yet commit — left readiness green
    // against a key that no longer opens the repository. Over-recording only
    // costs a spurious "re-export", which is the direction this must fail.
    if !matches!(rotation, Err(BorgError::ProcessFailed { .. }))
        && let Err(e) = record_passphrase_rotation(&app, &repo).await
    {
        tracing::warn!("could not record passphrase rotation for recovery readiness: {e}");
    }
    rotation.map_err(|e| match e {
        BorgError::Timeout { .. } => rotation_indeterminate_error(&e.to_string()),
        other => other.to_string(),
    })?;
    // Only after the repository accepted the rotation — keeping the stored
    // copy in lockstep with the repo is the entire point of this command. If
    // this write fails the two are now out of sync in the most dangerous
    // direction, so say so explicitly rather than reporting a plain failure.
    keychain::set_passphrase(&repo.ssh_url(), &new_passphrase)
        .map_err(|e| rotated_unsaved_error(&e))
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
    // Every matching profile, not the first. Nothing forbids two profiles
    // targeting one repository — the keychain is itself keyed on `ssh_url()`, so
    // sharing one is by design — and stopping at the first would leave the rest
    // counting a key export that is now stale.
    let matching: Vec<String> = data
        .profiles
        .iter()
        .filter(|p| p.repo.ssh_url() == url)
        .map(|p| p.id.clone())
        .collect();
    if matching.is_empty() {
        // A repo configured in the form but not yet saved as a profile has no
        // readiness to invalidate.
        return Ok(());
    }
    let dir = config_dir(app).await?;
    let timestamp = chrono::Utc::now().to_rfc3339();
    for profile_id in matching {
        history::append_readiness_event(
            &dir,
            history::ReadinessEvent {
                timestamp: timestamp.clone(),
                profile_id,
                kind: "passphrase_rotation".into(),
                outcome: "success".into(),
            },
        )
        .await?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Only a definite wrong-passphrase verdict may block a save. Everything
    /// else — unreachable repo, missing repo, missing borg — must fall through
    /// so a passphrase can still be stored before the repository exists.
    #[test]
    fn wrong_passphrase_detection_ignores_unrelated_failures() {
        for wrong in [
            "passphrase supplied in BORG_PASSPHRASE, by BORG_PASSCOMMAND, or via BORG_PASSPHRASE_FD is incorrect.",
            "Wrong passphrase",
        ] {
            assert!(looks_like_wrong_passphrase(wrong), "{wrong}");
        }
        for unrelated in [
            "Repository /backups/pc does not exist.",
            "connect to host tower port 22: Connection refused",
            "bash: borg: command not found",
            "Failed to create/acquire the lock (timeout).",
            "operation timed out after 20s",
            // The unencrypted-repo refusal mentions "passphrase" but is not a
            // wrong-passphrase verdict — blocking on it would be nonsense.
            "This repository is not encrypted, cannot change the passphrase.",
        ] {
            assert!(!looks_like_wrong_passphrase(unrelated), "{unrelated}");
        }
    }

    /// A damaged repository must never be reported as a wrong passphrase. The
    /// user would be repairing it with the correct passphrase in hand, be told
    /// it is wrong, and could discard the only copy they have.
    #[test]
    fn a_damaged_repository_is_not_reported_as_a_wrong_passphrase() {
        for corrupt in [
            "borgbackup.helpers.IntegrityError: Data integrity error: Decryption error",
            "Remote Exception (IntegrityError)",
            "Data integrity error: chunk id verification failed",
        ] {
            assert!(!looks_like_wrong_passphrase(corrupt), "{corrupt}");
        }
    }
}
