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
