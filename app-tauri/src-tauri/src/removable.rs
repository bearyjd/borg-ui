use std::path::Path;

#[derive(Debug, Default)]
pub struct TriggerState {
    was_present: bool,
}

impl TriggerState {
    pub fn update(&mut self, present: bool) -> bool {
        let fire = present && !self.was_present;
        self.was_present = present;
        fire
    }
}

pub fn removable_destination_present(path: &Path) -> bool {
    removable_destination_present_impl(path)
}

#[cfg(windows)]
fn removable_destination_present_impl(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{DRIVE_REMOVABLE, GetDriveTypeW};

    let Some(prefix) = path.components().next() else {
        return false;
    };
    let root = format!("{}\\", prefix.as_os_str().to_string_lossy());
    let root: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: root is a NUL-terminated UTF-16 buffer.
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOVABLE }
    &&path.exists()
}

#[cfg(not(windows))]
fn removable_destination_present_impl(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_fires_once_until_destination_disconnects() {
        let mut state = TriggerState::default();
        assert!(state.update(true));
        assert!(!state.update(true));
        assert!(!state.update(false));
        assert!(state.update(true));
    }
}
