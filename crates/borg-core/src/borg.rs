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
const CHECK_TIMEOUT_SECS: u64 = 24 * 60 * 60;

/// Number of archive entries streamed per batch by
/// [`BorgClient::list_contents_streaming`]. Large enough to amortise the IPC
/// round-trip, small enough that only one batch is ever held in memory — so a
/// 100k-entry archive stays bounded instead of allocating the whole listing.
const LIST_BATCH_SIZE: usize = 5_000;

/// Result of a `create` or `extract` run. A borg exit code of `1` means the
/// operation *succeeded* but emitted warnings (e.g. a file was locked or
/// unreadable and was skipped) — the archive is still valid and restorable.
/// Those warning lines are surfaced here so the UI can show them without
/// treating the whole backup as a failure.
#[derive(Debug, Default, Clone)]
pub struct OpOutcome {
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMode {
    Repository,
    VerifyData,
}

impl CheckMode {
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::Repository => &["check"],
            Self::VerifyData => &["check", "--verify-data"],
        }
    }
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

/// Defence in depth against option injection: every borg subcommand appends
/// the `--` end-of-options separator before its positional arguments (repo
/// locations, archive references, restore paths), so a value beginning with
/// `-` can never be parsed as a flag even if an upstream validation gate is
/// bypassed.
trait EndOfOptions {
    /// Append `--`; everything added afterwards is positional.
    fn end_options(&mut self) -> &mut Self;
}

impl EndOfOptions for Command {
    fn end_options(&mut self) -> &mut Self {
        self.arg("--")
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
        // Spawn via the shared helper so borg never flashes a console window on
        // Windows (CREATE_NO_WINDOW); a no-op elsewhere.
        let mut cmd = crate::proc::command(&self.binary_path);
        if let Some(ref passcommand) = self.passcommand {
            cmd.env("BORG_PASSCOMMAND", passcommand);
        }
        if let Some(p) = passphrase {
            cmd.env("BORG_PASSPHRASE", p);
        } else if self.passcommand.is_none() {
            // No passphrase and no passcommand: set an empty passphrase so borg
            // never blocks on an interactive passphrase prompt. On Windows borg
            // reads the prompt straight from the console (closing stdin isn't
            // enough); with an env value present it never prompts. An encrypted
            // repo then fails fast with "incorrect passphrase" instead of
            // hanging the app forever.
            cmd.env("BORG_PASSPHRASE", "");
        }
        cmd.env("BORG_RELOCATED_REPO_ACCESS_IS_OK", "yes");
        // Accessing a previously-unknown unencrypted repo otherwise triggers an
        // interactive "[y/N]" confirmation that hangs forever with no TTY
        // (notably on Windows, where it blocks the whole app).
        cmd.env("BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK", "yes");
        // `borg init` otherwise asks "Do you want your passphrase to be displayed
        // for verification? [y/N]" even when the passphrase is supplied via env —
        // another prompt that hangs on the Windows console. Answer it up front.
        cmd.env("BORG_DISPLAY_PASSPHRASE", "no");
        // Belt-and-suspenders: close stdin so any prompt that does read from it
        // gets EOF and aborts instead of hanging the app indefinitely.
        cmd.stdin(Stdio::null());
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
        cmd.args(["info", "--json"])
            .end_options()
            .arg(repo.location());
        let output = self
            .run_checked(cmd, "info", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;

        Ok(serde_json::from_slice(&output.stdout)?)
    }

    pub async fn export_key(
        &self,
        repo: &RepoConfig,
        destination: &Path,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["key", "export"])
            .end_options()
            .arg(repo.location())
            .arg(destination);
        self.run_checked(cmd, "key export", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;
        Ok(())
    }

    pub async fn import_key(
        &self,
        repo: &RepoConfig,
        source: &Path,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["key", "import"])
            .end_options()
            .arg(repo.location())
            .arg(source);
        self.run_checked(cmd, "key import", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;
        Ok(())
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
        cmd.kill_on_drop(true);

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

    /// Verify repository metadata, or metadata plus every archived data chunk.
    /// Borg's repair mode is intentionally not represented by this API.
    pub async fn check(
        &self,
        repo: &RepoConfig,
        mode: CheckMode,
        passphrase: Option<&str>,
        cancel: &CancelToken,
        on_progress: impl Fn(ProgressEvent) + Send + 'static,
    ) -> Result<OpOutcome> {
        let mut cmd = self.base_command_with(passphrase);
        cmd.args(mode.args())
            .args(["--progress", "--log-json"])
            .end_options()
            .arg(repo.location());

        tokio::time::timeout(
            Duration::from_secs(CHECK_TIMEOUT_SECS),
            self.run_streaming(cmd, "check", cancel, on_progress),
        )
        .await
        .map_err(|_| {
            cancel.cancel();
            BorgError::Timeout {
                seconds: CHECK_TIMEOUT_SECS,
            }
        })?
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
        cmd.args(upload_rate_args(profile));

        cmd.end_options().arg(&archive);
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
        cmd.end_options().arg(&archive);
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

    /// Prunes archives per the retention policy, restricted to archives whose
    /// name matches `archive_glob` (borg 1.x `--glob-archives`).
    ///
    /// The glob is mandatory and must carry a discriminating prefix: a borg
    /// repository is commonly shared by several machines/profiles, and an
    /// unscoped `borg prune` applies one profile's retention policy to *every*
    /// archive in the repository — silently deleting other machines' backups.
    /// An empty glob or one starting with `*` is rejected here as a final
    /// backstop; there is deliberately no whole-repository prune in this
    /// client. Higher layers (`prune_scoped` in the app) derive safe globs.
    pub async fn prune(
        &self,
        repo: &RepoConfig,
        retention: &crate::config::RetentionConfig,
        archive_glob: &str,
        passphrase: Option<&str>,
    ) -> Result<OpOutcome> {
        if archive_glob.is_empty() || archive_glob.starts_with('*') {
            return Err(BorgError::InvalidConfig {
                message: format!(
                    "refusing to prune with unscoped archive glob {archive_glob:?}: \
                     it would apply this retention policy to every archive in the \
                     repository, including other machines' backups"
                ),
            });
        }
        let mut cmd = self.base_command_with(passphrase);
        cmd.args(prune_args(retention, archive_glob));
        cmd.end_options().arg(repo.location());

        let output = self.run_checked(cmd, "prune", None).await?;
        // borg prune exits 1 (warning) for non-fatal issues; collect the plain
        // stderr lines so the caller can surface them.
        let warnings = if output.status.code() == Some(1) {
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        } else {
            Vec::new()
        };
        Ok(OpOutcome { warnings })
    }

    pub async fn init_repo(
        &self,
        repo: &RepoConfig,
        encryption: &str,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let mut cmd = self.base_command();
        cmd.args(["init", "--encryption", encryption])
            .end_options()
            .arg(repo.location());

        if let Some(pass) = passphrase {
            cmd.env("BORG_PASSPHRASE", pass);
            cmd.env("BORG_NEW_PASSPHRASE", pass);
        }

        self.run_checked(cmd, "init", Some(QUICK_OP_TIMEOUT_SECS))
            .await?;
        Ok(())
    }

    fn change_passphrase_command(&self, repo: &RepoConfig, old: &str, new: &str) -> Command {
        let mut cmd = self.base_command_with(Some(old));
        cmd.env("BORG_NEW_PASSPHRASE", new);
        cmd.args(["key", "change-passphrase"])
            .end_options()
            .arg(repo.location());
        cmd
    }

    /// Rotate the repository's real passphrase via `borg key change-passphrase`.
    /// Callers own keeping the OS keychain in sync: update it only after this
    /// succeeds, or the stored copy and the repository will desync.
    pub async fn change_passphrase(
        &self,
        repo: &RepoConfig,
        old_passphrase: &str,
        new_passphrase: &str,
    ) -> Result<()> {
        if old_passphrase.is_empty() {
            return Err(BorgError::InvalidConfig {
                message: "current passphrase is required to change the repository passphrase"
                    .into(),
            });
        }
        if new_passphrase.is_empty() {
            return Err(BorgError::InvalidConfig {
                message: "new passphrase cannot be empty".into(),
            });
        }
        let cmd = self.change_passphrase_command(repo, old_passphrase, new_passphrase);
        self.run_checked(cmd, "key change-passphrase", Some(QUICK_OP_TIMEOUT_SECS))
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
        cmd.arg("delete").end_options().arg(&archive);
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
        cmd.args(["list", "--json"])
            .end_options()
            .arg(repo.location());
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

    /// Collect the full contents of an archive into a `Vec`. Thin wrapper over
    /// [`list_contents_streaming`](Self::list_contents_streaming) for callers
    /// (tests, small archives) that want the whole listing at once. Prefer the
    /// streaming variant for archives that may hold 100k+ entries — this one
    /// materialises every entry.
    pub async fn list_contents(
        &self,
        repo: &RepoConfig,
        archive_name: &str,
        passphrase: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>> {
        let collected: Arc<Mutex<Vec<ArchiveEntry>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = collected.clone();
        let cancel = CancelToken::new();
        self.list_contents_streaming(repo, archive_name, passphrase, &cancel, move |batch| {
            sink.lock()
                .expect("list_contents sink mutex poisoned")
                .extend(batch);
        })
        .await?;
        // The streaming call has returned, so `on_batch` (the only other Arc
        // holder) has been dropped; take the accumulated entries back out.
        let entries =
            std::mem::take(&mut *collected.lock().expect("list_contents sink mutex poisoned"));
        Ok(entries)
    }

    /// Stream the contents of an archive to `on_batch` in chunks of up to
    /// [`LIST_BATCH_SIZE`] entries, returning the total number emitted. Reads
    /// borg's `list --json-lines` output line-by-line and never retains more
    /// than one batch at a time, so a 100k-entry archive stays bounded in
    /// memory instead of allocating the whole `Vec` up front. `on_batch` is
    /// invoked synchronously as batches fill; the Tauri layer forwards each one
    /// to the frontend over an `ipc::Channel`.
    pub async fn list_contents_streaming(
        &self,
        repo: &RepoConfig,
        archive_name: &str,
        passphrase: Option<&str>,
        cancel: &CancelToken,
        on_batch: impl Fn(Vec<ArchiveEntry>) + Send,
    ) -> Result<usize> {
        if cancel.is_cancelled() {
            return Err(BorgError::Cancelled);
        }
        let archive = format!("{}::{}", repo.location(), archive_name);

        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["list", "--json-lines"])
            .end_options()
            .arg(&archive);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        // If the read loop times out (or the caller drops this future), the
        // child is dropped without an explicit wait — kill it then rather than
        // leaking a hung borg/ssh process for up to LIST_CONTENTS_TIMEOUT_SECS.
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        // Drain stderr concurrently so a chatty borg can't deadlock on a full
        // pipe, and capture it for the failure path.
        let stderr_capture: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_clone = stderr_capture.clone();
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                stderr_clone
                    .lock()
                    .expect("list stderr mutex poisoned")
                    .push(line);
            }
        });

        // Read stdout line-by-line, parsing each JSON-lines row and flushing a
        // batch to `on_batch` once it fills. Bounded memory: at most one batch
        // (LIST_BATCH_SIZE entries) is held at a time. Malformed lines are
        // skipped, matching the previous collect-and-filter behaviour.
        let mut lines = BufReader::new(stdout).lines();
        let mut batch: Vec<ArchiveEntry> = Vec::with_capacity(LIST_BATCH_SIZE);
        let mut total = 0usize;
        let read = async {
            while let Some(line) = lines.next_line().await? {
                if cancel.is_cancelled() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "operation cancelled",
                    ));
                }
                if let Ok(entry) = serde_json::from_str::<ArchiveEntry>(&line) {
                    batch.push(entry);
                    total += 1;
                    if batch.len() >= LIST_BATCH_SIZE {
                        on_batch(std::mem::replace(
                            &mut batch,
                            Vec::with_capacity(LIST_BATCH_SIZE),
                        ));
                    }
                }
            }
            Ok::<(), std::io::Error>(())
        };
        tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(LIST_CONTENTS_TIMEOUT_SECS), read) => {
                result
                    .map_err(|_| BorgError::Timeout {
                        seconds: LIST_CONTENTS_TIMEOUT_SECS,
                    })?
                    .map_err(|error| {
                        if error.kind() == std::io::ErrorKind::Interrupted && cancel.is_cancelled() {
                            BorgError::Cancelled
                        } else {
                            error.into()
                        }
                    })?;
            }
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = stderr_task.await;
                return Err(BorgError::Cancelled);
            }
        }
        if !batch.is_empty() {
            on_batch(batch);
        }

        let status = tokio::select! {
            status = child.wait() => status?,
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = stderr_task.await;
                return Err(BorgError::Cancelled);
            }
        };
        let _ = stderr_task.await;

        match classify_exit(status.code()) {
            ExitClass::Ok => Ok(total),
            ExitClass::Warning => {
                let stderr = stderr_capture
                    .lock()
                    .expect("list stderr mutex poisoned")
                    .join("\n");
                if !stderr.trim().is_empty() {
                    warn!(
                        "borg list (contents) completed with warnings: {}",
                        stderr.trim()
                    );
                }
                Ok(total)
            }
            ExitClass::Error => Err(BorgError::ProcessFailed {
                message: "borg list (contents) failed".to_string(),
                exit_code: status.code(),
                stderr: stderr_capture
                    .lock()
                    .expect("list stderr mutex poisoned")
                    .join("\n"),
            }),
        }
    }

    /// Compare two archives in the same repository, returning one [`DiffEntry`]
    /// per changed path. `archive_a` is the baseline and `archive_b` the
    /// comparison target; both are archive *names* in `repo`. Generous timeout
    /// because diffing two large archives reads both manifests.
    pub async fn diff_archives(
        &self,
        repo: &RepoConfig,
        archive_a: &str,
        archive_b: &str,
        passphrase: Option<&str>,
    ) -> Result<Vec<DiffEntry>> {
        // borg's diff takes `repo::archive_a` positionally, then the bare name
        // of the second archive.
        let archive_ref = format!("{}::{}", repo.location(), archive_a);

        let mut cmd = self.base_command_with(passphrase);
        cmd.args(["diff", "--json-lines"])
            .end_options()
            .args([archive_ref.as_str(), archive_b]);
        let output = self
            .run_checked(cmd, "diff", Some(LIST_CONTENTS_TIMEOUT_SECS))
            .await?;

        let entries = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_diff_line)
            .collect();

        Ok(entries)
    }

    /// Run `borg compact` to actually free repository space left behind by
    /// `prune`/`delete` (borg only marks segments for deletion until compaction).
    /// Not time-limited — compaction rewrites segments and can take a while on a
    /// large repo. Returns a short human-readable summary (borg's "freed N bytes"
    /// line when present).
    pub async fn compact(&self, repo: &RepoConfig, passphrase: Option<&str>) -> Result<String> {
        let mut cmd = self.base_command_with(passphrase);
        // `--verbose` makes borg emit the "compaction freed about N" summary so
        // the UI can report how much space was reclaimed.
        cmd.args(["compact", "--verbose"])
            .end_options()
            .arg(repo.location());
        let output = self.run_checked(cmd, "compact", None).await?;

        // borg prints the freed-space line to stderr (info log); fall back to
        // stdout just in case, then to a generic message.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let summary = stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .find(|l| l.to_ascii_lowercase().contains("freed"))
            .unwrap_or("Compaction complete.")
            .to_string();
        Ok(summary)
    }
}

/// Builds the argument list for `borg prune` (everything between the binary
/// and the repository location). `--glob-archives` is always emitted so a
/// prune can never silently apply one profile's retention policy to archives
/// created by another profile or machine sharing the repository. This borg
/// build (1.4.x) uses `--glob-archives`; borg 2 renamed it `--match-archives`.
fn prune_args(retention: &crate::config::RetentionConfig, archive_glob: &str) -> Vec<String> {
    let mut args = vec![
        "prune".to_string(),
        "--glob-archives".to_string(),
        archive_glob.to_string(),
    ];
    let keeps = [
        ("--keep-hourly", retention.keep_hourly),
        ("--keep-daily", retention.keep_daily),
        ("--keep-weekly", retention.keep_weekly),
        ("--keep-monthly", retention.keep_monthly),
        ("--keep-yearly", retention.keep_yearly),
    ];
    for (flag, value) in keeps {
        if let Some(n) = value {
            args.push(flag.to_string());
            args.push(n.to_string());
        }
    }
    args
}

fn upload_rate_args(profile: &BackupProfile) -> Vec<String> {
    if profile.repo.is_local() {
        return Vec::new();
    }
    profile
        .upload_limit_kib
        .filter(|limit| *limit > 0)
        .map(|limit| vec!["--upload-ratelimit".into(), limit.to_string()])
        .unwrap_or_default()
}

/// One changed path from `borg diff`. A path may carry several underlying
/// change records (content plus ctime/mtime/mode); they are collapsed into a
/// single [`DiffStatus`] with byte deltas where borg reports them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffEntry {
    pub path: String,
    pub status: DiffStatus,
    /// Bytes added: the new file size for `Added`, or the added byte count for
    /// `Modified`. Zero for `Removed`/`Changed`.
    pub added: u64,
    /// Bytes removed: the old file size for `Removed`, or the removed byte count
    /// for `Modified`. Zero for `Added`/`Changed`.
    pub removed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffStatus {
    /// Present in `archive_b` but not `archive_a`.
    Added,
    /// Present in `archive_a` but not `archive_b`.
    Removed,
    /// Content changed.
    Modified,
    /// Only metadata changed (timestamps, mode, owner) or the entry type changed.
    Changed,
}

/// Parse one line of `borg diff --json-lines` output. Returns `None` for blank
/// or unparseable lines (defensive against borg version differences). The change
/// schema is `{"path": "...", "changes": [{"type": "added"|"removed"|"modified"|
/// "ctime"|"mtime"|..., ...}]}`; unknown change types collapse to
/// [`DiffStatus::Changed`].
fn parse_diff_line(line: &str) -> Option<DiffEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let path = value.get("path")?.as_str()?.to_string();
    let changes = value.get("changes")?.as_array()?;

    let mut status = DiffStatus::Changed;
    let mut added = 0u64;
    let mut removed = 0u64;

    for change in changes {
        let u = |key: &str| {
            change
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        match change.get("type").and_then(|t| t.as_str()) {
            // A whole new file: precedence over everything else.
            Some("added") => {
                status = DiffStatus::Added;
                added = u("size");
            }
            // A deleted file: outranks modified/changed but not added.
            Some("removed") => {
                if status != DiffStatus::Added {
                    status = DiffStatus::Removed;
                }
                removed = u("size");
            }
            // Content changed: outranks metadata-only "changed".
            Some("modified") => {
                if !matches!(status, DiffStatus::Added | DiffStatus::Removed) {
                    status = DiffStatus::Modified;
                }
                added += u("added");
                removed += u("removed");
            }
            // ctime/mtime/mode/owner/type changes leave status at Changed.
            _ => {}
        }
    }

    Some(DiffEntry {
        path,
        status,
        added,
        removed,
    })
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
    fn upload_rate_limit_is_only_passed_for_ssh() {
        let mut profile = BackupProfile {
            name: "test".into(),
            source_paths: vec![],
            excludes: vec![],
            compression: crate::config::Compression::default(),
            repo: test_repo(),
            upload_limit_kib: Some(512),
        };
        assert_eq!(
            upload_rate_args(&profile),
            vec!["--upload-ratelimit", "512"]
        );
        profile.repo.ssh_host.clear();
        profile.repo.ssh_user.clear();
        profile.repo.repo_path = "D:\\repo".into();
        assert!(upload_rate_args(&profile).is_empty());
    }

    #[test]
    fn prune_args_always_scope_with_glob_archives() {
        // Regression for the shared-repo data-loss bug: an unscoped
        // `borg prune` deletes other machines'/profiles' archives.
        let retention = crate::config::RetentionConfig {
            keep_daily: Some(7),
            keep_weekly: Some(4),
            ..Default::default()
        };
        assert_eq!(
            prune_args(&retention, "mybox-default-*"),
            vec![
                "prune",
                "--glob-archives",
                "mybox-default-*",
                "--keep-daily",
                "7",
                "--keep-weekly",
                "4",
            ]
        );
    }

    #[test]
    fn prune_args_emit_every_configured_keep_rule() {
        let retention = crate::config::RetentionConfig {
            keep_hourly: Some(1),
            keep_daily: Some(2),
            keep_weekly: Some(3),
            keep_monthly: Some(4),
            keep_yearly: Some(5),
        };
        let args = prune_args(&retention, "mybox-default-*");
        assert_eq!(args[..3], ["prune", "--glob-archives", "mybox-default-*"]);
        for (flag, n) in [
            ("--keep-hourly", "1"),
            ("--keep-daily", "2"),
            ("--keep-weekly", "3"),
            ("--keep-monthly", "4"),
            ("--keep-yearly", "5"),
        ] {
            let pos = args.iter().position(|a| a == flag).unwrap();
            assert_eq!(args[pos + 1], n);
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
    fn change_passphrase_command_sets_old_and_new_env() {
        let client = BorgClient::new(PathBuf::from("borg"));
        let cmd = client.change_passphrase_command(&test_repo(), "old-pass", "new-pass");
        let std_cmd = cmd.as_std();
        let envs: Vec<_> = std_cmd.get_envs().collect();
        assert!(
            envs.iter()
                .any(|(k, v)| *k == "BORG_PASSPHRASE" && v.is_some_and(|v| v == "old-pass"))
        );
        assert!(
            envs.iter()
                .any(|(k, v)| *k == "BORG_NEW_PASSPHRASE" && v.is_some_and(|v| v == "new-pass"))
        );
        let args: Vec<_> = std_cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "key".to_string(),
                "change-passphrase".to_string(),
                "--".to_string(),
                test_repo().location(),
            ]
        );
    }

    #[tokio::test]
    async fn change_passphrase_rejects_empty_passphrases_before_spawn() {
        // Nonexistent binary on purpose: rejection must happen before any
        // process is spawned — an empty old passphrase means there is nothing
        // to rotate (unencrypted repo or nothing stored), and an empty new
        // passphrase would lock the user out of prompting flows.
        let client = BorgClient::new(PathBuf::from("/nonexistent/borg"));
        for (old, new) in [("", "new-pass"), ("old-pass", ""), ("", "")] {
            let err = client
                .change_passphrase(&test_repo(), old, new)
                .await
                .expect_err("empty passphrase must be rejected");
            match err {
                BorgError::InvalidConfig { message } => {
                    assert!(message.contains("passphrase"), "{message}");
                }
                other => panic!("expected InvalidConfig, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn prune_refuses_unscoped_archive_globs() {
        // Final backstop for the shared-repo data-loss bug: even if a future
        // caller bypasses prune_scoped, the client itself must never issue a
        // prune that can match every archive in the repository. The binary
        // path is nonexistent on purpose — rejection must happen before any
        // process is spawned.
        let client = BorgClient::new(PathBuf::from("/nonexistent/borg"));
        let retention = crate::config::RetentionConfig {
            keep_daily: Some(7),
            ..Default::default()
        };
        for glob in ["", "*", "*-default"] {
            let err = client
                .prune(&test_repo(), &retention, glob, None)
                .await
                .expect_err("unscoped glob must be rejected");
            match err {
                BorgError::InvalidConfig { message } => {
                    assert!(message.contains("unscoped archive glob"), "{message}");
                }
                other => panic!("expected InvalidConfig, got {other:?}"),
            }
        }
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
    fn check_modes_never_include_repair() {
        assert_eq!(CheckMode::Repository.args(), &["check"]);
        assert_eq!(CheckMode::VerifyData.args(), &["check", "--verify-data"]);
        for mode in [CheckMode::Repository, CheckMode::VerifyData] {
            assert!(!mode.args().contains(&"--repair"));
        }
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
    async fn list_contents_streaming_returns_cancelled_before_spawn() {
        let client = BorgClient::new("borg-that-should-not-start".into());
        let repo = RepoConfig {
            ssh_host: String::new(),
            ssh_port: 0,
            ssh_user: String::new(),
            repo_path: "/tmp/repo".into(),
            ssh_key_path: None,
        };
        let cancel = CancelToken::new();
        cancel.cancel();

        let result = client
            .list_contents_streaming(&repo, "archive", None, &cancel, |_| {
                panic!("cancelled listing must not emit batches");
            })
            .await;

        assert!(matches!(result, Err(BorgError::Cancelled)));
    }

    #[test]
    fn parse_diff_line_added_file() {
        let line = r#"{"changes": [{"size": 9, "type": "added"}], "path": "src/c.txt"}"#;
        let entry = parse_diff_line(line).unwrap();
        assert_eq!(entry.path, "src/c.txt");
        assert_eq!(entry.status, DiffStatus::Added);
        assert_eq!(entry.added, 9);
        assert_eq!(entry.removed, 0);
    }

    #[test]
    fn parse_diff_line_removed_file() {
        let line = r#"{"changes": [{"size": 11, "type": "removed"}], "path": "src/b.txt"}"#;
        let entry = parse_diff_line(line).unwrap();
        assert_eq!(entry.status, DiffStatus::Removed);
        assert_eq!(entry.added, 0);
        assert_eq!(entry.removed, 11);
    }

    #[test]
    fn parse_diff_line_modified_with_metadata() {
        // Real borg 1.2.x output: a content change plus ctime/mtime noise.
        let line = r#"{"changes": [{"added": 25, "removed": 9, "type": "modified"}, {"new_ctime": "x", "old_ctime": "y", "type": "ctime"}, {"new_mtime": "x", "old_mtime": "y", "type": "mtime"}], "path": "src/a.txt"}"#;
        let entry = parse_diff_line(line).unwrap();
        assert_eq!(entry.status, DiffStatus::Modified);
        assert_eq!(entry.added, 25);
        assert_eq!(entry.removed, 9);
    }

    #[test]
    fn parse_diff_line_metadata_only_is_changed() {
        let line = r#"{"changes": [{"new_mtime": "x", "old_mtime": "y", "type": "mtime"}], "path": "src"}"#;
        let entry = parse_diff_line(line).unwrap();
        assert_eq!(entry.status, DiffStatus::Changed);
        assert_eq!(entry.added, 0);
        assert_eq!(entry.removed, 0);
    }

    #[test]
    fn parse_diff_line_added_outranks_other_changes() {
        // If both an "added" and a metadata change appear, status is Added.
        let line = r#"{"changes": [{"type": "mtime"}, {"size": 5, "type": "added"}], "path": "f"}"#;
        assert_eq!(parse_diff_line(line).unwrap().status, DiffStatus::Added);
    }

    #[test]
    fn parse_diff_line_skips_blank_and_garbage() {
        assert!(parse_diff_line("").is_none());
        assert!(parse_diff_line("not json").is_none());
        // Missing the required "path"/"changes" keys -> skipped, not panicked.
        assert!(parse_diff_line(r#"{"foo": "bar"}"#).is_none());
    }

    #[test]
    fn diff_entry_serializes_status_lowercase() {
        let entry = DiffEntry {
            path: "x".into(),
            status: DiffStatus::Modified,
            added: 1,
            removed: 2,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"status\":\"modified\""), "got: {json}");
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

    /// Option-injection regression tests: run every borg subcommand against a
    /// fake `borg` that records its argv, and prove the `--` end-of-options
    /// separator immediately precedes the positional arguments. Unix-only
    /// because the fake binary is a shell script; the argv construction under
    /// test is platform-independent.
    #[cfg(unix)]
    mod end_of_options {
        use super::*;
        use crate::config::{BackupProfile, Compression, RetentionConfig};

        /// Serialises these tests. On Linux a `fork` performed by one test can
        /// inherit another test's still-open write fd to its script; the
        /// subsequent `exec` of that script then fails with ETXTBSY.
        static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

        /// Write an executable fake `borg` that dumps its argv (one per line)
        /// to `args.txt` and prints `{}` so JSON-parsing callers succeed.
        fn fake_borg(dir: &Path) -> (PathBuf, PathBuf) {
            use std::os::unix::fs::PermissionsExt;
            let args_file = dir.join("args.txt");
            let script = dir.join("fake-borg");
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho '{{}}'\n",
                    args_file.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            (script, args_file)
        }

        fn recorded_args(path: &Path) -> Vec<String> {
            std::fs::read_to_string(path)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect()
        }

        /// Assert that the recorded argv contains exactly one `--` and that
        /// everything after it equals `positionals` — i.e. option parsing is
        /// terminated before any value that could look like a flag.
        fn assert_separator_then(args: &[String], positionals: &[&str]) {
            let separators = args.iter().filter(|a| *a == "--").count();
            assert_eq!(separators, 1, "expected exactly one `--` in argv: {args:?}");
            let sep = args.iter().position(|a| a == "--").unwrap();
            assert_eq!(
                &args[sep + 1..],
                positionals,
                "positionals after `--` mismatch: {args:?}"
            );
        }

        fn local_repo(path: &str) -> RepoConfig {
            RepoConfig {
                ssh_host: String::new(),
                ssh_port: 0,
                ssh_user: String::new(),
                repo_path: path.into(),
                ssh_key_path: None,
            }
        }

        #[tokio::test]
        async fn info_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            BorgClient::new(borg).info(&repo, None).await.unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo"]);
        }

        #[tokio::test]
        async fn list_archives_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            BorgClient::new(borg)
                .list_archives(&repo, None)
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo"]);
        }

        #[tokio::test]
        async fn list_contents_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            BorgClient::new(borg)
                .list_contents(&repo, "arch", None)
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo::arch"]);
        }

        #[tokio::test]
        async fn delete_archive_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            BorgClient::new(borg)
                .delete_archive(&repo, "arch", None)
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo::arch"]);
        }

        #[tokio::test]
        async fn change_passphrase_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            // A repo path that would be parsed as a flag without the `--`.
            let repo = local_repo("-oProxyCommand=touch /tmp/pwned");
            BorgClient::new(borg)
                .change_passphrase(&repo, "old-pass", "new-pass")
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["-oProxyCommand=touch /tmp/pwned"]);
        }

        #[tokio::test]
        async fn prune_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            let retention = RetentionConfig {
                keep_daily: Some(7),
                ..Default::default()
            };
            BorgClient::new(borg)
                .prune(&repo, &retention, "mybox-default-*", None)
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo"]);
        }

        #[tokio::test]
        async fn init_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            BorgClient::new(borg)
                .init_repo(&repo, "repokey", None)
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo"]);
        }

        #[tokio::test]
        async fn compact_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            BorgClient::new(borg).compact(&repo, None).await.unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo"]);
        }

        #[tokio::test]
        async fn check_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            let cancel = CancelToken::new();
            BorgClient::new(borg)
                .check(&repo, CheckMode::Repository, None, &cancel, |_| {})
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo"]);
        }

        #[tokio::test]
        async fn diff_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            BorgClient::new(borg)
                .diff_archives(&repo, "a", "b", None)
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo::a", "b"]);
        }

        #[tokio::test]
        async fn key_export_and_import_separate_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            let key = dir.path().join("key.txt");
            let client = BorgClient::new(borg);

            client.export_key(&repo, &key, None).await.unwrap();
            assert_separator_then(
                &recorded_args(&args),
                &["/tmp/repo", &key.display().to_string()],
            );

            client.import_key(&repo, &key, None).await.unwrap();
            assert_separator_then(
                &recorded_args(&args),
                &["/tmp/repo", &key.display().to_string()],
            );
        }

        #[tokio::test]
        async fn create_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let profile = BackupProfile {
                name: "test".into(),
                source_paths: vec![PathBuf::from("data"), PathBuf::from("more")],
                excludes: vec!["*.tmp".into()],
                compression: Compression::default(),
                repo: local_repo("/tmp/repo"),
                upload_limit_kib: None,
            };
            let cancel = CancelToken::new();
            BorgClient::new(borg)
                .create(&profile, "arch", None, None, &cancel, |_| {})
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo::arch", "data", "more"]);
        }

        #[tokio::test]
        async fn extract_separates_positionals() {
            let _guard = SPAWN_LOCK.lock().await;
            let dir = tempfile::tempdir().unwrap();
            let (borg, args) = fake_borg(dir.path());
            let repo = local_repo("/tmp/repo");
            let cancel = CancelToken::new();
            BorgClient::new(borg)
                .extract(
                    &repo,
                    "arch",
                    dir.path(),
                    &["docs/file.txt".into()],
                    None,
                    &cancel,
                    |_| {},
                )
                .await
                .unwrap();
            assert_separator_then(&recorded_args(&args), &["/tmp/repo::arch", "docs/file.txt"]);
        }
    }
}
