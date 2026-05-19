use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::debug;

use crate::error::{BorgError, Result};

pub async fn test_connection(
    host: &str,
    port: u16,
    user: &str,
    key_path: Option<&Path>,
) -> Result<bool> {
    let mut cmd = Command::new("ssh");
    cmd.args(["-o", "BatchMode=yes"])
        .args(["-o", "ConnectTimeout=10"])
        .args(["-p", &port.to_string()]);

    if let Some(key) = key_path {
        cmd.args(["-i", &key.to_string_lossy()]);
    }

    cmd.arg(format!("{}@{}", user, host)).arg("echo ok");

    let output = cmd.output().await?;
    Ok(output.status.success())
}

pub async fn generate_key(path: &Path) -> Result<PathBuf> {
    let output = Command::new("ssh-keygen")
        .args(["-t", "ed25519"])
        .args(["-f", &path.to_string_lossy()])
        .args(["-N", ""])
        .args(["-C", "borgui-backup-key"])
        .output()
        .await?;

    if !output.status.success() {
        return Err(BorgError::SshFailed {
            message: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    debug!("generated SSH key at {:?}", path);
    Ok(path.to_path_buf())
}

pub async fn read_public_key(private_key_path: &Path) -> Result<String> {
    let pub_path = private_key_path.with_extension("pub");
    tokio::fs::read_to_string(&pub_path)
        .await
        .map_err(|e| BorgError::SshFailed {
            message: format!("failed to read public key {:?}: {}", pub_path, e),
        })
}
