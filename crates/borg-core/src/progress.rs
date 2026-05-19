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
    LogMessage { levelname: String, message: String },

    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_archive_progress() {
        let json = r#"{"type":"archive_progress","original_size":1024,"compressed_size":512,"deduplicated_size":256,"nfiles":10,"path":"/home/user/doc.txt"}"#;
        let event: ProgressEvent = serde_json::from_str(json).unwrap();
        match event {
            ProgressEvent::ArchiveProgress {
                original_size,
                nfiles,
                ..
            } => {
                assert_eq!(original_size, Some(1024));
                assert_eq!(nfiles, Some(10));
            }
            _ => panic!("expected ArchiveProgress"),
        }
    }

    #[test]
    fn deserializes_percent_progress() {
        let json = r#"{"type":"progress_percent","finished":false,"message":"Backing up","current":50,"total":100}"#;
        let event: ProgressEvent = serde_json::from_str(json).unwrap();
        match event {
            ProgressEvent::Percent {
                finished,
                current,
                total,
                ..
            } => {
                assert!(!finished);
                assert_eq!(current, Some(50));
                assert_eq!(total, Some(100));
            }
            _ => panic!("expected Percent"),
        }
    }

    #[test]
    fn deserializes_log_message() {
        let json = r#"{"type":"log_message","levelname":"WARNING","message":"something happened"}"#;
        let event: ProgressEvent = serde_json::from_str(json).unwrap();
        match event {
            ProgressEvent::LogMessage { levelname, message } => {
                assert_eq!(levelname, "WARNING");
                assert_eq!(message, "something happened");
            }
            _ => panic!("expected LogMessage"),
        }
    }

    #[test]
    fn unknown_type_deserializes_as_unknown() {
        let json = r#"{"type":"something_new","data":123}"#;
        let event: ProgressEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, ProgressEvent::Unknown));
    }

    #[test]
    fn archive_progress_with_missing_optional_fields() {
        let json = r#"{"type":"archive_progress"}"#;
        let event: ProgressEvent = serde_json::from_str(json).unwrap();
        match event {
            ProgressEvent::ArchiveProgress {
                original_size,
                path,
                ..
            } => {
                assert_eq!(original_size, None);
                assert_eq!(path, None);
            }
            _ => panic!("expected ArchiveProgress"),
        }
    }
}
