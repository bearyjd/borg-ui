use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AuthorizedKeysInstructions {
    pub authorized_keys_line: String,
    pub maintenance_notes: Vec<&'static str>,
}

pub fn generate_authorized_keys_line(
    public_key: &str,
    repository_path: &str,
) -> Result<AuthorizedKeysInstructions, String> {
    let mut key_parts = public_key.split_whitespace();
    let key_type = key_parts
        .next()
        .ok_or_else(|| "public key is empty".to_string())?;
    let key_data = key_parts
        .next()
        .ok_or_else(|| "public key data is missing".to_string())?;
    if !key_type.starts_with("ssh-") || key_data.is_empty() {
        return Err("unsupported SSH public key".into());
    }
    if repository_path.trim().is_empty() || repository_path.contains(['\n', '\r', '\0']) {
        return Err("repository path is invalid".into());
    }
    let restricted_path = shell_quote(repository_path);
    let forced_command =
        format!("borg serve --restrict-to-repository {restricted_path} --append-only");
    let option_command = forced_command.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(AuthorizedKeysInstructions {
        authorized_keys_line: format!(
            "restrict,command=\"{option_command}\" {key_type} {key_data}"
        ),
        maintenance_notes: vec![
            "Install this line on the Borg server for the backup-only account.",
            "Append-only prevents the backup PC from permanently destroying repository data.",
            "Prune and delete remain logical operations until trusted server-side maintenance compacts them.",
            "Keep unrestricted maintenance credentials off this backup PC.",
            "Document server-side transaction-log recovery and maintenance procedures.",
        ],
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_strip_key_comment_and_quote_repository() {
        let instructions = generate_authorized_keys_line(
            "ssh-ed25519 AAAAC3Nza comment@private-pc",
            "/srv/Alice's backups",
        )
        .unwrap();
        assert!(!instructions.authorized_keys_line.contains("private-pc"));
        assert!(
            instructions
                .authorized_keys_line
                .starts_with("restrict,command=")
        );
        assert!(instructions.authorized_keys_line.contains("--append-only"));
        assert!(
            instructions
                .authorized_keys_line
                .contains(r#"/srv/Alice'\"'\"'s backups"#)
        );
    }

    #[test]
    fn rejects_malformed_keys_and_paths() {
        assert!(generate_authorized_keys_line("not-a-key", "/repo").is_err());
        assert!(generate_authorized_keys_line("ssh-ed25519 AAA", "/repo\ncommand=evil").is_err());
    }
}
