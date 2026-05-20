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

/// Remap an original path through a VSS shadow copy device path.
///
/// Example: `C:\Users\me\docs` with volume `C:\` and shadow device
/// `\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy3` becomes
/// `\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy3\Users\me\docs`.
pub fn remap_path(original: &Path, volume_root: &str, shadow_device: &str) -> Result<PathBuf> {
    let original_str = original.to_string_lossy();
    if original_str.len() < volume_root.len()
        || !original_str[..volume_root.len()].eq_ignore_ascii_case(volume_root)
    {
        return Err(BorgError::InvalidConfig {
            message: format!("path {} is not on volume {}", original_str, volume_root),
        });
    }
    let relative = &original_str[volume_root.len()..];
    let remapped = if relative.is_empty() {
        shadow_device.to_string()
    } else {
        format!("{}\\{}", shadow_device.trim_end_matches('\\'), relative)
    };
    Ok(PathBuf::from(remapped))
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

/// Snapshot all volumes referenced by `paths`, returning remapped paths and handles.
///
/// If VSS is unavailable or any snapshot fails, falls back to original paths
/// with a warning. The caller should always call `release_all` when done.
pub async fn snapshot_sources(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<SnapshotHandle>) {
    let volumes = match unique_volumes(paths) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Could not extract volumes, skipping VSS: {}", e);
            return (paths.to_vec(), vec![]);
        }
    };

    let mut handles: Vec<SnapshotHandle> = Vec::new();

    for vol in &volumes {
        match create_snapshot(Path::new(vol)).await {
            Ok(Some(handle)) => handles.push(handle),
            Ok(None) => return (paths.to_vec(), vec![]),
            Err(e) => {
                tracing::warn!("VSS snapshot failed for {}: {}", vol, e);
                release_all(handles).await;
                return (paths.to_vec(), vec![]);
            }
        }
    }

    if handles.is_empty() {
        return (paths.to_vec(), vec![]);
    }

    let mut remapped = Vec::with_capacity(paths.len());
    for path in paths {
        let vol = match extract_volume(path) {
            Ok(v) => v,
            Err(_) => {
                release_all(handles).await;
                return (paths.to_vec(), vec![]);
            }
        };
        let handle = match handles.iter().find(|h| h.volume == vol) {
            Some(h) => h,
            None => {
                release_all(handles).await;
                return (paths.to_vec(), vec![]);
            }
        };
        match remap_path(path, &vol, &handle.shadow_path.to_string_lossy()) {
            Ok(p) => remapped.push(p),
            Err(e) => {
                tracing::warn!("Path remap failed: {}", e);
                release_all(handles).await;
                return (paths.to_vec(), vec![]);
            }
        }
    }

    (remapped, handles)
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
    fn remaps_path_through_shadow() {
        let result = remap_path(
            Path::new("C:\\Users\\me\\docs"),
            "C:\\",
            "\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy3",
        )
        .unwrap();
        assert_eq!(
            result,
            PathBuf::from("\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy3\\Users\\me\\docs")
        );
    }

    #[test]
    fn remaps_volume_root_itself() {
        let result = remap_path(
            Path::new("C:\\"),
            "C:\\",
            "\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy1",
        )
        .unwrap();
        assert_eq!(
            result,
            PathBuf::from("\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy1")
        );
    }

    #[test]
    fn remap_rejects_wrong_volume() {
        assert!(
            remap_path(
                Path::new("D:\\data"),
                "C:\\",
                "\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy1",
            )
            .is_err()
        );
    }

    #[test]
    fn remap_case_insensitive_volume() {
        let result = remap_path(
            Path::new("c:\\users\\me"),
            "C:\\",
            "\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy1",
        );
        assert!(result.is_ok());
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
