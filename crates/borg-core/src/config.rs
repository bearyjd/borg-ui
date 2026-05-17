use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub repo_path: String,
    pub ssh_key_path: Option<PathBuf>,
}

impl RepoConfig {
    pub fn ssh_url(&self) -> String {
        format!(
            "ssh://{}@{}:{}/{}",
            self.ssh_user, self.ssh_host, self.ssh_port, self.repo_path
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupProfile {
    pub name: String,
    pub source_paths: Vec<PathBuf>,
    pub excludes: Vec<String>,
    pub compression: Compression,
    pub repo: RepoConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Compression {
    None,
    Lz4,
    Zstd { level: u8 },
    Zlib { level: u8 },
}

impl Default for Compression {
    fn default() -> Self {
        Self::Zstd { level: 3 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub profiles: Vec<BackupProfile>,
    pub borg_binary_path: PathBuf,
}
