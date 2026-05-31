use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Notify;
use tracing::{debug, warn};

use crate::archive::ArchiveEntry;
use crate::config::{BackupProfile, RepoConfig};
use crate::error::{BorgError, Result};
use crate::progress::ProgressEvent;

/// Default timeout for short, interactive borg/ssh calls (version, info,
/// listing archives). Long-running operations (create, extract, prune) are not
/// time-limited — they rely on [`CancelToken`] instead.
const QUICK_OP_TIMEOUT_SECS: u64 = 120;

/// Timeout for interactive-but-potentially-large reads (listing the contents of
/// one archive, which backs the archive browser). Generous, but bounded so a
/// stalled SSH connection can't freeze the UI forever.
const LIST_CONTENTS_TIMEOUT_SECS: u64 = 600;

/// Result of a `create` or `extract` run. A borg exit code of `1` means the
/// operation *succeeded* but emitted warnings (e.g. a file was locked or
/// unreadable and was skipped) — the archive is still valid and restorable.
/// Those warning lines are surfaced here so the UI can show them without
/// treating the whole backup as a failure.
#[derive(Debug, Default, Clone)]
pub struct OpOutcome {
    pub warnings: Vec<String>,
}

impl OpOutcome {
    pub fn had_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// How borg's process exit code should be interpreted.
enum ExitClass {
    /// rc == 0
    Ok,
    /// rc == 1 — completed with warnings (still a success for our purposes).
    Warning,
    /// rc >= 2 (or signal/unknown) — a real failure.
    Error,
}

fn classify_exit(code: Option<i32>) -> ExitClass {
    match code {
        Some(0) => ExitClass::Ok,
        Some(1) => ExitClass::Warning,
        _ => ExitClass::Error,
    }
}

/// Cooperative cancellation handle shared between a caller (e.g. a "Cancel"
/// button) and a running borg operation. Cancelling kills the borg child
/// process and the operation resolves to [`BorgError::Cancelled`].
#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Resolves as soon as the token is cancelled. Safe against the
    /// notify-before-await race: the flag is re-checked after registering
    /// interest in the notification.
    async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.notify.notified();
            tokio::pin!(notified);
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

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

    /// Run a command to completion, applying an optional timeout and treating
    /// borg's warning exit code (1) as success. Real failures (rc >= 2) become
    /// [`BorgError::ProcessFailed`].
    async fn run_checked(
        &self,
        mut cmd: Command,
        op: &str,
        timeout_secs: Option<u64>,
    ) -> Result<std::process::Output> {
        let output = match timeout_secs {
            Some(secs) => tokio::time::timeout(Duration::from_secs(secs), cmd.output())
                .await
                .map_err(|_| BorgError::Timeout { seconds: secs })??,
            None => cmd.output().await?,
        };

        match classify_exit(output.status.code()) {
            ExitClass::Ok => Ok(output),
            ExitClass::Warning => {
                // rc==1 means borg completed but emitted warnings. These ops
                // (prune/delete/init/list) don't stream warnings to the caller,
                // so surface them in the log rather than swallowing them.
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    warn!("borg {op} completed with warnings: {}", stderr.trim());
                }
                Ok(output)
            }
            ExitClass::Error => Err(BorgError::ProcessFailed {
                message: format!("borg {op} failed"),
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            }),
        }
    }

    pub async fn version(&self) -> Result<String> {
        let mut cmd = self.base_command();
        cmd.arg("--version");
        let output = self
            .run_checked(cmd, "version", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub async fn info(
        &self,
        repo: &RepoConfig,
        passphrase: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["info", "--json", &repo.location()]);
        let output = self
            .run_checked(cmd, "info", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;

        Ok(serde_json::from_slice(&output.stdout)?)
    }

    /// Spawn borg, stream its `--log-json` progress events to `on_progress`,
    /// drain stdout so the child can never block on a full pipe, honour
    /// `cancel`, and interpret the exit code with warning semantics. Shared by
    /// [`create`](Self::create) and [`extract`](Self::extract).
    async fn run_streaming(
        &self,
        mut cmd: Command,
        op: &str,
        cancel: &CancelToken,
        on_progress: impl Fn(ProgressEvent) + Send + 'static,
    ) -> Result<OpOutcome> {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stderr = child.stderr.take().expect("stderr was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let mut reader = BufReader::new(stderr).lines();

        let stderr_capture: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_clone = stderr_capture.clone();
        let warn_clone = warnings.clone();

        let reader_task = tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                match serde_json::from_str::<ProgressEvent>(&line) {
                    Ok(event) => {
                        if let ProgressEvent::LogMessage { levelname, message } = &event
                            && matches!(
                                levelname.to_ascii_uppercase().as_str(),
                                "WARNING" | "ERROR" | "CRITICAL"
                            )
                        {
                            warn_clone
                                .lock()
                                .expect("warn mutex poisoned")
                                .push(message.clone());
                        }
                        on_progress(event);
                    }
                    Err(_) => debug!("borg stderr: {}", line),
                }
                stderr_clone
                    .lock()
                    .expect("stderr mutex poisoned")
                    .push(line);
            }
        });

        // Drain stdout (borg's `--json` summary) so a full pipe can never
        // deadlock the child. We don't currently use the contents.
        let stdout_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut sink = Vec::new();
            let mut stdout = stdout;
            let _ = stdout.read_to_end(&mut sink).await;
        });

        let status = tokio::select! {
            res = child.wait() => res?,
            _ = cancel.cancelled() => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                let _ = reader_task.await;
                let _ = stdout_task.await;
                return Err(BorgError::Cancelled);
            }
        };
        let _ = reader_task.await;
        let _ = stdout_task.await;

        match classify_exit(status.code()) {
            ExitClass::Ok => Ok(OpOutcome::default()),
            ExitClass::Warning => Ok(OpOutcome {
                warnings: warnings.lock().expect("warn mutex poisoned").clone(),
            }),
            ExitClass::Error => {
                let captured = stderr_capture
                    .lock()
                    .expect("stderr mutex poisoned")
                    .join("\n");
                Err(BorgError::ProcessFailed {
                    message: format!("borg {op} failed"),
                    exit_code: status.code(),
                    stderr: captured,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        profile: &BackupProfile,
        archive_name: &str,
        cwd: Option<&Path>,
        passphrase: Option<&str>,
        cancel: &CancelToken,
        on_progress: impl Fn(ProgressEvent) + Send + 'static,
    ) -> Result<OpOutcome> {
        let archive = format!("{}::{}", profile.repo.location(), archive_name);

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

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        self.run_streaming(cmd, "create", cancel, on_progress).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn extract(
        &self,
        repo: &RepoConfig,
        archive_name: &str,
        destination: &Path,
        paths: &[String],
        passphrase: Option<&str>,
        cancel: &CancelToken,
        on_progress: impl Fn(ProgressEvent) + Send + 'static,
    ) -> Result<OpOutcome> {
        let archive = format!("{}::{}", repo.location(), archive_name);

        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["extract", "--progress", "--log-json"]);
        cmd.arg(&archive);
        // Selective restore: borg matches positional PATHs using path-prefix
        // (`pp:`) style by default, which is a *literal* match — so a stored
        // path like `report?.txt` or `what's up?.txt` is matched exactly rather
        // than as an fnmatch wildcard. We deliberately do NOT prefix an explicit
        // `pp:` here: several borg builds (e.g. 1.2.x) reject the inline style
        // prefix on `--pattern`/PATH with "Invalid pattern style", whereas the
        // positional default is literal and works across versions.
        for path in paths {
            cmd.arg(path);
        }
        cmd.current_dir(destination);

        self.run_streaming(cmd, "extract", cancel, on_progress)
            .await
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

        cmd.arg(repo.location());

        self.run_checked(cmd, "prune", None).await?;
        Ok(())
    }

    pub async fn init_repo(
        &self,
        repo: &RepoConfig,
        encryption: &str,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let mut cmd = self.base_command();
        cmd.args(["init", "--encryption", encryption, &repo.location()]);

        if let Some(pass) = passphrase {
            cmd.env("BORG_PASSPHRASE", pass);
            cmd.env("BORG_NEW_PASSPHRASE", pass);
        }

        self.run_checked(cmd, "init", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;
        Ok(())
    }

    pub async fn delete_archive(
        &self,
        repo: &RepoConfig,
        archive_name: &str,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let archive = format!("{}::{}", repo.location(), archive_name);
        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["delete", &archive]);
        self.run_checked(cmd, "delete", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;
        Ok(())
    }

    pub async fn list_archives(
        &self,
        repo: &RepoConfig,
        passphrase: Option<&str>,
    ) -> Result<Vec<ArchiveInfo>> {
        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["list", "--json", &repo.location()]);
        let output = self
            .run_checked(cmd, "list", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;

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

    // FIXME(perf): collects the full JSON-lines listing into memory before
    // returning. For a 100k-entry archive that's ~30-50 MB of allocations.
    // Replace with a streaming variant (line-by-line via tauri emit) once we
    // hit a user with a very large archive.
    pub async fn list_contents(
        &self,
        repo: &RepoConfig,
        archive_name: &str,
        passphrase: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>> {
        let archive = format!("{}::{}", repo.location(), archive_name);

        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["list", "--json-lines", &archive]);
        let output = self
            .run_checked(cmd, "list (contents)", Some(LIST_CONTENTS_TIMEOUT_SECS))
            .await?;

        let entries = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<ArchiveEntry>(line).ok())
            .collect();

        Ok(entries)
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

    #[test]
    fn classify_exit_treats_one_as_warning() {
        assert!(matches!(classify_exit(Some(0)), ExitClass::Ok));
        assert!(matches!(classify_exit(Some(1)), ExitClass::Warning));
        assert!(matches!(classify_exit(Some(2)), ExitClass::Error));
        assert!(matches!(classify_exit(Some(128)), ExitClass::Error));
        // A process killed by a signal reports no exit code -> treat as error.
        assert!(matches!(classify_exit(None), ExitClass::Error));
    }

    #[test]
    fn op_outcome_reports_warnings() {
        assert!(!OpOutcome::default().had_warnings());
        let with = OpOutcome {
            warnings: vec!["skipped locked.txt".into()],
        };
        assert!(with.had_warnings());
    }

    #[tokio::test]
    async fn cancel_token_resolves_when_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        let t2 = token.clone();
        token.cancel();
        assert!(token.is_cancelled());
        assert!(t2.is_cancelled());
        // cancelled() must return promptly for an already-cancelled token.
        tokio::time::timeout(Duration::from_secs(1), t2.cancelled())
            .await
            .expect("cancelled() should resolve immediately once cancelled");
    }

    #[tokio::test]
    async fn cancel_token_wakes_a_pending_waiter() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
        });
        // Give the waiter a moment to start awaiting, then cancel.
        tokio::task::yield_now().await;
        token.cancel();
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("waiter should wake within timeout")
            .expect("waiter task should not panic");
    }
}
