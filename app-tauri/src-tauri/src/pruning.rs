//! Profile-scoped retention pruning.
//!
//! This is the single sink for every prune in the app (manual prune from
//! Settings, auto-prune after a manual backup, auto-prune after a scheduled
//! backup). Pruning is always scoped with `--glob-archives` to the archives
//! the profile's naming template produces, so one profile's retention policy
//! can never delete archives created by another profile or machine sharing
//! the repository.
//!
//! Backwards compatibility with pre-0.3 archives (no prefix):
//! - Archives named by the old default template (`{datetime}-{random}`, e.g.
//!   `2026-05-24T143015-ab12`) do not match the scoped glob. On a shared
//!   repository they are indistinguishable from *another machine's* legacy
//!   archives, so widening the glob (or renaming them) automatically could
//!   itself cause cross-machine data loss. Instead they are left untouched
//!   and every prune that skips them reports a warning telling the user to
//!   clean them up manually — legacy archives never accumulate silently.
//! - Profiles with a custom template keep working: the glob is derived from
//!   the template with stable variables expanded and time-varying ones
//!   wildcarded, so it also matches the profile's historical archives.
//! - A custom template with no unique prefix (e.g. the old default typed in
//!   by hand) cannot be scoped at all; pruning then keeps the historical
//!   repository-wide behaviour (glob `*`) but warns that retention applies
//!   to every archive in the repository.

use borg_core::borg::{BorgClient, OpOutcome};
use borg_core::config::{RepoConfig, RetentionConfig};
use borg_core::error::Result;

use crate::archive_naming;

/// Resolves the `--glob-archives` scope for a profile, or `None` when the
/// profile's archive template has no unique prefix to scope on.
pub fn profile_prune_glob(archive_template: Option<&str>, profile_name: &str) -> Option<String> {
    let template = archive_template
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(archive_naming::DEFAULT_TEMPLATE);
    let hostname = archive_naming::current_hostname();
    archive_naming::prune_glob(template, &hostname, profile_name)
}

/// Prunes `repo` per `retention`, scoped to the archives produced by the
/// given profile's naming template. See the module docs for the exact
/// behaviour, including how pre-prefix legacy archives are handled.
pub async fn prune_scoped(
    borg: &BorgClient,
    repo: &RepoConfig,
    retention: &RetentionConfig,
    passphrase: Option<&str>,
    archive_template: Option<&str>,
    profile_name: &str,
) -> Result<OpOutcome> {
    let Some(glob) = profile_prune_glob(archive_template, profile_name) else {
        // No prefix to scope on (explicit user template): keep the historical
        // repository-wide prune, but never do so silently.
        let mut outcome = borg.prune(repo, retention, "*", passphrase).await?;
        outcome.warnings.push(
            "archive template has no unique profile prefix; retention pruning applies to \
             every archive in the repository, including other machines' backups"
                .into(),
        );
        return Ok(outcome);
    };

    let mut outcome = borg.prune(repo, retention, &glob, passphrase).await?;

    // Archives from the pre-0.3 unprefixed default template fall outside the
    // scoped glob and are deliberately never pruned (see module docs). Count
    // them and warn so they cannot pile up unnoticed. A listing failure only
    // skips the warning — the prune itself already succeeded.
    if let Ok(archives) = borg.list_archives(repo, passphrase).await {
        let legacy = archives
            .iter()
            .filter(|a| archive_naming::is_legacy_default_archive_name(&a.name))
            .count();
        if legacy > 0 {
            outcome.warnings.push(format!(
                "{legacy} archive(s) named by the old default template (no profile prefix) \
                 are excluded from retention pruning; review and delete them from the \
                 Archives page when no longer needed"
            ));
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_template_yields_a_scoped_glob() {
        let glob = profile_prune_glob(None, "default").expect("default template must scope");
        assert!(
            !glob.starts_with('*'),
            "glob must carry a discriminating prefix, got {glob:?}"
        );
        assert!(
            glob.contains("default"),
            "glob should embed the profile slug"
        );
        assert!(glob.ends_with('*'), "glob must match any timestamp/suffix");
    }

    #[test]
    fn blank_template_falls_back_to_default() {
        assert_eq!(
            profile_prune_glob(Some("   "), "default"),
            profile_prune_glob(None, "default")
        );
    }

    #[test]
    fn unprefixed_custom_template_cannot_be_scoped() {
        // A user who typed the old default back in gets repository-wide
        // pruning plus a warning (see prune_scoped), never a silent scope.
        assert_eq!(
            profile_prune_glob(Some("{datetime}-{random}"), "default"),
            None
        );
    }

    #[test]
    fn custom_prefixed_template_is_scoped() {
        assert_eq!(
            profile_prune_glob(Some("laptop-{date}-{random}"), "default").as_deref(),
            Some("laptop-*-*")
        );
    }
}
