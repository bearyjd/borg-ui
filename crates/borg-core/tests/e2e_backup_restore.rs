//! End-to-end backup → restore tests against a *real* borg binary.
//!
//! These exercise the actual backup engine the app ships: argument building,
//! `--log-json` progress parsing, exit-code/warning handling, and the
//! `pp:`-pattern selective restore — the parts that must work for a backup to
//! be trustworthy. They use a local on-disk repository (no SSH server needed),
//! which is the same code path the app uses for "Local folder / USB drive"
//! repos and keeps the test hermetic.
//!
//! The tests are **skipped** unless `BORG_TEST_BIN` points at a borg
//! executable, so CI without borg stays green. To run them:
//!
//! ```bash
//! BORG_TEST_BIN=/path/to/borg cargo test -p borg-core --test e2e_backup_restore -- --nocapture
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use borg_core::borg::{BorgClient, CancelToken};
use borg_core::config::{BackupProfile, Compression, RepoConfig};

/// Returns the borg binary to test against, or `None` to skip.
fn borg_bin() -> Option<PathBuf> {
    match std::env::var("BORG_TEST_BIN") {
        Ok(p) if !p.trim().is_empty() => {
            let pb = PathBuf::from(p);
            pb.exists().then_some(pb)
        }
        _ => None,
    }
}

/// Skip the test (return) if no borg binary is configured.
macro_rules! borg_or_skip {
    () => {
        match borg_bin() {
            Some(b) => BorgClient::new(b),
            None => {
                eprintln!(
                    "SKIP: set BORG_TEST_BIN to a borg executable to run end-to-end backup tests"
                );
                return;
            }
        }
    };
}

fn local_repo(path: &Path) -> RepoConfig {
    RepoConfig {
        ssh_host: String::new(),
        ssh_port: 0,
        ssh_user: String::new(),
        repo_path: path.to_string_lossy().into_owned(),
        ssh_key_path: None,
    }
}

fn profile(repo: RepoConfig, sources: Vec<PathBuf>, excludes: Vec<String>) -> BackupProfile {
    BackupProfile {
        name: "e2e".into(),
        source_paths: sources,
        excludes,
        compression: Compression::Zstd { level: 3 },
        repo,
    }
}

/// init → create → list → list-contents → extract → byte-for-byte verify,
/// with no encryption.
#[tokio::test]
async fn unencrypted_roundtrip_preserves_file_contents() {
    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    let out = tmp.path().join("out");
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("alpha.txt"), b"alpha contents").unwrap();
    fs::write(
        src.join("sub").join("beta.bin"),
        vec![0u8, 1, 2, 3, 255, 254],
    )
    .unwrap();
    fs::create_dir_all(&out).unwrap();

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();

    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    let outcome = client
        .create(&prof, "daily-1", Some(&src), None, &cancel, |_| {})
        .await
        .expect("backup should succeed");
    assert!(
        !outcome.had_warnings(),
        "clean backup should have no warnings"
    );

    let archives = client.list_archives(&repo, None).await.unwrap();
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].name, "daily-1");

    let contents = client.list_contents(&repo, "daily-1", None).await.unwrap();
    assert!(
        contents.iter().any(|e| e.path.ends_with("alpha.txt")),
        "archive listing should include alpha.txt, got: {:?}",
        contents.iter().map(|e| &e.path).collect::<Vec<_>>()
    );

    client
        .extract(&repo, "daily-1", &out, &[], None, &cancel, |_| {})
        .await
        .expect("restore should succeed");

    assert_eq!(fs::read(out.join("alpha.txt")).unwrap(), b"alpha contents");
    assert_eq!(
        fs::read(out.join("sub").join("beta.bin")).unwrap(),
        vec![0u8, 1, 2, 3, 255, 254]
    );
}

/// The same roundtrip against an encrypted repository (repokey-blake2 +
/// passphrase) — the recommended production configuration.
#[tokio::test]
async fn encrypted_roundtrip_with_passphrase() {
    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    let out = tmp.path().join("out");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("secret.txt"), b"top secret data").unwrap();
    fs::create_dir_all(&out).unwrap();

    let repo = local_repo(&repo_path);
    let pass = Some("correct horse battery staple");
    client
        .init_repo(&repo, "repokey-blake2", pass)
        .await
        .expect("encrypted init should succeed");

    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    client
        .create(&prof, "enc-1", Some(&src), pass, &cancel, |_| {})
        .await
        .expect("encrypted backup should succeed");

    // Listing without the passphrase must fail (proves encryption is real).
    assert!(
        client.list_archives(&repo, None).await.is_err(),
        "listing an encrypted repo without a passphrase should fail"
    );

    let archives = client.list_archives(&repo, pass).await.unwrap();
    assert_eq!(archives.len(), 1);

    client
        .extract(&repo, "enc-1", &out, &[], pass, &cancel, |_| {})
        .await
        .expect("encrypted restore should succeed");
    assert_eq!(
        fs::read(out.join("secret.txt")).unwrap(),
        b"top secret data"
    );
}

/// Selective restore: extracting a single path must restore only that file.
#[tokio::test]
async fn selective_restore_extracts_only_requested_paths() {
    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    let out = tmp.path().join("out");
    fs::create_dir_all(src.join("keep")).unwrap();
    fs::create_dir_all(src.join("skip")).unwrap();
    fs::write(src.join("keep").join("wanted.txt"), b"wanted").unwrap();
    fs::write(src.join("skip").join("unwanted.txt"), b"unwanted").unwrap();
    fs::create_dir_all(&out).unwrap();

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();
    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    client
        .create(&prof, "sel-1", Some(&src), None, &cancel, |_| {})
        .await
        .unwrap();

    // Discover the exact stored path for the wanted file, then restore just it.
    let contents = client.list_contents(&repo, "sel-1", None).await.unwrap();
    let wanted = contents
        .iter()
        .find(|e| e.path.ends_with("wanted.txt"))
        .expect("wanted.txt should be in the archive")
        .path
        .clone();

    client
        .extract(&repo, "sel-1", &out, &[wanted], None, &cancel, |_| {})
        .await
        .unwrap();

    assert!(
        out.join("keep").join("wanted.txt").exists(),
        "selected file should be restored"
    );
    assert!(
        !out.join("skip").join("unwanted.txt").exists(),
        "unselected file must NOT be restored"
    );
}

/// A source file the OS won't let borg read must NOT fail the whole backup:
/// borg exits 1 (warning), the archive is still created, and the readable
/// files restore cleanly. This is the locked-file scenario (Outlook PST,
/// browser profiles, in-use Office docs) that is common on Windows.
#[cfg(unix)]
#[tokio::test]
async fn unreadable_file_yields_warning_not_failure() {
    use std::os::unix::fs::PermissionsExt;

    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    let out = tmp.path().join("out");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("readable.txt"), b"i am readable").unwrap();
    let locked = src.join("locked.txt");
    fs::write(&locked, b"cannot read me").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    fs::create_dir_all(&out).unwrap();

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();
    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();

    let outcome = client
        .create(&prof, "warn-1", Some(&src), None, &cancel, |_| {})
        .await
        .expect("backup with an unreadable file must still succeed (borg rc=1)");
    assert!(
        outcome.had_warnings(),
        "an unreadable file should surface a warning"
    );

    // The archive exists and the readable file restores.
    let archives = client.list_archives(&repo, None).await.unwrap();
    assert_eq!(archives.len(), 1);
    client
        .extract(&repo, "warn-1", &out, &[], None, &cancel, |_| {})
        .await
        .expect("restore of a warning archive should succeed");
    assert_eq!(
        fs::read(out.join("readable.txt")).unwrap(),
        b"i am readable"
    );

    // Restore permissions so the tempdir can be cleaned up.
    let _ = fs::set_permissions(&locked, fs::Permissions::from_mode(0o644));
}

/// Filenames with characters that are fnmatch-significant (`?`, `*`) must
/// round-trip through selective restore thanks to the `pp:` literal pattern.
#[cfg(unix)]
#[tokio::test]
async fn special_character_filename_roundtrips() {
    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    let out = tmp.path().join("out");
    fs::create_dir_all(&src).unwrap();
    let tricky = "what's up?.txt";
    fs::write(src.join(tricky), b"tricky name").unwrap();
    fs::create_dir_all(&out).unwrap();

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();
    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    client
        .create(&prof, "spec-1", Some(&src), None, &cancel, |_| {})
        .await
        .unwrap();

    let contents = client.list_contents(&repo, "spec-1", None).await.unwrap();
    let stored = contents
        .iter()
        .find(|e| e.path.contains("up?"))
        .expect("tricky filename should be stored")
        .path
        .clone();

    client
        .extract(&repo, "spec-1", &out, &[stored], None, &cancel, |_| {})
        .await
        .unwrap();
    assert_eq!(fs::read(out.join(tricky)).unwrap(), b"tricky name");
}

/// prune runs cleanly and delete removes a specific archive.
#[tokio::test]
async fn prune_and_delete_manage_archives() {
    use borg_core::config::RetentionConfig;

    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("f.txt"), b"data").unwrap();

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();
    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    client
        .create(&prof, "a1", Some(&src), None, &cancel, |_| {})
        .await
        .unwrap();
    client
        .create(&prof, "a2", Some(&src), None, &cancel, |_| {})
        .await
        .unwrap();

    // A keep-all prune must succeed and remove nothing.
    let retention = RetentionConfig {
        keep_daily: Some(100),
        ..Default::default()
    };
    let prune_warnings = client.prune(&repo, &retention, None).await.unwrap();
    assert!(
        prune_warnings.warnings.is_empty(),
        "a keep-all prune should report no warnings"
    );
    assert_eq!(client.list_archives(&repo, None).await.unwrap().len(), 2);

    // Deleting a1 leaves only a2.
    client.delete_archive(&repo, "a1", None).await.unwrap();
    let remaining = client.list_archives(&repo, None).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].name, "a2");
}

/// diff between two archives reports added / removed / modified paths.
#[tokio::test]
async fn diff_reports_added_removed_and_modified() {
    use borg_core::borg::DiffStatus;

    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("keep.txt"), b"unchanged").unwrap();
    fs::write(src.join("change.txt"), b"original").unwrap();
    fs::write(src.join("gone.txt"), b"will be removed").unwrap();

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();
    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    client
        .create(&prof, "before", Some(&src), None, &cancel, |_| {})
        .await
        .unwrap();

    // Mutate the source: modify one file, delete one, add one.
    fs::write(src.join("change.txt"), b"original plus much more content").unwrap();
    fs::remove_file(src.join("gone.txt")).unwrap();
    fs::write(src.join("fresh.txt"), b"brand new").unwrap();
    client
        .create(&prof, "after", Some(&src), None, &cancel, |_| {})
        .await
        .unwrap();

    let diff = client
        .diff_archives(&repo, "before", "after", None)
        .await
        .expect("diff should succeed");

    let find = |needle: &str| diff.iter().find(|e| e.path.ends_with(needle));
    assert_eq!(
        find("fresh.txt").map(|e| e.status),
        Some(DiffStatus::Added),
        "fresh.txt should be Added; diff: {diff:?}"
    );
    assert_eq!(
        find("gone.txt").map(|e| e.status),
        Some(DiffStatus::Removed),
        "gone.txt should be Removed; diff: {diff:?}"
    );
    let changed = find("change.txt").expect("change.txt should be in the diff");
    assert_eq!(changed.status, DiffStatus::Modified);
    assert!(
        changed.added > 0,
        "a longer file should report added bytes; got {changed:?}"
    );
    // An unchanged file should not appear as a content change.
    assert!(
        find("keep.txt").is_none_or(|e| e.status == DiffStatus::Changed),
        "keep.txt content must not change; got {:?}",
        find("keep.txt")
    );
}

/// compact runs cleanly after a delete and returns a summary string.
#[tokio::test]
async fn compact_runs_after_delete() {
    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("data.bin"), vec![7u8; 4096]).unwrap();

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();
    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    client
        .create(&prof, "c1", Some(&src), None, &cancel, |_| {})
        .await
        .unwrap();
    client.delete_archive(&repo, "c1", None).await.unwrap();

    let summary = client
        .compact(&repo, None)
        .await
        .expect("compact should succeed on a valid repo");
    assert!(!summary.is_empty(), "compact should return a summary line");

    // The repo is still usable afterwards (list works, returns no archives).
    let archives = client.list_archives(&repo, None).await.unwrap();
    assert!(archives.is_empty(), "c1 was deleted before compaction");
}

/// `list_contents_streaming` must emit every entry (in batches) and reassemble
/// to exactly the same set as the collected `list_contents`, returning a total
/// that matches. Backs the virtualized archive browser, where the frontend
/// rebuilds the tree from the streamed batches.
#[tokio::test]
async fn streaming_list_matches_collected_listing() {
    use std::sync::{Arc, Mutex};

    let client = borg_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    let src = tmp.path().join("src");

    // Just over LIST_BATCH_SIZE (5000) entries across nested directories, so the
    // mid-stream batch-flush path is actually exercised (>= 2 batches), not just
    // the final partial flush — this multi-batch case is the whole point of the
    // feature. 6 dirs x 850 files = 5100 files + their dirs.
    for d in 0..6 {
        let dir = src.join(format!("dir{d:02}"));
        fs::create_dir_all(&dir).unwrap();
        for f in 0..850 {
            fs::write(dir.join(format!("file{f:04}.txt")), format!("d{d}-f{f}")).unwrap();
        }
    }

    let repo = local_repo(&repo_path);
    client.init_repo(&repo, "none", None).await.unwrap();
    let prof = profile(repo.clone(), vec![PathBuf::from(".")], vec![]);
    let cancel = CancelToken::new();
    client
        .create(&prof, "big-1", Some(&src), None, &cancel, |_| {})
        .await
        .expect("backup should succeed");

    // Collected baseline.
    let collected = client.list_contents(&repo, "big-1", None).await.unwrap();
    assert!(
        collected.len() >= 5100,
        "expected at least the 5100 files we wrote, got {}",
        collected.len()
    );

    // Streamed: accumulate batches and count how many arrived.
    let streamed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let batches = Arc::new(Mutex::new(0usize));
    let sink = streamed.clone();
    let batch_counter = batches.clone();
    let total = client
        .list_contents_streaming(&repo, "big-1", None, move |batch| {
            assert!(!batch.is_empty(), "batches should never be empty");
            *batch_counter.lock().unwrap() += 1;
            sink.lock()
                .unwrap()
                .extend(batch.into_iter().map(|e| e.path));
        })
        .await
        .expect("streaming list should succeed");

    let streamed_paths = Arc::try_unwrap(streamed).unwrap().into_inner().unwrap();
    assert_eq!(
        total,
        collected.len(),
        "returned total must equal the number of entries"
    );
    assert_eq!(
        total,
        streamed_paths.len(),
        "every entry must arrive in some batch"
    );
    assert!(
        *batches.lock().unwrap() >= 2,
        "with 5100+ entries and LIST_BATCH_SIZE=5000 the mid-stream flush must \
         fire, so at least 2 batches should have been emitted, got {}",
        *batches.lock().unwrap()
    );
    // Order-preserving reassembly: streamed order matches the collected order.
    let collected_paths: Vec<&str> = collected.iter().map(|e| e.path.as_str()).collect();
    let streamed_refs: Vec<&str> = streamed_paths.iter().map(String::as_str).collect();
    assert_eq!(
        streamed_refs, collected_paths,
        "streamed paths must match the collected listing exactly, in order"
    );
}
