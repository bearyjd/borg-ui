use std::path::Path;
use borg_core::error::Result;

/// Creates a VSS snapshot of the given volume.
///
/// v0.1: not implemented — backups run against live files.
/// v0.2: will shell out to vshadow.exe or diskshadow.exe
///       to create a shadow copy, returning the snapshot path.
///
/// Requires administrator privileges.
pub async fn create_snapshot(_volume: &Path) -> Result<Option<SnapshotHandle>> {
    tracing::warn!("VSS not yet implemented — backing up live files");
    Ok(None)
}

pub async fn release_snapshot(_handle: SnapshotHandle) -> Result<()> {
    Ok(())
}

pub struct SnapshotHandle {
    pub shadow_path: std::path::PathBuf,
    pub snapshot_id: String,
}
