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

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, BorgError>;
