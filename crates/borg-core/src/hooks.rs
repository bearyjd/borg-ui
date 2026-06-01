//! Pre/post-backup command hooks.
//!
//! A hook is a user-authored shell command run immediately before or after a
//! backup (e.g. dump a database, then back it up; or notify a service on
//! completion). It is the user's own configuration, run on their own machine
//! through the platform shell, so shell semantics (pipes, redirection) are
//! intentional. The substitution variables (`$repo_url`, `$archive_name`) are
//! already validated upstream — archive names are alphanumeric and repo
//! locations reject shell metacharacters — so expansion can't inject anything
//! the user didn't type. No external or untrusted input reaches the shell.

use std::process::Stdio;
use std::time::Duration;

use crate::error::{BorgError, Result};
use crate::proc;

/// Safety backstop so a hung hook can't freeze a backup indefinitely. Generous,
/// because a legitimate pre-backup step (a large DB dump) can take a while.
const HOOK_TIMEOUT_SECS: u64 = 3600;

/// Values substituted into a hook command before it runs.
pub struct HookContext<'a> {
    pub repo_url: &'a str,
    pub archive_name: &'a str,
}

/// Substitute the supported variables into a hook command template. Longer
/// variable names are replaced first so no prefix shadows another.
pub fn expand(template: &str, ctx: &HookContext<'_>) -> String {
    template
        .replace("$archive_name", ctx.archive_name)
        .replace("$repo_url", ctx.repo_url)
}

/// Run a hook command through the platform shell after variable expansion.
/// `label` (e.g. "pre-backup") is used only in error messages. Returns the
/// trimmed combined output on success; a non-zero exit becomes
/// [`BorgError::ProcessFailed`] carrying that output.
pub async fn run(label: &str, command: &str, ctx: &HookContext<'_>) -> Result<String> {
    let expanded = expand(command, ctx);

    #[cfg(windows)]
    let mut cmd = {
        let mut c = proc::command("cmd");
        c.args(["/C", &expanded]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = proc::command("sh");
        c.args(["-c", &expanded]);
        c
    };

    // Hooks are non-interactive; close stdin so anything that reads it gets EOF
    // instead of blocking the backup.
    cmd.stdin(Stdio::null());

    let output = tokio::time::timeout(Duration::from_secs(HOOK_TIMEOUT_SECS), cmd.output())
        .await
        .map_err(|_| BorgError::Timeout {
            seconds: HOOK_TIMEOUT_SECS,
        })??;

    let combined = {
        let mut s = String::from_utf8_lossy(&output.stdout).into_owned();
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.trim().is_empty() {
            if !s.trim().is_empty() {
                s.push('\n');
            }
            s.push_str(&err);
        }
        s.trim().to_string()
    };

    if output.status.success() {
        Ok(combined)
    } else {
        Err(BorgError::ProcessFailed {
            message: format!("{label} command failed"),
            exit_code: output.status.code(),
            stderr: combined,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> HookContext<'static> {
        HookContext {
            repo_url: "/mnt/usb/repo",
            archive_name: "daily-2026",
        }
    }

    #[test]
    fn expand_substitutes_both_variables() {
        let out = expand("backup $archive_name to $repo_url", &ctx());
        assert_eq!(out, "backup daily-2026 to /mnt/usb/repo");
    }

    #[test]
    fn expand_leaves_unknown_tokens_untouched() {
        let out = expand("echo $not_a_var $archive_name", &ctx());
        assert_eq!(out, "echo $not_a_var daily-2026");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_returns_output_with_expanded_variables() {
        let out = run("pre-backup", "echo got $archive_name", &ctx())
            .await
            .expect("a successful hook should not error");
        assert_eq!(out, "got daily-2026");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_errors_on_nonzero_exit() {
        let err = run("post-backup", "echo boom >&2; exit 7", &ctx())
            .await
            .expect_err("a failing hook must error");
        match err {
            BorgError::ProcessFailed {
                message,
                exit_code,
                stderr,
            } => {
                assert!(message.contains("post-backup"));
                assert_eq!(exit_code, Some(7));
                assert!(stderr.contains("boom"));
            }
            other => panic!("expected ProcessFailed, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_executes_side_effects_through_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran.txt");
        let cmd = format!("echo $archive_name > {}", marker.display());
        run("pre-backup", &cmd, &ctx()).await.unwrap();
        let contents = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(contents.trim(), "daily-2026");
    }
}
