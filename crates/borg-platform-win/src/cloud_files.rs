use std::path::{Path, PathBuf};

use borg_core::borg::CancelToken;

pub const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
pub const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
pub const FILE_ATTRIBUTE_UNPINNED: u32 = 0x0010_0000;

#[derive(Debug, Clone)]
pub struct PlaceholderFile {
    pub path: PathBuf,
    pub size: u64,
}

pub fn is_placeholder_attributes(attributes: u32) -> bool {
    attributes
        & (FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
            | FILE_ATTRIBUTE_UNPINNED)
        != 0
}

pub fn find_placeholders(
    roots: &[PathBuf],
    cancel: &CancelToken,
) -> Result<Vec<PlaceholderFile>, String> {
    let mut found = Vec::new();
    for root in roots {
        scan(root, cancel, &mut found)?;
        if cancel.is_cancelled() {
            return Err("operation cancelled".into());
        }
    }
    Ok(found)
}

fn scan(path: &Path, cancel: &CancelToken, found: &mut Vec<PlaceholderFile>) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Ok(());
    }
    let entries = std::fs::read_dir(path).map_err(|_| "could not scan a backup source")?;
    for entry in entries {
        if cancel.is_cancelled() {
            break;
        }
        let entry = entry.map_err(|_| "could not scan a backup source")?;
        let file_type = entry
            .file_type()
            .map_err(|_| "could not inspect a backup source")?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            scan(&entry.path(), cancel, found)?;
        } else if file_type.is_file() {
            let metadata = entry
                .metadata()
                .map_err(|_| "could not inspect a backup source")?;
            if is_placeholder(&metadata) {
                found.push(PlaceholderFile {
                    path: entry.path(),
                    size: metadata.len(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_placeholder(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    is_placeholder_attributes(metadata.file_attributes())
}

#[cfg(not(windows))]
fn is_placeholder(_metadata: &std::fs::Metadata) -> bool {
    false
}

pub fn hydrate(file: &PlaceholderFile, cancel: &CancelToken) -> Result<(), String> {
    use std::io::Read;
    let mut input =
        std::fs::File::open(&file.path).map_err(|_| "cloud placeholder hydration failed")?;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err("operation cancelled".into());
        }
        let read = input
            .read(&mut buffer)
            .map_err(|_| "cloud placeholder hydration failed")?;
        if read == 0 {
            return Ok(());
        }
    }
}

#[cfg(windows)]
pub fn free_space(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0_u64;
    // SAFETY: path is NUL-terminated and available is writable.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(available)
    }
}

#[cfg(not(windows))]
pub fn free_space(_path: &Path) -> Result<u64, String> {
    Ok(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_on_demand_attribute_variants() {
        assert!(is_placeholder_attributes(FILE_ATTRIBUTE_RECALL_ON_OPEN));
        assert!(is_placeholder_attributes(
            FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
        ));
        assert!(is_placeholder_attributes(FILE_ATTRIBUTE_UNPINNED));
        assert!(!is_placeholder_attributes(0x20));
    }

    #[test]
    fn hydration_honors_pre_cancelled_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("placeholder.bin");
        std::fs::write(&path, [1_u8; 16]).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = hydrate(&PlaceholderFile { path, size: 16 }, &cancel);
        assert_eq!(result.unwrap_err(), "operation cancelled");
    }
}
