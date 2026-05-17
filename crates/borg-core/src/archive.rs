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
