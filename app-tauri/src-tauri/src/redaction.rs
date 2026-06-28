use std::sync::OnceLock;

use regex::Regex;

const REDACTED: &str = "[REDACTED]";
const SENSITIVE_ENV_NAMES: &[&str] = &[
    "BORG_PASSPHRASE",
    "BORG_PASSCOMMAND",
    "BORG_RSH",
    "SSH_AUTH_SOCK",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AZURE_STORAGE_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
];

pub fn redact(input: &str) -> String {
    let mut output = input.to_string();
    for regex in patterns() {
        output = regex
            .replace_all(&output, |caps: &regex::Captures<'_>| {
                format!("{}={REDACTED}", &caps[1])
            })
            .into_owned();
    }
    output = private_key_pattern()
        .replace_all(&output, REDACTED)
        .into_owned();
    output = url_credentials_pattern()
        .replace_all(&output, "${scheme}${user}:[REDACTED]@")
        .into_owned();
    for name in SENSITIVE_ENV_NAMES {
        if let Ok(value) = std::env::var(name)
            && !value.is_empty()
        {
            output = output.replace(&value, REDACTED);
        }
    }
    output
}

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Regex::new(
                r#"(?i)\b(passphrase|password|token|secret|private_key|BORG_PASSPHRASE|BORG_PASSCOMMAND)\s*[:=]\s*(?:\S+|"[^"]*")"#,
            )
            .expect("valid secret pattern"),
            Regex::new(
                r#"(?i)\b(AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY|AZURE_STORAGE_KEY)\s*[:=]\s*(?:\S+|"[^"]*")"#,
            )
            .expect("valid environment pattern"),
        ]
    })
}

fn private_key_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?s)-----BEGIN [^-]*PRIVATE KEY-----.*?-----END [^-]*PRIVATE KEY-----")
            .expect("valid private key pattern")
    })
}

fn url_credentials_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?P<scheme>[a-zA-Z][a-zA-Z0-9+.-]*://)(?P<user>[^/@:\s]+):[^@\s]+@")
            .expect("valid URL credential pattern")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_secret_forms() {
        let text = "passphrase=hunter2 password: nope BORG_PASSPHRASE=secret";
        let redacted = redact(text);
        assert!(!redacted.contains("hunter2"));
        assert!(!redacted.contains("nope"));
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn redacts_url_passwords_and_private_keys() {
        let text = "https://alice:p4ss@example.test\n-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
        let redacted = redact(text);
        assert!(redacted.contains("https://alice:[REDACTED]@example.test"));
        assert!(!redacted.contains("abc"));
    }
}
