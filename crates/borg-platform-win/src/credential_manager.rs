//! Windows Credential Manager smoke coverage.
//!
//! This intentionally lives outside the Tauri crate: a direct test binary must
//! not link WebView2 merely to verify the native Credential Manager backend.

#[cfg(test)]
mod tests {
    use keyring::{Entry, Error as KeyringError};

    const SERVICE: &str = "borg-ui";
    const ACCOUNT: &str = "borguismokevalidate";
    const SECRET: &str = "cred-mgr-roundtrip-123";

    fn entry() -> Entry {
        Entry::new(SERVICE, ACCOUNT).expect("Credential Manager entry should be constructible")
    }

    /// Opt-in, interactive-session round-trip through the real Windows
    /// Credential Manager. The Windows smoke harness runs this in session 1.
    #[test]
    fn windows_credential_manager_roundtrip() {
        if std::env::var("BORGUI_KEYCHAIN_TEST").as_deref() != Ok("1") {
            eprintln!("SKIP: set BORGUI_KEYCHAIN_TEST=1 on the Windows smoke VM to run");
            return;
        }

        // Do not touch a user credential: this distinctive account is owned by
        // the smoke test and is cleaned up on both normal completion and before
        // a retry after an interrupted prior run.
        let _ = entry().delete_credential();
        entry()
            .set_password(SECRET)
            .expect("set_password should succeed");

        // A fresh Entry proves persistence in Credential Manager rather than an
        // in-process cache.
        assert_eq!(
            entry().get_password().expect("get_password should succeed"),
            SECRET
        );

        let listing = std::process::Command::new("cmd")
            .args(["/C", "cmdkey", "/list"])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).to_lowercase())
            .unwrap_or_default();
        if !listing.contains(ACCOUNT) {
            eprintln!(
                "WARN: cmdkey did not show the throwaway target; round-trip is authoritative"
            );
        }

        match entry().delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => {}
            Err(error) => panic!("delete_credential should succeed: {error}"),
        }
        assert!(matches!(entry().get_password(), Err(KeyringError::NoEntry)));
        println!("KEYCHAIN_ROUNDTRIP_OK");
    }
}
