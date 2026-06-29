use rand_core::OsRng;
use ssh_key::{Algorithm, LineEnding, PrivateKey};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::debug;

use crate::proc;

use crate::error::{BorgError, Result};

pub async fn test_connection(
    host: &str,
    port: u16,
    user: &str,
    key_path: Option<&Path>,
) -> Result<()> {
    let mut cmd = proc::command("ssh");
    cmd.args(["-o", "BatchMode=yes"])
        .args(["-o", "ConnectTimeout=10"])
        .args(["-p", &port.to_string()]);

    if let Some(key) = key_path {
        cmd.args(["-i", &key.to_string_lossy()]);
    }

    cmd.arg(format!("{}@{}", user, host)).arg("echo ok");

    let output = cmd.output().await?;
    if output.status.success() {
        return Ok(());
    }

    // ssh writes the actionable diagnostic — "Permission denied (publickey)",
    // "Host key verification failed", "Connection refused", timeouts — to
    // stderr. Surface it instead of collapsing every failure into a bare
    // boolean the UI can only render as "Connection failed."
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    let message = if message.is_empty() {
        match output.status.code() {
            Some(code) => format!("ssh exited with status {code}"),
            None => "ssh was terminated by a signal".to_string(),
        }
    } else {
        message.to_string()
    };
    Err(BorgError::SshFailed { message })
}

/// Pre-flight reachability check: can we open a TCP connection to `host:port`?
/// This confirms the server is up and the SSH port is actually listening —
/// more reliable than an ICMP ping, which firewalls routinely drop even when
/// SSH works fine. It also validates the Host and Port together.
pub async fn check_reachable(host: &str, port: u16) -> Result<()> {
    let addr = format!("{host}:{port}");
    match timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => Ok(()),
        Ok(Err(e)) => Err(BorgError::CheckFailed {
            message: format!("{e} ({addr})"),
        }),
        Err(_) => Err(BorgError::CheckFailed {
            message: format!("timed out after 5s ({addr})"),
        }),
    }
}

/// Validate an unencrypted OpenSSH private key and derive its public key
/// without depending on an installed `ssh-keygen`.
pub async fn validate_key(key_path: &Path) -> Result<String> {
    let encoded = tokio::fs::read(key_path).await?;
    let private_key = PrivateKey::from_openssh(&encoded).map_err(|e| BorgError::CheckFailed {
        message: format!("The selected file is not a valid OpenSSH private key: {e}"),
    })?;
    if private_key.is_encrypted() {
        return Err(BorgError::CheckFailed {
            message: "Passphrase-protected keys are not supported because unattended backups cannot unlock them."
                .into(),
        });
    }
    private_key
        .public_key()
        .to_openssh()
        .map_err(|e| BorgError::CheckFailed {
            message: format!("failed to derive public key: {e}"),
        })
}

/// Generate an unencrypted Ed25519 keypair in OpenSSH format.
///
/// Existing files are only replaced when `overwrite` is explicitly true.
pub async fn generate_key(path: &Path, overwrite: bool) -> Result<PathBuf> {
    let public_path = path.with_extension("pub");
    if !overwrite
        && (tokio::fs::try_exists(path).await? || tokio::fs::try_exists(&public_path).await?)
    {
        return Err(BorgError::CheckFailed {
            message: "An SSH key already exists at this location.".into(),
        });
    }

    let parent = path.parent().ok_or_else(|| BorgError::CheckFailed {
        message: "SSH key path has no parent directory.".into(),
    })?;
    tokio::fs::create_dir_all(parent).await?;

    let mut private_key =
        PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(|e| BorgError::SshFailed {
            message: format!("failed to generate Ed25519 key: {e}"),
        })?;
    private_key.set_comment("borgui-backup-key");
    let private_text =
        private_key
            .to_openssh(LineEnding::LF)
            .map_err(|e| BorgError::SshFailed {
                message: format!("failed to encode private key: {e}"),
            })?;
    let public_text = private_key
        .public_key()
        .to_openssh()
        .map_err(|e| BorgError::SshFailed {
            message: format!("failed to encode public key: {e}"),
        })?;

    let private_tmp = path.with_extension("borgui-private.tmp");
    let public_tmp = path.with_extension("borgui-public.tmp");
    tokio::fs::write(&private_tmp, private_text.as_bytes()).await?;
    restrict_private_key_permissions(&private_tmp).await?;
    tokio::fs::write(&public_tmp, format!("{public_text}\n")).await?;
    if let Err(error) =
        commit_keypair(&private_tmp, &public_tmp, path, &public_path, overwrite).await
    {
        let _ = tokio::fs::remove_file(&private_tmp).await;
        let _ = tokio::fs::remove_file(&public_tmp).await;
        return Err(error);
    }
    debug!("generated SSH key at {:?}", path);
    Ok(path.to_path_buf())
}

async fn commit_keypair(
    private_source: &Path,
    public_source: &Path,
    private_destination: &Path,
    public_destination: &Path,
    overwrite: bool,
) -> Result<()> {
    let private_backup = private_destination.with_extension("borgui-private.bak");
    let public_backup = public_destination.with_extension("borgui-public.bak");
    let mut backed_up_private = false;
    let mut backed_up_public = false;

    if overwrite && tokio::fs::try_exists(private_destination).await? {
        tokio::fs::rename(private_destination, &private_backup).await?;
        backed_up_private = true;
    }
    if overwrite && tokio::fs::try_exists(public_destination).await? {
        if let Err(error) = tokio::fs::rename(public_destination, &public_backup).await {
            if backed_up_private {
                let _ = tokio::fs::rename(&private_backup, private_destination).await;
            }
            return Err(error.into());
        }
        backed_up_public = true;
    }

    if let Err(error) = tokio::fs::rename(private_source, private_destination).await {
        restore_keypair(
            private_destination,
            public_destination,
            &private_backup,
            &public_backup,
            backed_up_private,
            backed_up_public,
        )
        .await;
        return Err(error.into());
    }
    if let Err(error) = tokio::fs::rename(public_source, public_destination).await {
        restore_keypair(
            private_destination,
            public_destination,
            &private_backup,
            &public_backup,
            backed_up_private,
            backed_up_public,
        )
        .await;
        return Err(error.into());
    }

    if backed_up_private {
        let _ = tokio::fs::remove_file(private_backup).await;
    }
    if backed_up_public {
        let _ = tokio::fs::remove_file(public_backup).await;
    }
    Ok(())
}

async fn restore_keypair(
    private_destination: &Path,
    public_destination: &Path,
    private_backup: &Path,
    public_backup: &Path,
    backed_up_private: bool,
    backed_up_public: bool,
) {
    let _ = tokio::fs::remove_file(private_destination).await;
    let _ = tokio::fs::remove_file(public_destination).await;
    if backed_up_private {
        let _ = tokio::fs::rename(private_backup, private_destination).await;
    }
    if backed_up_public {
        let _ = tokio::fs::rename(public_backup, public_destination).await;
    }
}

#[cfg(unix)]
async fn restrict_private_key_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn restrict_private_key_permissions(_path: &Path) -> Result<()> {
    Ok(())
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

        let result = generate_key(&key_path, false).await.unwrap();

        assert_eq!(result, key_path);
        assert!(key_path.exists());
        assert!(key_path.with_extension("pub").exists());
    }

    #[tokio::test]
    async fn generate_key_creates_ed25519() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path, false).await.unwrap();

        let pub_content = tokio::fs::read_to_string(key_path.with_extension("pub"))
            .await
            .unwrap();
        assert!(pub_content.starts_with("ssh-ed25519 "));
    }

    #[tokio::test]
    async fn generate_key_includes_comment() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path, false).await.unwrap();

        let pub_content = tokio::fs::read_to_string(key_path.with_extension("pub"))
            .await
            .unwrap();
        assert!(pub_content.contains("borgui-backup-key"));
    }

    #[tokio::test]
    async fn generate_key_refuses_overwrite_without_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path, false).await.unwrap();
        let original = tokio::fs::read(&key_path).await.unwrap();

        let error = generate_key(&key_path, false).await.unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(tokio::fs::read(&key_path).await.unwrap(), original);
    }

    #[tokio::test]
    async fn generate_key_replaces_pair_after_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path, false).await.unwrap();
        let original = tokio::fs::read(&key_path).await.unwrap();

        generate_key(&key_path, true).await.unwrap();
        assert_ne!(tokio::fs::read(&key_path).await.unwrap(), original);
        assert!(validate_key(&key_path).await.is_ok());
    }

    #[tokio::test]
    async fn keypair_commit_restores_original_pair_when_public_commit_fails() {
        let dir = tempfile::tempdir().unwrap();
        let private_path = dir.path().join("id_ed25519");
        let public_path = dir.path().join("id_ed25519.pub");
        let private_tmp = dir.path().join("private.tmp");
        let missing_public_tmp = dir.path().join("missing-public.tmp");
        tokio::fs::write(&private_path, "old private")
            .await
            .unwrap();
        tokio::fs::write(&public_path, "old public").await.unwrap();
        tokio::fs::write(&private_tmp, "new private").await.unwrap();

        assert!(
            commit_keypair(
                &private_tmp,
                &missing_public_tmp,
                &private_path,
                &public_path,
                true,
            )
            .await
            .is_err()
        );
        assert_eq!(
            tokio::fs::read_to_string(&private_path).await.unwrap(),
            "old private"
        );
        assert_eq!(
            tokio::fs::read_to_string(&public_path).await.unwrap(),
            "old public"
        );
    }

    #[tokio::test]
    async fn generate_key_fails_on_invalid_path() {
        let result = generate_key(Path::new("/nonexistent/dir/key"), false).await;
        assert!(result.is_err());
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
        generate_key(&key_path, false).await.unwrap();

        let pub_content = read_public_key(&key_path).await.unwrap();
        assert!(pub_content.starts_with("ssh-ed25519 "));
        assert!(pub_content.contains("borgui-backup-key"));
    }

    #[tokio::test]
    async fn test_connection_errors_for_closed_port() {
        let result = test_connection("127.0.0.1", 61234, "nobody", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connection_with_key_path_errors_for_closed_port() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("fake_key");
        tokio::fs::write(&key_path, "not a real key").await.unwrap();

        let result = test_connection("127.0.0.1", 61234, "nobody", Some(&key_path)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn check_reachable_succeeds_for_open_port() {
        // Bind an ephemeral port so there's a real listener to connect to.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let result = check_reachable("127.0.0.1", port).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn check_reachable_errors_for_closed_port() {
        let result = check_reachable("127.0.0.1", 61235).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn validate_key_returns_public_key() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("vkey");
        generate_key(&key_path, false).await.unwrap();

        let pubkey = validate_key(&key_path).await.unwrap();
        assert!(pubkey.starts_with("ssh-ed25519 "));
    }

    #[tokio::test]
    async fn validate_key_errors_on_non_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notakey");
        tokio::fs::write(&path, "garbage").await.unwrap();

        let error = validate_key(&path).await.unwrap_err();
        assert!(error.to_string().contains("valid OpenSSH private key"));
    }

    #[tokio::test]
    async fn validate_key_rejects_passphrase_protected_key() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("encrypted_key");
        let output = proc::command("ssh-keygen")
            .args(["-t", "ed25519"])
            .args(["-f", &key_path.to_string_lossy()])
            .args(["-N", "test-passphrase"])
            .output()
            .await
            .unwrap();
        assert!(output.status.success());

        let error = validate_key(&key_path).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Passphrase-protected keys are not supported")
        );
    }

    #[tokio::test]
    async fn validate_key_accepts_existing_ecdsa_key() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("ecdsa_key");
        let output = proc::command("ssh-keygen")
            .args(["-t", "ecdsa"])
            .args(["-b", "256"])
            .args(["-f", &key_path.to_string_lossy()])
            .args(["-N", ""])
            .output()
            .await
            .unwrap();
        assert!(output.status.success());

        let public_key = validate_key(&key_path).await.unwrap();
        assert!(public_key.starts_with("ecdsa-sha2-nistp256 "));
    }

    #[tokio::test]
    async fn test_connection_failure_surfaces_message() {
        let err = test_connection("127.0.0.1", 61234, "nobody", None)
            .await
            .unwrap_err();
        assert!(matches!(err, BorgError::SshFailed { .. }));
        // The whole point of the change: a failure carries a real, non-empty
        // diagnostic for the UI to display, not just a boolean.
        assert!(!err.to_string().is_empty());
    }
}
