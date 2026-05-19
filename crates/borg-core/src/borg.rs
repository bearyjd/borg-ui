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
        let mut cmd = Command::new(&self.binary_path);
        if let Some(ref passcommand) = self.passcommand {
            cmd.env("BORG_PASSCOMMAND", passcommand);
        }
        cmd.env("BORG_RELOCATED_REPO_ACCESS_IS_OK", "yes");
        cmd
    }

    pub async fn version(&self) -> Result<String> {
        let output = self.base_command().arg("--version").output().await?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub async fn info(&self, repo: &RepoConfig) -> Result<serde_json::Value> {
        let output = self
            .base_command()
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
        on_progress: impl Fn(ProgressEvent) + Send + 'static,
    ) -> Result<()> {
        let repo_url = profile.repo.ssh_url();
        let archive = format!("{}::{}", repo_url, archive_name);

        let mut cmd = self.base_command();
        cmd.args(["create", "--json", "--progress", "--log-json"]);

        let compression = match &profile.compression {
            crate::config::Compression::None => "none".to_string(),
            crate::config::Compression::Lz4 => "lz4".to_string(),
            crate::config::Compression::Zstd { level } => format!("zstd,{}", level),
            crate::config::Compression::Zlib { level } => format!("zlib,{}", level),
        };
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

    pub async fn list_archives(&self, repo: &RepoConfig) -> Result<Vec<ArchiveInfo>> {
        let output = self
            .base_command()
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
