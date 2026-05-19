use serde::{Deserialize, Serialize};

use crate::borg::BorgClient;
use crate::config::RepoConfig;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub path: String,
    pub mode: String,
    pub size: u64,
    #[serde(rename = "type")]
    pub entry_type: String,
}

pub async fn list_archive_contents(
    client: &BorgClient,
    repo: &RepoConfig,
    archive_name: &str,
) -> Result<Vec<ArchiveEntry>> {
    let repo_url = repo.ssh_url();
    let archive = format!("{}::{}", repo_url, archive_name);

    let output = tokio::process::Command::new(client.binary_path())
        .args(["list", "--json-lines", &archive])
        .output()
        .await?;

    let entries = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<ArchiveEntry>(line).ok())
        .collect();

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_file_entry() {
        let json = r#"{"path":"docs/readme.txt","mode":"-rw-r--r--","size":1024,"type":"f"}"#;
        let entry: ArchiveEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.path, "docs/readme.txt");
        assert_eq!(entry.mode, "-rw-r--r--");
        assert_eq!(entry.size, 1024);
        assert_eq!(entry.entry_type, "f");
    }

    #[test]
    fn deserializes_directory_entry() {
        let json = r#"{"path":"docs","mode":"drwxr-xr-x","size":0,"type":"d"}"#;
        let entry: ArchiveEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.entry_type, "d");
        assert_eq!(entry.size, 0);
    }

    #[test]
    fn deserializes_symlink_entry() {
        let json = r#"{"path":"link","mode":"lrwxrwxrwx","size":11,"type":"l"}"#;
        let entry: ArchiveEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.entry_type, "l");
    }

    #[test]
    fn roundtrip_json() {
        let entry = ArchiveEntry {
            path: "test/file.rs".into(),
            mode: "-rw-r--r--".into(),
            size: 512,
            entry_type: "f".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: ArchiveEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.path, entry.path);
        assert_eq!(parsed.size, entry.size);
    }

    #[test]
    fn parses_json_lines() {
        let lines = r#"{"path":"a.txt","mode":"-rw-r--r--","size":10,"type":"f"}
{"path":"b.txt","mode":"-rw-r--r--","size":20,"type":"f"}
{"path":"dir","mode":"drwxr-xr-x","size":0,"type":"d"}"#;

        let entries: Vec<ArchiveEntry> = lines
            .lines()
            .filter_map(|line| serde_json::from_str::<ArchiveEntry>(line).ok())
            .collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, "a.txt");
        assert_eq!(entries[2].entry_type, "d");
    }

    #[test]
    fn skips_malformed_json_lines() {
        let lines = r#"{"path":"good.txt","mode":"-rw-r--r--","size":10,"type":"f"}
not valid json
{"path":"also-good.txt","mode":"-rw-r--r--","size":20,"type":"f"}"#;

        let entries: Vec<ArchiveEntry> = lines
            .lines()
            .filter_map(|line| serde_json::from_str::<ArchiveEntry>(line).ok())
            .collect();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn empty_input_returns_empty() {
        let entries: Vec<ArchiveEntry> = ""
            .lines()
            .filter_map(|line| serde_json::from_str::<ArchiveEntry>(line).ok())
            .collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn handles_unicode_paths() {
        let json = r#"{"path":"文档/日本語.txt","mode":"-rw-r--r--","size":100,"type":"f"}"#;
        let entry: ArchiveEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.path, "文档/日本語.txt");
    }

    #[test]
    fn rejects_missing_required_field() {
        let json = r#"{"path":"test.txt","mode":"-rw-r--r--","size":10}"#;
        assert!(serde_json::from_str::<ArchiveEntry>(json).is_err());
    }
}
