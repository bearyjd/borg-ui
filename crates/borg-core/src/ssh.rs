use std::path::{Path, PathBuf};
use tracing::debug;

use crate::proc;

use crate::error::{BorgError, Result};

pub async fn test_connection(
    host: &str,
    port: u16,
    user: &str,
    key_path: Option<&Path>,
) -> Result<bool> {
    let mut cmd = proc::command("ssh");
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
    let output = proc::command("ssh-keygen")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generate_key_creates_keypair() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");

        let result = generate_key(&key_path).await.unwrap();

        assert_eq!(result, key_path);
        assert!(key_path.exists());
        assert!(key_path.with_extension("pub").exists());
    }

    #[tokio::test]
    async fn generate_key_creates_ed25519() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path).await.unwrap();

        let pub_content = tokio::fs::read_to_string(key_path.with_extension("pub"))
            .await
            .unwrap();
        assert!(pub_content.starts_with("ssh-ed25519 "));
    }

    #[tokio::test]
    async fn generate_key_includes_comment() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path).await.unwrap();

        let pub_content = tokio::fs::read_to_string(key_path.with_extension("pub"))
            .await
            .unwrap();
        assert!(pub_content.contains("borgui-backup-key"));
    }

    #[tokio::test]
    async fn generate_key_fails_on_invalid_path() {
        let result = generate_key(Path::new("/nonexistent/dir/key")).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, BorgError::SshFailed { .. }));
    }

    #[tokio::test]
    async fn read_public_key_returns_content() {
        let dir = tempfile::tempdir().unwrap();
        let pub_path = dir.path().join("test_key.pub");
        let expected = "ssh-ed25519 AAAA... borgui-backup-key\n";
        tokio::fs::write(&pub_path, expected).await.unwrap();

        let private_path = dir.path().join("test_key");
        let result = read_public_key(&private_path).await.unwrap();
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn read_public_key_missing_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let private_path = dir.path().join("nonexistent_key");

        let result = read_public_key(&private_path).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, BorgError::SshFailed { .. }));
        assert!(err.to_string().contains("failed to read public key"));
    }

    #[tokio::test]
    async fn read_public_key_after_generate() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("roundtrip_key");
        generate_key(&key_path).await.unwrap();

        let pub_content = read_public_key(&key_path).await.unwrap();
        assert!(pub_content.starts_with("ssh-ed25519 "));
        assert!(pub_content.contains("borgui-backup-key"));
    }

    #[tokio::test]
    async fn test_connection_returns_false_for_closed_port() {
        let result = test_connection("127.0.0.1", 61234, "nobody", None)
            .await
            .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn test_connection_with_key_path_returns_false_for_closed_port() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("fake_key");
        tokio::fs::write(&key_path, "not a real key").await.unwrap();

        let result = test_connection("127.0.0.1", 61234, "nobody", Some(&key_path))
            .await
            .unwrap();
        assert!(!result);
    }
}
