//! "Start BorgUI at login" via the Windows registry `Run` key.
//!
//! Follows the same shell-out pattern as [`crate::scheduler`]: drive the
//! `reg` console tool, validate inputs, and unit-test the pure logic. The actual
//! registry round-trip only happens on Windows (`reg` is absent elsewhere) and
//! needs a real-Windows confirm; the query path is resilient so a status check
//! never errors on a platform without `reg`.
//!
//! The registered command starts the app with `--minimized`, so autostart drops
//! BorgUI into the tray instead of popping the window open at every login (the
//! flag is handled in the app's `lib.rs`).

use borg_core::error::{BorgError, Result};

/// Per-user "run at sign-in" key. Values under it launch for the current user
/// at login and need no elevation (unlike the machine-wide `HKLM` key).
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// The registry value name BorgUI registers under the `Run` key.
pub const AUTOSTART_VALUE: &str = "BorgUI";

fn validate_value_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(BorgError::InvalidConfig {
            message: "autostart value name cannot be empty".into(),
        });
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(BorgError::InvalidConfig {
            message: "autostart value name contains invalid characters".into(),
        });
    }
    Ok(())
}

fn validate_exe_path(path: &str) -> Result<()> {
    if path.trim().is_empty() {
        return Err(BorgError::InvalidConfig {
            message: "executable path cannot be empty".into(),
        });
    }
    // `reg` receives this as a direct argument (no shell), so the only characters
    // that matter are ones that would corrupt the stored REG_SZ value or the
    // quotes we wrap the path in. Real paths legitimately contain ':', '\\',
    // spaces, and '()' (e.g. "Program Files (x86)").
    if path.chars().any(|c| matches!(c, '"' | '\n' | '\r' | '\0')) {
        return Err(BorgError::InvalidConfig {
            message: "executable path contains invalid characters".into(),
        });
    }
    Ok(())
}

/// The command line stored under the `Run` key: the quoted executable plus the
/// flag that makes BorgUI start hidden in the tray. The path is quoted so login
/// parses it as one token even when it contains spaces.
fn autostart_command(exe_path: &str) -> String {
    format!("\"{exe_path}\" --minimized")
}

async fn run_reg(args: &[&str]) -> Result<std::process::Output> {
    Ok(tokio::process::Command::new("reg")
        .args(args)
        .output()
        .await?)
}

/// Register BorgUI to start at login.
pub async fn enable(value_name: &str, exe_path: &str) -> Result<()> {
    validate_value_name(value_name)?;
    validate_exe_path(exe_path)?;

    let data = autostart_command(exe_path);
    let output = run_reg(&[
        "add", RUN_KEY, "/V", value_name, "/T", "REG_SZ", "/D", &data, "/F",
    ])
    .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(BorgError::ProcessFailed {
            message: "reg add (enable autostart) failed".into(),
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        })
    }
}

/// Remove the autostart entry. Idempotent: an already-absent value is success.
pub async fn disable(value_name: &str) -> Result<()> {
    validate_value_name(value_name)?;

    let output = run_reg(&["delete", RUN_KEY, "/V", value_name, "/F"]).await?;
    if output.status.success() {
        return Ok(());
    }
    // `reg delete` returns non-zero when the value doesn't exist — which still
    // means "not registered". Re-query rather than parse the (localized) error
    // text, so toggling off twice doesn't surface a spurious failure.
    if !is_enabled(value_name).await {
        return Ok(());
    }
    Err(BorgError::ProcessFailed {
        message: "reg delete (disable autostart) failed".into(),
        exit_code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into(),
    })
}

/// Whether the `Run`-key value currently exists. Resilient by design: an invalid
/// name, a spawn/IO failure (e.g. `reg` absent off Windows), or a non-zero query
/// all report `false` so a status check never errors the caller.
pub async fn is_enabled(value_name: &str) -> bool {
    if validate_value_name(value_name).is_err() {
        return false;
    }
    match run_reg(&["query", RUN_KEY, "/V", value_name]).await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_default_value_name() {
        assert!(validate_value_name(AUTOSTART_VALUE).is_ok());
        assert!(validate_value_name("BorgUI-Daily_2").is_ok());
    }

    #[test]
    fn rejects_empty_value_name() {
        assert!(validate_value_name("").is_err());
    }

    #[test]
    fn rejects_value_name_with_space_or_injection() {
        assert!(validate_value_name("bad name").is_err());
        assert!(validate_value_name("evil&del").is_err());
        assert!(validate_value_name("a\\b").is_err());
    }

    #[test]
    fn accepts_realistic_windows_exe_paths() {
        assert!(validate_exe_path(r"C:\Program Files\BorgUI\BorgUI.exe").is_ok());
        // Parens and spaces ("Program Files (x86)") must be allowed.
        assert!(validate_exe_path(r"C:\Program Files (x86)\BorgUI\BorgUI.exe").is_ok());
    }

    #[test]
    fn rejects_empty_or_corrupting_exe_path() {
        assert!(validate_exe_path("").is_err());
        assert!(validate_exe_path("   ").is_err());
        assert!(validate_exe_path("C:\\has\"quote.exe").is_err());
        assert!(validate_exe_path("C:\\has\nnewline.exe").is_err());
    }

    #[test]
    fn autostart_command_quotes_path_and_adds_flag() {
        assert_eq!(
            autostart_command(r"C:\Program Files\BorgUI\BorgUI.exe"),
            r#""C:\Program Files\BorgUI\BorgUI.exe" --minimized"#
        );
    }

    #[tokio::test]
    async fn is_enabled_is_false_for_invalid_name() {
        // Validation guard short-circuits before any `reg` call, so this is
        // deterministic on every platform.
        assert!(!is_enabled("bad name!").await);
    }

    #[tokio::test]
    async fn enable_rejects_bad_exe_path_before_running_reg() {
        assert!(enable(AUTOSTART_VALUE, "C:\\bad\"quote.exe").await.is_err());
    }

    #[tokio::test]
    async fn disable_rejects_bad_value_name_before_running_reg() {
        assert!(disable("bad name").await.is_err());
    }
}
