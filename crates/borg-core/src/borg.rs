use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::config::{BackupProfile, RepoConfig};
use crate::error::{BorgError, Result};
use crate::progress::ProgressEvent;

pub struct BorgClient {
    binary_path: PathBuf,
    passcommand: Option<String>,
}

impl BorgClient {
    pub fn new(binary_path: PathBuf) -> Self {
        Self {
            binary_path,
            passcommand: None,
        }
    }

    pub fn with_passcommand(mut self, cmd: String) -> Self {
        self.passcommand = Some(cmd);
        self
    }

    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }

    fn base_command(&self) -> Command {
        self.base_command_with(None)
    }

    fn base_command_with(&self, passphrase: Option<&str>) -> Command {
        let mut cmd = Command::new(&self.binary_path);
        if let Some(ref passcommand) = self.passcommand {
            cmd.env("BORG_PASSCOMMAND", passcommand);
        }
        if let Some(p) = passphrase {
            cmd.env("BORG_PASSPHRASE", p);
        }
        cmd.env("BORG_RELOCATED_REPO_ACCESS_IS_OK", "yes");
        cmd
    }

    pub async fn version(&self) -> Result<String> {
        let output = self.base_command().arg("--version").output().await?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub async fn info(
        &self,
        repo: &RepoConfig,
        passphrase: Option<&str>,
    ) -> Result<serde_json::Value> {
        let output = self
            .base_command_with(passphrase)
            .args(["info", "--json", &repo.ssh_url()])
            .output()
            .await?;

        if !output.status.success() {
            return Err(BorgError::ProcessFailed {
                message: "borg info failed".into(),
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }

        Ok(serde_json::from_slice(&output.stdout)?)
    }

    pub async fn create(
        &self,
        profile: &BackupProfile,
        archive_name: &str,
        passphrase: Option<&str>,
        on_progress: impl Fn(ProgressEvent) + Send + 'static,
    ) -> Result<()> {
        let repo_url = profile.repo.ssh_url();
        let archive = format!("{}::{}", repo_url, archive_name);

        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["create", "--json", "--progress", "--log-json"]);

        let compression = profile.compression.to_borg_arg();
        cmd.args(["--compression", &compression]);

        for exclude in &profile.excludes {
            cmd.args(["--exclude", exclude]);
        }

        cmd.arg(&archive);
        for path in &profile.source_paths {
            cmd.arg(path);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stderr = child.stderr.take().expect("stderr was piped");
        let mut reader = BufReader::new(stderr).lines();

        let stderr_capture: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_clone = stderr_capture.clone();

        let reader_task = tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(event) = serde_json::from_str::<ProgressEvent>(&line) {
                    on_progress(event);
                } else {
                    debug!("borg stderr: {}", line);
                }
                stderr_clone
                    .lock()
                    .expect("stderr mutex poisoned")
                    .push(line);
            }
        });

        let status = child.wait().await?;
        let _ = reader_task.await;

        if !status.success() {
            let captured = stderr_capture
                .lock()
                .expect("stderr mutex poisoned")
                .join("\n");
            return Err(BorgError::ProcessFailed {
                message: "borg create failed".into(),
                exit_code: status.code(),
                stderr: captured,
            });
        }

        Ok(())
    }

    pub async fn extract(
        &self,
        repo: &RepoConfig,
        archive_name: &str,
        destination: &Path,
        passphrase: Option<&str>,
        on_progress: impl Fn(ProgressEvent) + Send + 'static,
    ) -> Result<()> {
        let archive = format!("{}::{}", repo.ssh_url(), archive_name);

        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["extract", "--progress", "--log-json"]);
        cmd.arg(&archive);
        cmd.current_dir(destination);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stderr = child.stderr.take().expect("stderr was piped");
        let mut reader = BufReader::new(stderr).lines();

        let stderr_capture: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_clone = stderr_capture.clone();

        let reader_task = tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(event) = serde_json::from_str::<ProgressEvent>(&line) {
                    on_progress(event);
                } else {
                    debug!("borg stderr: {}", line);
                }
                stderr_clone
                    .lock()
                    .expect("stderr mutex poisoned")
                    .push(line);
            }
        });

        let status = child.wait().await?;
        let _ = reader_task.await;

        if !status.success() {
            let captured = stderr_capture
                .lock()
                .expect("stderr mutex poisoned")
                .join("\n");
            return Err(BorgError::ProcessFailed {
                message: "borg extract failed".into(),
                exit_code: status.code(),
                stderr: captured,
            });
        }

        Ok(())
    }

    pub async fn prune(
        &self,
        repo: &RepoConfig,
        retention: &crate::config::RetentionConfig,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let mut cmd = self.base_command_with(passphrase);
        cmd.arg("prune");

        if let Some(n) = retention.keep_hourly {
            cmd.args(["--keep-hourly", &n.to_string()]);
        }
        if let Some(n) = retention.keep_daily {
            cmd.args(["--keep-daily", &n.to_string()]);
        }
        if let Some(n) = retention.keep_weekly {
            cmd.args(["--keep-weekly", &n.to_string()]);
        }
        if let Some(n) = retention.keep_monthly {
            cmd.args(["--keep-monthly", &n.to_string()]);
        }
        if let Some(n) = retention.keep_yearly {
            cmd.args(["--keep-yearly", &n.to_string()]);
        }

        cmd.arg(repo.ssh_url());

        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(BorgError::ProcessFailed {
                message: "borg prune failed".into(),
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }

        Ok(())
    }

    pub async fn init_repo(
        &self,
        repo: &RepoConfig,
        encryption: &str,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let mut cmd = self.base_command();
        cmd.args(["init", "--encryption", encryption, &repo.ssh_url()]);

        if let Some(pass) = passphrase {
            cmd.env("BORG_PASSPHRASE", pass);
            cmd.env("BORG_NEW_PASSPHRASE", pass);
        }

        let output = cmd.output().await?;

        if !output.status.success() {
            return Err(BorgError::ProcessFailed {
                message: "borg init failed".into(),
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }

        Ok(())
    }

    pub async fn delete_archive(
        &self,
        repo: &RepoConfig,
        archive_name: &str,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let archive = format!("{}::{}", repo.ssh_url(), archive_name);
        let output = self
            .base_command_with(passphrase)
            .args(["delete", &archive])
            .output()
            .await?;

        if !output.status.success() {
            return Err(BorgError::ProcessFailed {
                message: "borg delete failed".into(),
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }

        Ok(())
    }

    pub async fn list_archives(
        &self,
        repo: &RepoConfig,
        passphrase: Option<&str>,
    ) -> Result<Vec<ArchiveInfo>> {
        let output = self
            .base_command_with(passphrase)
            .args(["list", "--json", &repo.ssh_url()])
            .output()
            .await?;

        if !output.status.success() {
            return Err(BorgError::ProcessFailed {
                message: "borg list failed".into(),
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }

        let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let archives = match parsed["archives"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|a| {
                    Some(ArchiveInfo {
                        name: a["name"].as_str()?.to_string(),
                        start: a["start"].as_str()?.to_string(),
                        id: a["id"].as_str()?.to_string(),
                    })
                })
                .collect(),
            None => {
                warn!("borg list output missing 'archives' array");
                vec![]
            }
        };

        Ok(archives)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArchiveInfo {
    pub name: String,
    pub start: String,
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RepoConfig;

    fn test_repo() -> RepoConfig {
        RepoConfig {
            ssh_host: "backup.example.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/data/repo".into(),
            ssh_key_path: None,
        }
    }

    #[test]
    fn client_new_stores_binary_path() {
        let client = BorgClient::new(PathBuf::from("/usr/bin/borg"));
        assert_eq!(client.binary_path(), Path::new("/usr/bin/borg"));
    }

    #[test]
    fn client_with_passcommand_sets_field() {
        let client = BorgClient::new(PathBuf::from("borg")).with_passcommand("cat /secret".into());
        assert_eq!(client.passcommand.as_deref(), Some("cat /secret"));
    }

    #[test]
    fn client_without_passcommand_is_none() {
        let client = BorgClient::new(PathBuf::from("borg"));
        assert!(client.passcommand.is_none());
    }

    #[test]
    fn archive_info_deserializes() {
        let json = r#"{"name":"backup-2024","start":"2024-01-15T10:00:00","id":"abc123"}"#;
        let info: ArchiveInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "backup-2024");
        assert_eq!(info.start, "2024-01-15T10:00:00");
        assert_eq!(info.id, "abc123");
    }

    #[test]
    fn archive_info_roundtrip() {
        let info = ArchiveInfo {
            name: "daily-2024".into(),
            start: "2024-06-01T12:00:00".into(),
            id: "deadbeef".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: ArchiveInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, info.name);
        assert_eq!(parsed.id, info.id);
    }

    #[test]
    fn archive_info_rejects_missing_field() {
        let json = r#"{"name":"backup","start":"2024-01-01T00:00:00"}"#;
        assert!(serde_json::from_str::<ArchiveInfo>(json).is_err());
    }

    #[test]
    fn archive_url_format() {
        let repo = test_repo();
        let archive = format!("{}::{}", repo.ssh_url(), "my-backup");
        assert_eq!(
            archive,
            "ssh://borg@backup.example.com:22//data/repo::my-backup"
        );
    }

    #[test]
    fn parses_borg_list_json_output() {
        let json = r#"{
            "archives": [
                {"name": "backup-1", "start": "2024-01-01T00:00:00", "id": "aaa"},
                {"name": "backup-2", "start": "2024-01-02T00:00:00", "id": "bbb"}
            ]
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let archives: Vec<ArchiveInfo> = parsed["archives"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| {
                Some(ArchiveInfo {
                    name: a["name"].as_str()?.to_string(),
                    start: a["start"].as_str()?.to_string(),
                    id: a["id"].as_str()?.to_string(),
                })
            })
            .collect();
        assert_eq!(archives.len(), 2);
        assert_eq!(archives[0].name, "backup-1");
        assert_eq!(archives[1].id, "bbb");
    }

    #[test]
    fn missing_archives_array_returns_empty() {
        let json = r#"{"repository": {"id": "abc"}}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let archives: Vec<ArchiveInfo> = match parsed["archives"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|a| {
                    Some(ArchiveInfo {
                        name: a["name"].as_str()?.to_string(),
                        start: a["start"].as_str()?.to_string(),
                        id: a["id"].as_str()?.to_string(),
                    })
                })
                .collect(),
            None => vec![],
        };
        assert!(archives.is_empty());
    }

    #[test]
    fn skips_archive_entries_with_missing_fields() {
        let json = r#"{
            "archives": [
                {"name": "good", "start": "2024-01-01T00:00:00", "id": "aaa"},
                {"name": "no-id", "start": "2024-01-02T00:00:00"},
                {"start": "2024-01-03T00:00:00", "id": "ccc"}
            ]
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let archives: Vec<ArchiveInfo> = parsed["archives"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| {
                Some(ArchiveInfo {
                    name: a["name"].as_str()?.to_string(),
                    start: a["start"].as_str()?.to_string(),
                    id: a["id"].as_str()?.to_string(),
                })
            })
            .collect();
        assert_eq!(archives.len(), 1);
        assert_eq!(archives[0].name, "good");
    }

    #[test]
    fn base_command_sets_relocated_env() {
        let client = BorgClient::new(PathBuf::from("borg"));
        let cmd = client.base_command();
        let envs: Vec<_> = cmd.as_std().get_envs().collect();
        let relocated = envs
            .iter()
            .find(|(k, _)| *k == "BORG_RELOCATED_REPO_ACCESS_IS_OK");
        assert!(relocated.is_some());
    }

    #[test]
    fn base_command_sets_passcommand_env() {
        let client = BorgClient::new(PathBuf::from("borg")).with_passcommand("echo secret".into());
        let cmd = client.base_command();
        let envs: Vec<_> = cmd.as_std().get_envs().collect();
        let passcommand = envs.iter().find(|(k, _)| *k == "BORG_PASSCOMMAND");
        assert!(passcommand.is_some());
    }

    #[test]
    fn base_command_without_passcommand_skips_env() {
        let client = BorgClient::new(PathBuf::from("borg"));
        let cmd = client.base_command();
        let envs: Vec<_> = cmd.as_std().get_envs().collect();
        let passcommand = envs.iter().find(|(k, _)| *k == "BORG_PASSCOMMAND");
        assert!(passcommand.is_none());
    }
}
