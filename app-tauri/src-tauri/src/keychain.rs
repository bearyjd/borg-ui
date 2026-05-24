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
