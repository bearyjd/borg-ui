//! Process-spawning helper shared by the borg and ssh wrappers.

use std::ffi::OsStr;
use tokio::process::Command;

/// Build a [`tokio::process::Command`] that never flashes a console window on
/// Windows.
///
/// `borg.exe`, `ssh.exe` and `ssh-keygen.exe` are console-subsystem programs,
/// so spawning them from the windowed Tauri GUI would briefly pop up a black
/// console. The `CREATE_NO_WINDOW` process-creation flag suppresses that. tokio's
/// `Command` doesn't expose creation flags, so we build a `std::process::Command`
/// first, set the flag under `#[cfg(windows)]`, then convert. A no-op on every
/// other platform.
pub(crate) fn command<S: AsRef<OsStr>>(program: S) -> Command {
    let std_cmd = std::process::Command::new(program);

    #[cfg(windows)]
    let std_cmd = {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW — see Microsoft "Process Creation Flags".
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut std_cmd = std_cmd;
        std_cmd.creation_flags(CREATE_NO_WINDOW);
        std_cmd
    };

    Command::from(std_cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_targets_the_given_program() {
        let cmd = command("borg");
        assert_eq!(cmd.as_std().get_program(), "borg");
    }

    #[test]
    fn command_starts_with_no_args() {
        let cmd = command("ssh");
        assert_eq!(cmd.as_std().get_args().count(), 0);
    }
}
