//! SSH connectivity and key-material commands.

use super::*;

#[tauri::command]
pub async fn test_ssh_connection(
    host: String,
    port: u16,
    user: String,
    key_path: Option<String>,
) -> Result<(), String> {
    // Option-injection gate: ssh is spawned with direct argv, so a host or
    // user beginning with `-` would be parsed as an ssh flag (e.g.
    // `-oProxyCommand=...`) instead of part of the destination.
    borg_core::config::reject_option_like("ssh_host", &host).map_err(|e| e.to_string())?;
    borg_core::config::reject_option_like("ssh_user", &user).map_err(|e| e.to_string())?;
    let key = key_path.map(PathBuf::from);
    borg_core::ssh::test_connection(&host, port, &user, key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Per-field pre-flight: can we reach the SSH server on this host:port?
#[tauri::command]
pub async fn check_host_reachable(host: String, port: u16) -> Result<(), String> {
    borg_core::ssh::check_reachable(&host, port)
        .await
        .map_err(|e| e.to_string())
}

/// Per-field pre-flight: validate the private-key file and return its public key.
#[tauri::command]
pub async fn validate_ssh_key(key_path: String) -> Result<String, String> {
    borg_core::ssh::validate_key(&PathBuf::from(key_path))
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct GeneratedSshKey {
    pub private_key_path: String,
    pub public_key: String,
}

/// Generate BorgUI's managed Ed25519 key without requiring Windows OpenSSH.
#[tauri::command]
pub async fn generate_ssh_key(
    app: tauri::AppHandle,
    overwrite: bool,
) -> Result<GeneratedSshKey, String> {
    let key_path = config_dir(&app)
        .await?
        .join("ssh")
        .join("id_ed25519_borgui");
    borg_core::ssh::generate_key(&key_path, overwrite)
        .await
        .map_err(|e| e.to_string())?;
    let public_key = borg_core::ssh::read_public_key(&key_path)
        .await
        .map_err(|e| e.to_string())?
        .trim()
        .to_string();
    Ok(GeneratedSshKey {
        private_key_path: key_path.to_string_lossy().into_owned(),
        public_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ssh_connection_rejects_option_like_host_and_user() {
        // Both must fail at the validation gate, before ssh is ever spawned.
        let err = test_ssh_connection("-oProxyCommand=calc".into(), 22, "borg".into(), None)
            .await
            .unwrap_err();
        assert!(err.contains("cannot start with '-'"), "got: {err}");
        let err = test_ssh_connection("host.example.com".into(), 22, "-l".into(), None)
            .await
            .unwrap_err();
        assert!(err.contains("cannot start with '-'"), "got: {err}");
    }
}
