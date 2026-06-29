use std::io::{Read, Write};
use std::path::Path;

use age::armor::{ArmoredReader, ArmoredWriter, Format};
use age::secrecy::SecretString;
use age::{Decryptor, Encryptor};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

const FORMAT_NAME: &str = "borgui-recovery-key";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryEnvelope {
    pub format: String,
    pub version: u32,
    pub created_at: String,
    pub repository_id: String,
    pub encryption: String,
    pub payload: String,
}

pub fn encrypt(
    mut borg_key: Vec<u8>,
    repository_id: String,
    passphrase: String,
) -> Result<RecoveryEnvelope, String> {
    if passphrase.is_empty() {
        borg_key.zeroize();
        return Err("recovery passphrase cannot be empty".into());
    }
    let encryptor = Encryptor::with_user_passphrase(SecretString::from(passphrase));
    let mut armored = Vec::new();
    {
        let armor = ArmoredWriter::wrap_output(&mut armored, Format::AsciiArmor)
            .map_err(|error| error.to_string())?;
        let mut writer = encryptor
            .wrap_output(armor)
            .map_err(|error| error.to_string())?;
        writer
            .write_all(&borg_key)
            .map_err(|error| error.to_string())?;
        let armor = writer.finish().map_err(|error| error.to_string())?;
        armor.finish().map_err(|error| error.to_string())?;
    }
    borg_key.zeroize();

    String::from_utf8(armored)
        .map(|payload| RecoveryEnvelope {
            format: FORMAT_NAME.into(),
            version: FORMAT_VERSION,
            created_at: chrono::Utc::now().to_rfc3339(),
            repository_id,
            encryption: "age-scrypt".into(),
            payload,
        })
        .map_err(|error| error.to_string())
}

pub fn decrypt(envelope: &RecoveryEnvelope, passphrase: String) -> Result<Vec<u8>, String> {
    validate(envelope)?;
    let reader = ArmoredReader::new(envelope.payload.as_bytes());
    let decryptor =
        Decryptor::new(reader).map_err(|error| format!("invalid recovery payload: {error}"))?;
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase));
    let mut plaintext = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| "incorrect recovery passphrase or corrupt payload".to_string())?;
    let mut key = Vec::new();
    plaintext
        .read_to_end(&mut key)
        .map_err(|_| "incorrect recovery passphrase or corrupt payload".to_string())?;
    if !key.starts_with(b"BORG_KEY ") {
        key.zeroize();
        return Err("decrypted payload is not a Borg key export".into());
    }
    Ok(key)
}

pub fn parse(bytes: &[u8]) -> Result<RecoveryEnvelope, String> {
    let envelope: RecoveryEnvelope =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid recovery file: {error}"))?;
    validate(&envelope)?;
    Ok(envelope)
}

fn validate(envelope: &RecoveryEnvelope) -> Result<(), String> {
    if envelope.format != FORMAT_NAME {
        return Err("not a BorgUI recovery-key file".into());
    }
    if envelope.version > FORMAT_VERSION {
        return Err(format!(
            "recovery format version {} is newer than supported version {}",
            envelope.version, FORMAT_VERSION
        ));
    }
    if envelope.version == 0 || envelope.encryption != "age-scrypt" {
        return Err("unsupported recovery-key format".into());
    }
    if envelope.repository_id.trim().is_empty() {
        return Err("recovery file is missing its repository identifier".into());
    }
    Ok(())
}

pub fn restrictive_temp(config_dir: &Path) -> Result<tempfile::NamedTempFile, String> {
    std::fs::create_dir_all(config_dir).map_err(|error| error.to_string())?;
    tempfile::Builder::new()
        .prefix(".borgui-recovery-")
        .tempfile_in(config_dir)
        .map_err(|error| error.to_string())
}

pub fn secure_remove(mut file: tempfile::NamedTempFile) -> Result<(), String> {
    let length = file
        .as_file()
        .metadata()
        .map_err(|error| error.to_string())?
        .len();
    file.as_file_mut()
        .set_len(0)
        .map_err(|error| error.to_string())?;
    if length > 0 {
        file.as_file_mut()
            .write_all(&vec![0_u8; length as usize])
            .map_err(|error| error.to_string())?;
        file.as_file_mut()
            .sync_all()
            .map_err(|error| error.to_string())?;
    }
    file.close().map_err(|error| error.to_string())
}

pub fn write_exclusive(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "destination has no parent directory".to_string())?;
    let mut file = tempfile::Builder::new()
        .prefix(".borgui-encrypted-recovery-")
        .tempfile_in(parent)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    file.persist_noclobber(destination).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            "destination already exists; choose a new file name".to_string()
        } else {
            error.error.to_string()
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key() -> Vec<u8> {
        b"BORG_KEY 0123456789abcdef\nZmFrZS1rZXk=\n".to_vec()
    }

    #[test]
    fn encryption_round_trip_and_wrong_passphrase() {
        let envelope = encrypt(sample_key(), "repo-id".into(), "correct".into()).unwrap();
        assert!(!envelope.payload.contains("ZmFrZS1rZXk"));
        assert_eq!(decrypt(&envelope, "correct".into()).unwrap(), sample_key());
        assert!(decrypt(&envelope, "wrong".into()).is_err());
    }

    #[test]
    fn rejects_corrupt_and_future_formats() {
        assert!(parse(b"not json").is_err());
        let mut envelope = encrypt(sample_key(), "repo-id".into(), "correct".into()).unwrap();
        envelope.version = FORMAT_VERSION + 1;
        assert!(
            decrypt(&envelope, "correct".into())
                .unwrap_err()
                .contains("newer")
        );
    }

    #[test]
    fn restrictive_temp_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = restrictive_temp(dir.path()).unwrap();
        file.write_all(&sample_key()).unwrap();
        let path = file.path().to_path_buf();
        secure_remove(file).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn exclusive_write_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovery.json");
        write_exclusive(&path, b"first").unwrap();
        assert!(write_exclusive(&path, b"second").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"first");
    }
}
