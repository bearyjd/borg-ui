use std::path::{Path, PathBuf};

use borg_core::error::{BorgError, Result};

pub struct SnapshotHandle {
    pub shadow_path: PathBuf,
    pub snapshot_id: String,
    pub volume: String,
}

/// Extract the volume root from a Windows path (e.g., `C:\Users\me` → `C:\`).
pub fn extract_volume(path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    let bytes = path_str.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return Ok(format!("{}:\\", (bytes[0] as char).to_ascii_uppercase()));
    }
    Err(BorgError::InvalidConfig {
        message: format!("cannot extract volume from path: {}", path_str),
    })
}

/// Collect unique volumes from a list of paths.
pub fn unique_volumes(paths: &[PathBuf]) -> Result<Vec<String>> {
    let mut volumes: Vec<String> = Vec::new();
    for path in paths {
        let vol = extract_volume(path)?;
        if !volumes.contains(&vol) {
            volumes.push(vol);
        }
    }
    Ok(volumes)
}

/// Strip the volume root from an absolute Windows path, yielding the path
/// relative to that volume. This is what borg stores when it runs with its
/// working directory set to the snapshot junction (Approach B): a clean,
/// restorable path with no drive-letter colon or shadow-device prefix.
///
/// `C:\Users\me\docs` on volume `C:\` becomes `Users\me\docs`. Backing up the
/// volume root itself yields `.` (borg archives the whole junction).
pub fn volume_relative(original: &Path, volume_root: &str) -> Result<PathBuf> {
    let original_str = original.to_string_lossy();
    if original_str.len() < volume_root.len()
        || !original_str[..volume_root.len()].eq_ignore_ascii_case(volume_root)
    {
        return Err(BorgError::InvalidConfig {
            message: format!("path {} is not on volume {}", original_str, volume_root),
        });
    }
    let relative = original_str[volume_root.len()..].trim_start_matches(['\\', '/']);
    if relative.is_empty() {
        return Ok(PathBuf::from("."));
    }
    Ok(PathBuf::from(relative))
}

/// The source path to hand borg under a *drive-letter-named* snapshot junction,
/// so the stored archive path is byte-identical to borg's live-file layout.
///
/// borg stores an absolute live source `C:\Users\me\docs` as `C/Users/me/docs`
/// (drive letter as the leading component). Under VSS we mount the shadow as a
/// junction named `C` inside a wrapper dir, set that wrapper as borg's cwd, and
/// pass this `C\Users\me\docs` as the source — so borg stores the SAME
/// `C/Users/me/docs`. This keeps VSS invisible to excludes, archive browsing,
/// and restore (the same profile produces the same layout whether or not VSS
/// engaged on a given run). `C:\` (the volume root) maps to just `C`.
pub fn drive_relative_source(original: &Path, volume_root: &str) -> Result<PathBuf> {
    let rel = volume_relative(original, volume_root)?;
    let letter = volume_root
        .chars()
        .next()
        .unwrap_or('C')
        .to_ascii_uppercase();
    if rel == Path::new(".") {
        return Ok(PathBuf::from(letter.to_string()));
    }
    let mut s = std::ffi::OsString::from(letter.to_string());
    s.push("\\");
    s.push(rel.as_os_str());
    Ok(PathBuf::from(s))
}

fn validate_volume(volume: &str) -> Result<()> {
    let bytes = volume.as_bytes();
    if bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        return Ok(());
    }
    Err(BorgError::InvalidConfig {
        message: format!("invalid volume root: {}", volume),
    })
}

fn validate_shadow_id(id: &str) -> Result<()> {
    let bytes = id.as_bytes();
    if bytes.len() != 38 || bytes[0] != b'{' || bytes[37] != b'}' {
        return Err(BorgError::InvalidConfig {
            message: "invalid shadow copy ID format".into(),
        });
    }
    let inner = &id[1..37];
    for (i, c) in inner.chars().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if c != '-' {
                    return Err(BorgError::InvalidConfig {
                        message: "invalid shadow copy ID format".into(),
                    });
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return Err(BorgError::InvalidConfig {
                        message: "invalid shadow copy ID format".into(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Create a VSS snapshot of the given volume.
///
/// On Windows, shells out to PowerShell using Win32_ShadowCopy WMI class.
/// On non-Windows, returns `Ok(None)` (backups run against live files).
/// Requires administrator privileges on Windows.
pub async fn create_snapshot(volume: &Path) -> Result<Option<SnapshotHandle>> {
    let vol_str = extract_volume(volume)?;
    validate_volume(&vol_str)?;

    #[cfg(not(windows))]
    {
        let _ = vol_str;
        tracing::warn!("VSS not available on this platform — backing up live files");
        Ok(None)
    }

    #[cfg(windows)]
    {
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             $r=(Get-WmiObject -List Win32_ShadowCopy).Create('{vol}','ClientAccessible'); \
             if($r.ReturnValue -ne 0){{Write-Error \"VSS failed: $($r.ReturnValue)\";exit 1}}; \
             $s=Get-WmiObject Win32_ShadowCopy|Where-Object{{$_.ID -eq $r.ShadowID}}; \
             Write-Output \"$($s.ID)|$($s.DeviceObject)\"",
            vol = vol_str
        );

        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("VSS snapshot failed: {}", stderr.trim());
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.trim();
        let (id, device) = line
            .split_once('|')
            .ok_or_else(|| BorgError::ProcessFailed {
                message: "unexpected VSS output format".into(),
                exit_code: None,
                stderr: line.into(),
            })?;

        validate_shadow_id(id)?;

        tracing::info!("VSS snapshot created: {} → {}", vol_str, device);
        Ok(Some(SnapshotHandle {
            shadow_path: PathBuf::from(device),
            snapshot_id: id.to_string(),
            volume: vol_str,
        }))
    }
}

/// Release a VSS snapshot by its ID.
pub async fn release_snapshot(handle: SnapshotHandle) -> Result<()> {
    validate_shadow_id(&handle.snapshot_id)?;

    #[cfg(not(windows))]
    {
        let _ = handle;
        Ok(())
    }

    #[cfg(windows)]
    {
        let script = format!(
            "Get-WmiObject Win32_ShadowCopy | \
             Where-Object {{ $_.ID -eq '{}' }} | \
             ForEach-Object {{ $_.Delete() }}",
            handle.snapshot_id
        );

        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                "VSS release failed for {}: {}",
                handle.snapshot_id,
                stderr.trim()
            );
        } else {
            tracing::info!("VSS snapshot released: {}", handle.snapshot_id);
        }
        Ok(())
    }
}

/// A borg invocation plan: the source paths and optional working directory to
/// hand `BorgClient::create`, plus the snapshot/junction state to release after.
///
/// When a VSS snapshot is active, `source_paths` are volume-relative and `cwd`
/// is the snapshot junction mount — so borg stores clean, restorable paths.
/// Otherwise this is a live-file plan: the original paths and no `cwd`. The
/// caller MUST call [`SnapshotPlan::release`] once the backup finishes (success
/// or failure) to remove the junction and delete the snapshot.
pub struct SnapshotPlan {
    /// Paths to pass to borg. Under VSS these are drive-letter-prefixed and
    /// resolved against `cwd` (the junction's wrapper dir) so the stored layout
    /// matches a live backup (`C\Users\me`); otherwise the original paths.
    pub source_paths: Vec<PathBuf>,
    /// Working directory for borg: the snapshot junction's wrapper dir under VSS,
    /// else `None`.
    pub cwd: Option<PathBuf>,
    handles: Vec<SnapshotHandle>,
    mounts: Vec<PathBuf>,
}

impl SnapshotPlan {
    /// A live-file plan: back up the originals directly, no snapshot.
    fn live(paths: &[PathBuf]) -> Self {
        Self {
            source_paths: paths.to_vec(),
            cwd: None,
            handles: Vec::new(),
            mounts: Vec::new(),
        }
    }

    /// True when a VSS snapshot backs this plan (vs. a live-file fallback).
    pub fn is_snapshot(&self) -> bool {
        self.cwd.is_some()
    }

    /// Remove the junction(s) then delete the snapshot(s). Best-effort: errors
    /// are logged, never propagated — releasing must not fail a backup.
    ///
    /// This is an explicit call, not a `Drop` guard (release is async and Drop
    /// cannot await). Both callers invoke it after `borg.create(...).await` on
    /// every happy/error path, so the only leak window is a panic/unwind between
    /// `prepare_snapshot` and here — rare, and self-limiting: `mount_snapshot`
    /// clears a stale junction on the next run and a leaked shadow copy is
    /// reclaimed on reboot. A `Drop`-based safety net is a possible follow-up.
    pub async fn release(self) {
        for mount in &self.mounts {
            #[cfg(windows)]
            {
                unmount_snapshot(mount).await;
            }
            #[cfg(not(windows))]
            {
                let _ = mount;
            }
        }
        release_all(self.handles).await;
    }
}

/// Build a backup plan for `paths`, taking a VSS snapshot when possible.
///
/// Approach B (junction): snapshot the single volume the sources live on, mount
/// the shadow copy as an NTFS junction, and return volume-relative source paths
/// plus the junction as borg's working directory. borg then stores paths that
/// match the live-file layout (`C/Users/me/docs/...`) and restore correctly, and
/// exclusively-locked files are captured from the frozen snapshot.
///
/// Falls back to a live-file plan (today's behavior) — logging why — when:
/// - not on Windows;
/// - the sources span more than one volume (borg takes a single working dir);
/// - VSS is unavailable or snapshot creation fails (e.g. a non-admin user);
/// - the junction can't be mounted or a path can't be made volume-relative.
pub async fn prepare_snapshot(paths: &[PathBuf]) -> SnapshotPlan {
    #[cfg(not(windows))]
    {
        SnapshotPlan::live(paths)
    }

    #[cfg(windows)]
    {
        let volumes = match unique_volumes(paths) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("VSS skipped (cannot determine source volume): {e}");
                return SnapshotPlan::live(paths);
            }
        };
        // borg runs with a single working directory, so one snapshot can only
        // cover one volume. Multi-volume backups fall back to live files.
        if volumes.len() != 1 {
            tracing::info!(
                "VSS skipped: sources span {} volumes (multi-volume snapshot unsupported); backing up live files",
                volumes.len()
            );
            return SnapshotPlan::live(paths);
        }
        let volume = volumes[0].clone();

        let handle = match create_snapshot(Path::new(&volume)).await {
            Ok(Some(h)) => h,
            Ok(None) => {
                tracing::warn!("VSS unavailable for {volume}; backing up live files");
                return SnapshotPlan::live(paths);
            }
            Err(e) => {
                tracing::warn!("VSS snapshot of {volume} failed: {e}; backing up live files");
                return SnapshotPlan::live(paths);
            }
        };

        // Mount the shadow as a junction named after the drive letter; borg runs
        // with the junction's PARENT as cwd and `C\<rel>` sources, so stored paths
        // match the live-file layout exactly (see drive_relative_source).
        let junction = match mount_snapshot(&handle).await {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("VSS junction mount failed: {e}; backing up live files");
                let _ = release_snapshot(handle).await;
                return SnapshotPlan::live(paths);
            }
        };
        let cwd = match junction.parent() {
            Some(p) => p.to_path_buf(),
            None => {
                tracing::warn!("VSS junction has no parent dir; backing up live files");
                unmount_snapshot(&junction).await;
                let _ = release_snapshot(handle).await;
                return SnapshotPlan::live(paths);
            }
        };

        let mut sources = Vec::with_capacity(paths.len());
        for path in paths {
            match drive_relative_source(path, &volume) {
                Ok(src) => sources.push(src),
                Err(e) => {
                    tracing::warn!("VSS path remap failed: {e}; backing up live files");
                    unmount_snapshot(&junction).await;
                    let _ = release_snapshot(handle).await;
                    return SnapshotPlan::live(paths);
                }
            }
        }

        tracing::info!(
            "VSS snapshot active: {volume} -> {} (junction {})",
            handle.shadow_path.display(),
            junction.display()
        );
        SnapshotPlan {
            source_paths: sources,
            cwd: Some(cwd),
            handles: vec![handle],
            mounts: vec![junction],
        }
    }
}

/// Mount a VSS shadow copy as an NTFS junction *named after the drive letter*,
/// inside a per-process wrapper dir under `%TEMP%`. Returns the junction path; its
/// parent (the wrapper) is borg's working directory. Naming the junction `C` makes
/// borg store `C/...` paths identical to the live-file layout, so VSS stays
/// invisible to excludes / browsing / restore.
///
/// `mklink /J` is given the shadow device path WITH a trailing backslash — the
/// form validated against `\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopyN`
/// (see `tests/smoke-windows/validate-vss-spike.ps1`). The junction is a plain
/// reparse point; [`unmount_snapshot`] removes only the link + wrapper, never the
/// shadow. The wrapper name carries the full snapshot GUID + this process's pid so
/// two backup processes sharing `%TEMP%` can never collide (the cancel slot only
/// guards backups within one app instance).
#[cfg(windows)]
pub async fn mount_snapshot(handle: &SnapshotHandle) -> Result<PathBuf> {
    let letter = handle
        .volume
        .chars()
        .next()
        .unwrap_or('C')
        .to_ascii_uppercase();
    let id = handle.snapshot_id.trim_matches(|c| c == '{' || c == '}');
    let base = std::env::temp_dir().join(format!("BorgVSS-{}-{}", std::process::id(), id));
    let junction = base.join(letter.to_string());

    // Clear any stale junction + wrapper left by a prior crashed run of THIS process.
    unmount_snapshot(&junction).await;
    std::fs::create_dir_all(&base)?;

    let target = format!(
        "{}\\",
        handle.shadow_path.to_string_lossy().trim_end_matches('\\')
    );
    let output = tokio::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .await?;

    // Check the reparse point itself (symlink_metadata, like unmount_snapshot)
    // rather than `exists()`, which would follow the junction and stat the shadow
    // -- real I/O that can spuriously fail. The mklink exit code is authoritative.
    if !output.status.success() || std::fs::symlink_metadata(&junction).is_err() {
        // mklink failed, so the wrapper holds no junction -> safe to remove empty.
        let _ = std::fs::remove_dir(&base);
        return Err(BorgError::ProcessFailed {
            message: "mklink /J could not mount the VSS snapshot".into(),
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(junction)
}

/// Remove a snapshot junction created by [`mount_snapshot`] and its empty wrapper
/// dir. `rmdir` deletes only the reparse point — the shadow copy behind it is
/// untouched. Best-effort: failures are logged, never propagated.
#[cfg(windows)]
pub async fn unmount_snapshot(junction: &Path) {
    // symlink_metadata does not traverse the reparse point, so a dangling junction
    // (shadow already gone) still reports as present and gets cleaned; a genuinely
    // absent junction is a no-op (no spurious warning).
    if std::fs::symlink_metadata(junction).is_ok() {
        match tokio::process::Command::new("cmd")
            .args(["/c", "rmdir"])
            .arg(junction)
            .output()
            .await
        {
            Ok(o) if !o.status.success() => tracing::warn!(
                "failed to remove VSS junction {}: {}",
                junction.display(),
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) => tracing::warn!("failed to remove VSS junction {}: {e}", junction.display()),
            _ => {}
        }
    }
    // Remove the now-empty per-process wrapper dir we created for the junction
    // (remove_dir is non-recursive, so it never touches the shadow contents).
    if let Some(base) = junction.parent() {
        let _ = std::fs::remove_dir(base);
    }
}

/// Release all snapshot handles, logging but not propagating errors.
pub async fn release_all(handles: Vec<SnapshotHandle>) {
    for handle in handles {
        if let Err(e) = release_snapshot(handle).await {
            tracing::warn!("Failed to release VSS snapshot: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_volume_from_drive_path() {
        assert_eq!(
            extract_volume(Path::new("C:\\Users\\me\\docs")).unwrap(),
            "C:\\"
        );
    }

    #[test]
    fn extracts_volume_uppercase() {
        assert_eq!(extract_volume(Path::new("d:\\data")).unwrap(), "D:\\");
    }

    #[test]
    fn extracts_volume_forward_slash() {
        assert_eq!(
            extract_volume(Path::new("E:/backups/repo")).unwrap(),
            "E:\\"
        );
    }

    #[test]
    fn rejects_relative_path() {
        assert!(extract_volume(Path::new("relative/path")).is_err());
    }

    #[test]
    fn rejects_unc_path() {
        assert!(extract_volume(Path::new("\\\\server\\share")).is_err());
    }

    #[test]
    fn rejects_empty_path() {
        assert!(extract_volume(Path::new("")).is_err());
    }

    #[test]
    fn rejects_bare_drive_letter() {
        assert!(extract_volume(Path::new("C:")).is_err());
    }

    #[test]
    fn unique_volumes_deduplicates() {
        let paths = vec![
            PathBuf::from("C:\\Users\\me\\docs"),
            PathBuf::from("C:\\Users\\me\\pics"),
            PathBuf::from("D:\\data"),
            PathBuf::from("c:\\other"),
        ];
        let vols = unique_volumes(&paths).unwrap();
        assert_eq!(vols, vec!["C:\\", "D:\\"]);
    }

    #[test]
    fn unique_volumes_empty_input() {
        let vols = unique_volumes(&[]).unwrap();
        assert!(vols.is_empty());
    }

    #[test]
    fn volume_relative_strips_volume_root() {
        assert_eq!(
            volume_relative(Path::new("C:\\Users\\me\\docs"), "C:\\").unwrap(),
            PathBuf::from("Users\\me\\docs")
        );
    }

    #[test]
    fn volume_relative_root_itself_is_dot() {
        assert_eq!(
            volume_relative(Path::new("C:\\"), "C:\\").unwrap(),
            PathBuf::from(".")
        );
    }

    #[test]
    fn volume_relative_rejects_wrong_volume() {
        assert!(volume_relative(Path::new("D:\\data"), "C:\\").is_err());
    }

    #[test]
    fn volume_relative_is_case_insensitive() {
        assert_eq!(
            volume_relative(Path::new("c:\\users\\me"), "C:\\").unwrap(),
            PathBuf::from("users\\me")
        );
    }

    #[test]
    fn volume_relative_trims_extra_separators() {
        assert_eq!(
            volume_relative(Path::new("C:\\\\Users"), "C:\\").unwrap(),
            PathBuf::from("Users")
        );
    }

    // drive_relative_source prepends the drive letter so borg stores the SAME
    // layout as a live (non-VSS) backup -- `C:\Users\me` -> `C\Users\me` ->
    // stored `C/Users/me`. Keeps VSS invisible to excludes / browsing / restore.
    #[test]
    fn drive_relative_source_prepends_letter() {
        assert_eq!(
            drive_relative_source(Path::new("C:\\Users\\me\\docs"), "C:\\").unwrap(),
            PathBuf::from("C\\Users\\me\\docs")
        );
    }

    #[test]
    fn drive_relative_source_volume_root_is_bare_letter() {
        assert_eq!(
            drive_relative_source(Path::new("C:\\"), "C:\\").unwrap(),
            PathBuf::from("C")
        );
    }

    #[test]
    fn drive_relative_source_uses_other_letters() {
        assert_eq!(
            drive_relative_source(Path::new("D:\\data\\db"), "D:\\").unwrap(),
            PathBuf::from("D\\data\\db")
        );
    }

    #[test]
    fn drive_relative_source_uppercases_letter() {
        assert_eq!(
            drive_relative_source(Path::new("c:\\users\\me"), "c:\\").unwrap(),
            PathBuf::from("C\\users\\me")
        );
    }

    #[test]
    fn drive_relative_source_rejects_wrong_volume() {
        assert!(drive_relative_source(Path::new("D:\\data"), "C:\\").is_err());
    }

    // Off Windows, prepare_snapshot can never take a real VSS snapshot, so it
    // must yield a live-file plan: the originals, no working directory. (On
    // Windows the same fallback path covers non-admin / multi-volume, but a unit
    // test there could create a real snapshot, so we gate this to non-Windows.)
    #[cfg(not(windows))]
    #[tokio::test]
    async fn prepare_snapshot_falls_back_to_live_plan_off_windows() {
        let paths = vec![PathBuf::from("C:\\Users\\me\\docs")];
        let plan = prepare_snapshot(&paths).await;
        assert_eq!(plan.source_paths, paths);
        assert!(plan.cwd.is_none());
        assert!(!plan.is_snapshot());
        plan.release().await;
    }

    #[test]
    fn validates_correct_shadow_id() {
        assert!(validate_shadow_id("{B7E24B70-49FD-4B42-B1F1-AE4A0E68B22D}").is_ok());
    }

    #[test]
    fn validates_lowercase_shadow_id() {
        assert!(validate_shadow_id("{b7e24b70-49fd-4b42-b1f1-ae4a0e68b22d}").is_ok());
    }

    #[test]
    fn rejects_shadow_id_without_braces() {
        assert!(validate_shadow_id("B7E24B70-49FD-4B42-B1F1-AE4A0E68B22D").is_err());
    }

    #[test]
    fn rejects_shadow_id_wrong_length() {
        assert!(validate_shadow_id("{short}").is_err());
    }

    #[test]
    fn rejects_shadow_id_with_invalid_chars() {
        assert!(validate_shadow_id("{ZZZZZZZZ-49FD-4B42-B1F1-AE4A0E68B22D}").is_err());
    }

    #[test]
    fn rejects_shadow_id_injection_attempt() {
        assert!(validate_shadow_id("{B7E24B70-49FD-4B42-B1F1-AE4A0E68B22D}'; DROP TABLE").is_err());
    }

    #[test]
    fn validates_correct_volume() {
        assert!(validate_volume("C:\\").is_ok());
        assert!(validate_volume("D:\\").is_ok());
    }

    #[test]
    fn rejects_invalid_volumes() {
        assert!(validate_volume("C:").is_err());
        assert!(validate_volume("C:/").is_err());
        assert!(validate_volume("CC:\\").is_err());
        assert!(validate_volume("\\\\server").is_err());
        assert!(validate_volume("").is_err());
    }

    #[test]
    fn rejects_volume_injection() {
        assert!(validate_volume("C:\\'; DROP TABLE --").is_err());
    }

    #[tokio::test]
    async fn create_snapshot_returns_none_on_non_windows() {
        #[cfg(not(windows))]
        {
            let result = create_snapshot(Path::new("C:\\")).await.unwrap();
            assert!(result.is_none());
        }
    }
}
