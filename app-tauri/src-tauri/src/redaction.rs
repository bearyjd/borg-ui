use std::sync::OnceLock;

use regex::Regex;

const REDACTED: &str = "[REDACTED]";
const SENSITIVE_ENV_NAMES: &[&str] = &[
    "BORG_PASSPHRASE",
    // `\b` cannot match between `_` and `PASSPHRASE`, so the generic pattern
    // below never covered this one — it needs naming explicitly.
    "BORG_NEW_PASSPHRASE",
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
    for (pattern, prefix) in user_path_patterns() {
        output = pattern
            .replace_all(&output, format!("{prefix}{REDACTED}"))
            .into_owned();
    }
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
                r#"(?i)\b(passphrase|password|token|secret|private_key|BORG_(?:NEW_)?PASSPHRASE|BORG_PASSCOMMAND)\s*[:=]\s*(?:\S+|"[^"]*")"#,
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

/// Account names inside home-directory paths.
///
/// borg's warnings name the file they are about (`C:\Users\alice\Documents\
/// tax.pdf: Permission denied`), and those warnings now reach the log file and
/// therefore the support bundle. Scrubbing the *whole* path would destroy the
/// diagnostic value that makes the log worth exporting at all, but the account
/// name is the part that identifies a person rather than a problem, so it goes.
///
/// This is a reduction, not a guarantee: a bundle can still contain file and
/// folder names from backed-up sources. `export_diagnostics` says so explicitly
/// rather than implying otherwise.
fn user_path_patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                // C:\Users\alice, D:/Users/alice — stop at the next separator.
                //
                // `\s` is excluded from the account segment on purpose. Without
                // it the match is greedy across spaces, so a log line like
                // "Backing up C:\Users\alice to the repository" collapsed to
                // "Backing up C:\Users\[REDACTED]" — destroying the rest of the
                // message, which is the exact diagnostic value this approach
                // exists to preserve. The cost is that an account name
                // containing a space is only partly scrubbed.
                Regex::new(r#"(?i)(?P<prefix>[a-z]:[\\/]Users[\\/])(?P<user>[^\\/:*?"<>|\s]+)"#)
                    .expect("valid windows home pattern"),
                "$prefix",
            ),
            (
                // \\nas\share\Users\bob, \\localhost\C$\Users\alice. Backing up
                // from a NAS or mapped drive is ordinary on Windows, and the
                // drive-letter pattern above cannot see those paths at all
                // (`C$` has no colon).
                Regex::new(
                    r#"(?i)(?P<prefix>\\\\[^\\/:*?"<>|\s]+\\[^\\/:*?"<>|\s]+\\Users\\)(?P<user>[^\\/:*?"<>|\s]+)"#,
                )
                .expect("valid UNC home pattern"),
                "$prefix",
            ),
            (
                // /home/alice, /Users/alice (macOS), /root stays as-is.
                //
                // Anchored to a path start (line start, whitespace, or a quote)
                // so a URL path segment like https://docs.example/home/setup is
                // left alone — over-redacting eats the diagnostic value this
                // whole approach is trying to preserve.
                Regex::new(r#"(?m)(?P<prefix>(?:^|[\s"'=(\[])/(?:home|Users)/)(?P<user>[^/\s:]+)"#)
                    .expect("valid unix home pattern"),
                "$prefix",
            ),
        ]
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

    /// `BORG_NEW_PASSPHRASE` carries the rotation's *new* secret. `\b` cannot
    /// match between `_` and `PASSPHRASE`, so the generic `\bpassphrase` pattern
    /// never covered it — it needs its own alternation and env-name entry.
    #[test]
    fn redacts_the_rotation_new_passphrase_variable() {
        let redacted = redact("BORG_NEW_PASSPHRASE=rotated-secret");
        assert!(!redacted.contains("rotated-secret"), "{redacted}");
        assert!(redacted.contains(REDACTED), "{redacted}");
    }

    /// borg warnings name the file they are about, and those reach the log file
    /// and the support bundle. The account name identifies a person rather than
    /// a problem, so it is scrubbed — while the rest of the path stays, because
    /// a log with no paths is not worth exporting.
    #[test]
    fn redacts_account_names_from_home_paths() {
        let cases = [
            (
                r"C:\Users\alice\Documents\tax.pdf: Permission denied",
                r"C:\Users\[REDACTED]\Documents\tax.pdf",
            ),
            (
                "/home/bob/photos/img.jpg: Permission denied",
                "/home/[REDACTED]/photos/img.jpg",
            ),
            ("/Users/carol/Desktop/x", "/Users/[REDACTED]/Desktop/x"),
        ];
        for (input, expected) in cases {
            let redacted = redact(input);
            assert!(redacted.contains(expected), "{redacted}");
        }
        let redacted = redact(r"C:\Users\alice\Documents\tax.pdf");
        assert!(!redacted.contains("alice"), "{redacted}");
        // The diagnostic part survives — that is the whole point of the log.
        assert!(redacted.contains("tax.pdf"), "{redacted}");
    }

    /// A greedy account segment used to swallow everything after the username
    /// when a space followed it, so "Backing up C:\Users\alice to the
    /// repository" became "Backing up C:\Users\[REDACTED]" — deleting the very
    /// message the log exists to carry. Every earlier test happened to put a
    /// separator straight after the name, so none of them saw it.
    #[test]
    fn account_redaction_stops_at_the_username_and_keeps_the_rest_of_the_line() {
        let cases = [
            (
                r"Backing up C:\Users\alice to the repository now",
                r"Backing up C:\Users\[REDACTED] to the repository now",
            ),
            (
                r"path=C:\Users\alice error=disk full",
                r"path=C:\Users\[REDACTED] error=disk full",
            ),
            (
                "/home/bob failed: disk full",
                "/home/[REDACTED] failed: disk full",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(redact(input), expected);
        }
    }

    /// Only the account segment goes. A path that merely contains "users"
    /// elsewhere, or a drive root, must survive intact.
    #[test]
    fn account_redaction_leaves_unrelated_paths_alone() {
        for untouched in [
            r"D:\Photos\2026\img.jpg",
            "/var/backups/repo",
            r"C:\Program Files\BorgUI\borg.exe",
            // A URL path segment is not a home directory. Over-redacting eats
            // the diagnostic value the log exists for.
            "see https://docs.example.test/home/setup-guide",
        ] {
            assert_eq!(redact(untouched), untouched);
        }
    }

    /// Backing up from a NAS or mapped network drive is ordinary on Windows,
    /// and the drive-letter pattern cannot see those paths at all.
    #[test]
    fn redacts_account_names_from_unc_paths() {
        let cases = [
            (
                r"\\nas\share\Users\bob\file.txt: Permission denied",
                r"\\nas\share\Users\[REDACTED]\file.txt",
            ),
            (
                r"\\localhost\C$\Users\alice\Backups",
                r"\\localhost\C$\Users\[REDACTED]\Backups",
            ),
        ];
        for (input, expected) in cases {
            let redacted = redact(input);
            assert!(redacted.contains(expected), "{redacted}");
        }
        assert!(!redact(r"\\nas\share\Users\bob\f.txt").contains("bob"));
    }

    #[test]
    fn redacts_url_passwords_and_private_keys() {
        let text = "https://alice:p4ss@example.test\n-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
        let redacted = redact(text);
        assert!(redacted.contains("https://alice:[REDACTED]@example.test"));
        assert!(!redacted.contains("abc"));
    }
}
