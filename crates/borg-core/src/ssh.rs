use rand_core::OsRng;
use ssh_key::{Algorithm, LineEnding, PrivateKey};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::debug;

use crate::proc;

use crate::error::{BorgError, Result};

/// Outer bound on the `ssh` connectivity probe. Comfortably above ssh's own
/// `-o ConnectTimeout=10` so a healthy-but-slow host still succeeds, while a
/// host that never answers can no longer hang the caller indefinitely.
const CONNECT_TEST_TIMEOUT_SECS: u64 = 30;

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
    // Reap the child if the timeout below fires. Without this a hung ssh.exe
    // outlives the probe and accumulates across calls.
    cmd.kill_on_drop(true);

    // Bound the probe ourselves rather than trusting `-o ConnectTimeout`.
    // Windows OpenSSH does not reliably honour it: on the Windows guest,
    // `ssh -o BatchMode=yes -o ConnectTimeout=10` against a closed/filtered
    // port was still running after 90s. Nothing else bounds this call, so the
    // caller hangs forever -- including `test_ssh_connection`, the "Test
    // connection" button, which awaits it directly with no timeout of its own.
    // Mirrors the timeout `borg.rs::run_checked` puts around every borg spawn.
    let output = timeout(Duration::from_secs(CONNECT_TEST_TIMEOUT_SECS), cmd.output())
        .await
        .map_err(|_| BorgError::Timeout {
            seconds: CONNECT_TEST_TIMEOUT_SECS,
        })??;
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
    let classification = classify_ssh_failure_message(&message);
    let message = format!("{classification}: {message}");
    Err(BorgError::SshFailed { message })
}

pub fn classify_ssh_failure_message(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("host key verification failed")
        || lower.contains("remote host identification has changed")
        || lower.contains("no hostkey alg")
    {
        "host-key trust failed"
    } else if lower.contains("permission denied")
        || lower.contains("authentication failed")
        || lower.contains("too many authentication failures")
        || lower.contains("publickey")
    {
        "authentication failed"
    } else if lower.contains("administratively prohibited")
        || lower.contains("not allowed to execute")
        || lower.contains("shell request failed")
        || lower.contains("sftp connections only")
        || lower.contains("account is currently not available")
    {
        "authorization failed"
    } else if lower.contains("connection refused")
        || lower.contains("connection timed out")
        || lower.contains("operation timed out")
        || lower.contains("network is unreachable")
        || lower.contains("no route to host")
        || lower.contains("could not resolve hostname")
        || lower.contains("temporary failure in name resolution")
    {
        "reachability failed"
    } else {
        "ssh failed"
    }
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
    // Create the private-key file empty and lock its permissions down BEFORE
    // the key material is written, so the secret never sits on disk with the
    // default (other-user-readable) mode/ACL. If permissions cannot be
    // restricted, key generation must fail — remove the temp file and bail
    // rather than leave behind a key other local users could read.
    tokio::fs::write(&private_tmp, b"").await?;
    if let Err(error) = restrict_private_key_permissions(&private_tmp).await {
        let _ = tokio::fs::remove_file(&private_tmp).await;
        return Err(error);
    }
    tokio::fs::write(&private_tmp, private_text.as_bytes()).await?;
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

/// Restrict a private-key file to the current user only: collapse the ACL
/// down to zero entries and replace it with a single full-control grant for
/// the current user. This is the Windows equivalent of `chmod 600`.
#[cfg(windows)]
async fn restrict_private_key_permissions(path: &Path) -> Result<()> {
    let sid = current_user_sid().await?;
    // New files get explicit (non-inherited) ACEs for SYSTEM/Administrators
    // from the creating process's default DACL, on top of whatever the
    // parent directory inherits down. `/inheritance:r` alone only strips
    // the inherited ACEs, leaving those explicit ones behind. `/reset`
    // first collapses the ACL back to just-inherited defaults, then
    // `/inheritance:r` strips those too, so the file has zero ACEs before
    // we grant the sole entry we want.
    // Grant by SID (`*S-1-...`), never by account name: localized Windows
    // editions translate well-known account names, so a name-based grant
    // breaks outside English locales. SIDs are locale-independent.
    for args in [
        vec!["/reset".to_string()],
        vec!["/inheritance:r".to_string()],
        vec!["/grant:r".to_string(), format!("*{sid}:F")],
    ] {
        let output = proc::command("icacls")
            .arg(path)
            .args(&args)
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BorgError::CheckFailed {
                message: format!(
                    "failed to restrict private key permissions on {:?}: icacls exited with {:?}: {}",
                    path,
                    output.status.code(),
                    stderr.trim()
                ),
            });
        }
    }
    Ok(())
}

/// Resolve the current user's SID via `whoami /user` (ships with every
/// supported Windows, in System32).
#[cfg(windows)]
async fn current_user_sid() -> Result<String> {
    let output = proc::command("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .await?;
    if !output.status.success() {
        return Err(BorgError::CheckFailed {
            message: format!(
                "failed to resolve current user SID: whoami exited with {:?}",
                output.status.code()
            ),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_sid_from_whoami(&stdout).ok_or_else(|| BorgError::CheckFailed {
        message: format!(
            "failed to parse current user SID from whoami output: {}",
            stdout.trim()
        ),
    })
}

/// Extract the `S-1-...` SID from `whoami /user /fo csv /nh` output
/// (`"DOMAIN\user","S-1-5-21-..."`). Only the SID field is used, so the
/// parse is independent of the account-name field and of locale.
/// Un-gated so the parsing logic is unit-testable on every platform.
#[cfg(any(windows, test))]
fn parse_sid_from_whoami(output: &str) -> Option<String> {
    let start = output.find("S-1-")?;
    let tail = &output[start..];
    let end = tail
        .char_indices()
        .skip(1)
        .find(|(_, c)| !c.is_ascii_digit() && *c != '-')
        .map(|(i, _)| i)
        .unwrap_or(tail.len());
    let sid = tail[..end].trim_end_matches('-');
    // A real SID has sub-authorities beyond the bare "S-1-" prefix.
    if sid.len() > 4 && sid.ends_with(|c: char| c.is_ascii_digit()) {
        Some(sid.to_string())
    } else {
        None
    }
}

/// Fail closed on platforms without an implementation: refusing to generate
/// the key beats silently leaving it readable by other users.
#[cfg(not(any(unix, windows)))]
async fn restrict_private_key_permissions(path: &Path) -> Result<()> {
    Err(BorgError::CheckFailed {
        message: format!(
            "cannot restrict private key permissions on {:?}: unsupported platform",
            path
        ),
    })
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

    #[test]
    fn parses_sid_from_whoami_csv_output() {
        let output =
            "\"DESKTOP-ABC\\someuser\",\"S-1-5-21-1004336348-1177238915-682003330-1001\"\r\n";
        assert_eq!(
            parse_sid_from_whoami(output).as_deref(),
            Some("S-1-5-21-1004336348-1177238915-682003330-1001")
        );
    }

    #[test]
    fn parses_sid_rejects_output_without_sid() {
        assert_eq!(parse_sid_from_whoami("\"DESKTOP\\user\",\"\""), None);
        assert_eq!(parse_sid_from_whoami("garbage"), None);
        // A bare prefix with no sub-authorities is not a usable SID.
        assert_eq!(parse_sid_from_whoami("\"u\",\"S-1-\""), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn generated_private_key_has_owner_only_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path, false).await.unwrap();

        let mode = tokio::fs::metadata(&key_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn current_user_sid_resolves() {
        let sid = current_user_sid().await.unwrap();
        assert!(sid.starts_with("S-1-"), "unexpected SID: {sid}");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn restrict_permissions_fails_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does_not_exist");
        assert!(restrict_private_key_permissions(&missing).await.is_err());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn generated_private_key_grants_only_current_user() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("test_key");
        generate_key(&key_path, false).await.unwrap();

        let output = proc::command("icacls")
            .arg(&key_path)
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let listing = String::from_utf8_lossy(&output.stdout);
        // Exactly one ACE — the current user's explicit full-control grant.
        // Inherited entries (Users, SYSTEM, Administrators, ...) must be gone,
        // and the DACL must survive the tmp-file rename into place.
        let aces: Vec<&str> = listing.lines().filter(|l| l.contains(":(")).collect();
        assert_eq!(aces.len(), 1, "expected exactly one ACE, got: {listing}");
        assert!(
            aces[0].contains("(F)") && !aces[0].contains("(I)"),
            "expected an explicit (non-inherited) full-control ACE: {listing}"
        );
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
        // Use a regular file as a parent directory: writing a key under a
        // path whose component is a file fails on every platform. A hardcoded
        // "/nonexistent/dir" is unwritable on Linux but creatable on Windows
        // CI (writable drive root), so it is not a portable "invalid path".
        let dir = tempfile::tempdir().unwrap();
        let file_as_dir = dir.path().join("not-a-dir");
        tokio::fs::write(&file_as_dir, b"x").await.unwrap();
        let result = generate_key(&file_as_dir.join("key"), false).await;
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
        // Which variant depends on the platform, and both are correct:
        // everywhere ssh answers, it reports "Connection refused" and we return
        // SshFailed carrying that text. On Windows, OpenSSH does not honour
        // `-o ConnectTimeout` against this port and never answers, so our own
        // bound fires and Timeout is the honest result. Pinning SshFailed here
        // made this test hang forever on Windows before that bound existed.
        assert!(matches!(
            err,
            BorgError::SshFailed { .. } | BorgError::Timeout { .. }
        ));
        // The guarantee this test actually exists to protect: a failure carries
        // a real, non-empty diagnostic for the UI to display, not just a boolean.
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn classifies_common_ssh_failures() {
        assert_eq!(
            classify_ssh_failure_message("Host key verification failed."),
            "host-key trust failed"
        );
        assert_eq!(
            classify_ssh_failure_message("Permission denied (publickey)."),
            "authentication failed"
        );
        assert_eq!(
            classify_ssh_failure_message("This service allows sftp connections only."),
            "authorization failed"
        );
        assert_eq!(
            classify_ssh_failure_message(
                "ssh: connect to host example port 22: Connection refused"
            ),
            "reachability failed"
        );
    }
}
