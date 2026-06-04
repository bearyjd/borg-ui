use thiserror::Error;

#[derive(Debug, Error)]
pub enum BorgError {
    #[error("borg process failed: {message}")]
    ProcessFailed {
        message: String,
        exit_code: Option<i32>,
        stderr: String,
    },

    #[error("repository not found: {path}")]
    RepoNotFound { path: String },

    #[error("authentication failed: {reason}")]
    AuthFailed { reason: String },

    #[error("passphrase required but not provided")]
    PassphraseRequired,

    #[error("SSH connection failed: {message}")]
    SshFailed { message: String },

    #[error("invalid configuration: {message}")]
    InvalidConfig { message: String },

    // NOTE: the frontend detects user-cancellation by matching the substring
    // "operation cancelled" in the error string (see backup/+page.svelte and
    // archives/+page.svelte). Do not reword this message without updating those
    // call sites, or cancelled operations will be recorded as failures.
    #[error("operation cancelled")]
    Cancelled,

    #[error("operation timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, BorgError>;

impl BorgError {
    /// A verbose, log-friendly description that includes captured process
    /// `stderr` for [`ProcessFailed`](Self::ProcessFailed) — unlike `Display`,
    /// which stays terse for UI surfaces. Use this where a failure is recorded
    /// without a live console to inspect (e.g. the scheduled-backup history), so
    /// the underlying borg error isn't lost. The stderr tail is trimmed and
    /// length-capped so a record carries the real error without dumping an entire
    /// progress log.
    pub fn detail(&self) -> String {
        match self {
            BorgError::ProcessFailed {
                exit_code, stderr, ..
            } => {
                // Reuse Display ("borg process failed: {message}"), then append
                // the exit code and the meaningful tail of stderr when present.
                let mut out = self.to_string();
                if let Some(code) = exit_code {
                    out.push_str(&format!(" (exit {code})"));
                }
                let tail = stderr_tail(stderr);
                if !tail.is_empty() {
                    out.push_str(": ");
                    out.push_str(&tail);
                }
                out
            }
            other => other.to_string(),
        }
    }
}

/// The last few non-empty lines of captured stderr, length-capped — the real
/// error is usually at the end (a failing op stops emitting progress first).
fn stderr_tail(stderr: &str) -> String {
    // 12 (not 8): borg `--log-json` can emit a trailing summary/progress line
    // after the ERROR line, and the PyInstaller failure is two lines; keep enough
    // tail that the actual error can't be pushed out.
    const MAX_LINES: usize = 12;
    const MAX_CHARS: usize = 600;
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let start = lines.len().saturating_sub(MAX_LINES);
    let joined = lines[start..].join("; ");
    if joined.chars().count() > MAX_CHARS {
        let truncated: String = joined.chars().take(MAX_CHARS).collect();
        format!("{truncated}...")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_failed_displays_message() {
        let err = BorgError::ProcessFailed {
            message: "borg create failed".into(),
            exit_code: Some(2),
            stderr: "permission denied".into(),
        };
        assert!(err.to_string().contains("borg create failed"));
    }

    #[test]
    fn detail_includes_stderr_tail_and_exit_code() {
        let err = BorgError::ProcessFailed {
            message: "borg create failed".into(),
            exit_code: Some(2),
            stderr: "progress noise\n\n[PYI] Failed to load Python DLL python311.dll\nLoadLibrary: not found".into(),
        };
        let detail = err.detail();
        assert!(detail.contains("borg create failed"));
        assert!(detail.contains("exit 2"));
        assert!(detail.contains("Failed to load Python DLL"));
        assert!(detail.contains("LoadLibrary: not found"));
        // Display stays terse — stderr only shows up in detail().
        assert!(!err.to_string().contains("Failed to load Python DLL"));
    }

    #[test]
    fn detail_surfaces_message_from_json_log_line() {
        // The streaming create path captures borg's `--log-json` stderr; detail()
        // must still surface the human-readable message embedded in the JSON.
        let err = BorgError::ProcessFailed {
            message: "borg create failed".into(),
            exit_code: Some(2),
            stderr: r#"{"type": "log_message", "message": "Repository /repo does not exist.", "levelname": "ERROR", "msgid": "Repository.DoesNotExist"}"#.into(),
        };
        let detail = err.detail();
        assert!(detail.contains("does not exist"));
        assert!(detail.contains("Repository.DoesNotExist"));
    }

    #[test]
    fn stderr_tail_keeps_last_lines() {
        // > MAX_LINES: keeps the LAST lines, drops the earliest.
        let many = (1..=30)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let err = BorgError::ProcessFailed {
            message: "x".into(),
            exit_code: None,
            stderr: many,
        };
        let detail = err.detail();
        assert!(detail.contains("line30"), "should keep the last line");
        assert!(
            !detail.contains("line1;") && !detail.contains("line5;"),
            "should drop the earliest lines: {detail}"
        );
    }

    #[test]
    fn stderr_tail_caps_length_with_ellipsis() {
        let err = BorgError::ProcessFailed {
            message: "x".into(),
            exit_code: None,
            stderr: "y".repeat(2000),
        };
        let detail = err.detail();
        assert!(detail.ends_with("..."), "over-long stderr is truncated");
        // message + " (no exit) " prefix is tiny; the capped tail dominates.
        assert!(
            detail.chars().count() < 700,
            "stays bounded: {}",
            detail.len()
        );
    }

    #[test]
    fn detail_falls_back_to_display_without_stderr() {
        let err = BorgError::ProcessFailed {
            message: "borg init failed".into(),
            exit_code: None,
            stderr: String::new(),
        };
        assert_eq!(err.detail(), "borg process failed: borg init failed");

        let cfg = BorgError::InvalidConfig {
            message: "bad path".into(),
        };
        assert_eq!(cfg.detail(), cfg.to_string());
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let borg_err: BorgError = io_err.into();
        assert!(borg_err.to_string().contains("file not found"));
    }

    #[test]
    fn json_error_converts() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let borg_err: BorgError = json_err.into();
        assert!(matches!(borg_err, BorgError::Json(_)));
    }

    #[test]
    fn all_error_variants_display() {
        let errors: Vec<BorgError> = vec![
            BorgError::RepoNotFound {
                path: "/repo".into(),
            },
            BorgError::AuthFailed {
                reason: "bad key".into(),
            },
            BorgError::PassphraseRequired,
            BorgError::SshFailed {
                message: "timeout".into(),
            },
            BorgError::InvalidConfig {
                message: "missing field".into(),
            },
        ];
        for err in errors {
            assert!(!err.to_string().is_empty());
        }
    }
}
