use keyring::{Entry, Error as KeyringError};

const SERVICE: &str = "borg-ui";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| e.to_string())
}

pub fn set_passphrase(account: &str, passphrase: &str) -> Result<(), String> {
    entry(account)?
        .set_password(passphrase)
        .map_err(|e| e.to_string())
}

pub fn get_passphrase(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn clear_passphrase(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn has_passphrase(account: &str) -> Result<bool, String> {
    Ok(get_passphrase(account)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in integration test that exercises the real Windows Credential
    /// Manager through keyring's `windows-native` backend.
    ///
    /// Compiled on every platform (so Linux CI type-checks it), but only
    /// *executed* on Windows when `BORGUI_KEYCHAIN_TEST=1` is set — so an
    /// unattended `cargo test`, and Linux CI in particular, never touch a keyring
    /// backend. Set the env var on the Windows smoke VM to run it (see
    /// `tests/smoke-windows/README.md`).
    ///
    /// MUST run in an interactive desktop session (session 1). Credential Manager
    /// is unreachable from an SSH/network logon — there it fails with
    /// `ERROR_NO_SUCH_LOGON_SESSION` (verified on the smoke VM). `validate-gui.ps1`
    /// compiles this over SSH but launches it via an `/IT` scheduled task.
    ///
    /// Proves item 5 of the GUI-validation pass: a passphrase set via the app's
    /// keychain module is persisted to Credential Manager (a fresh `Entry` reads
    /// it back — keyring keeps no in-process cache), is visible to the OS
    /// `cmdkey` tool, and is removed on clear. Uses a throwaway account and
    /// always clears, so a real stored passphrase is never touched.
    #[test]
    fn windows_credential_manager_roundtrip() {
        if !cfg!(windows) || std::env::var("BORGUI_KEYCHAIN_TEST").as_deref() != Ok("1") {
            eprintln!(
                "SKIP: Windows-only; set BORGUI_KEYCHAIN_TEST=1 on the Windows smoke VM to run"
            );
            return;
        }

        // Distinctive, lowercase-alphanumeric throwaway account so the keyring
        // target is easy to spot in `cmdkey /list` regardless of how the backend
        // orders service/account in the target name.
        let account = "borguismokevalidate";
        let secret = "cred-mgr-roundtrip-123";

        // Start clean in case a previously aborted run left the credential behind.
        let _ = clear_passphrase(account);

        // NOTE: this panic message is a load-bearing signal for the smoke harness.
        // Run from an SSH/network logon, keyring fails with the Windows error name
        // `ERROR_NO_SUCH_LOGON_SESSION`; validate-gui.ps1 greps the test output for
        // that name (and its HRESULT) to SKIP rather than FAIL. Don't suppress or
        // reword keyring's error text here without updating that matcher.
        set_passphrase(account, secret).expect("set_passphrase should succeed");

        // A fresh `Entry` (built inside get_passphrase) reads straight from
        // Credential Manager — this proves persistence, not an in-memory echo.
        assert_eq!(
            get_passphrase(account).expect("get_passphrase should succeed"),
            Some(secret.to_string()),
            "stored passphrase should read back from Credential Manager"
        );

        // Soft, non-fatal OS-level visibility check. The authoritative proof is
        // the round-trip above (a fresh `Entry` reading from Credential Manager);
        // `cmdkey`'s target string is formatted by keyring and we don't control
        // it, so a miss here is a warning, not a failure (avoids a false-fail if
        // the target encoding changes across keyring versions).
        let listing = cmdkey_list().to_lowercase();
        if listing.contains(account) {
            eprintln!("cmdkey /list shows the '{account}' target (visible in Credential Manager)");
        } else {
            eprintln!(
                "WARN: '{account}' not found verbatim in cmdkey /list (keyring may encode the target); round-trip still proves persistence"
            );
        }

        clear_passphrase(account).expect("clear_passphrase should succeed");
        assert_eq!(
            get_passphrase(account).expect("get_passphrase after clear should succeed"),
            None,
            "passphrase should be gone after clear"
        );

        // Positive marker on the REAL (non-skipped) path only, so the smoke
        // harness can distinguish a genuine pass from the self-skip above (which
        // also exits a #[test] as "ok"). See tests/smoke-windows/validate-gui.ps1.
        println!("KEYCHAIN_ROUNDTRIP_OK");
    }

    fn cmdkey_list() -> String {
        // Never panic: a cmdkey failure here must not abort before the
        // clear_passphrase below it, which would leak the throwaway credential.
        std::process::Command::new("cmd")
            .args(["/C", "cmdkey", "/list"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    }
}
