use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProgressEvent {
    #[serde(rename = "archive_progress")]
    ArchiveProgress {
        original_size: Option<u64>,
        compressed_size: Option<u64>,
        deduplicated_size: Option<u64>,
        nfiles: Option<u64>,
        path: Option<String>,
    },

    #[serde(rename = "progress_percent")]
    Percent {
        finished: bool,
        message: Option<String>,
        current: Option<u64>,
        total: Option<u64>,
    },

    #[serde(rename = "log_message")]
    LogMessage {
        levelname: String,
        message: String,
    },

    #[serde(other)]
    Unknown,
}
