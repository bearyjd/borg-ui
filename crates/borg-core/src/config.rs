use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{BorgError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub repo_path: String,
    pub ssh_key_path: Option<PathBuf>,
}

const SSH_FORBIDDEN: &[char] = &[
    '@', ':', ' ', '\'', '"', ';', '&', '|', '`', '$', '\n', '\r',
];
const PATH_FORBIDDEN: &[char] = &[';', '&', '|', '`', '$', '\'', '"', '\n', '\r', '\0'];

impl RepoConfig {
    pub fn validate(&self) -> Result<()> {
        if self.ssh_host.trim().is_empty() {
            return Err(BorgError::InvalidConfig {
                message: "ssh_host cannot be empty".into(),
            });
        }
        if self.ssh_user.trim().is_empty() {
            return Err(BorgError::InvalidConfig {
                message: "ssh_user cannot be empty".into(),
            });
        }
        if self.repo_path.trim().is_empty() {
            return Err(BorgError::InvalidConfig {
                message: "repo_path cannot be empty".into(),
            });
        }
        if self.ssh_port == 0 {
            return Err(BorgError::InvalidConfig {
                message: "ssh_port must be > 0".into(),
            });
        }
        if self.ssh_host.chars().any(|c| SSH_FORBIDDEN.contains(&c)) {
            return Err(BorgError::InvalidConfig {
                message: "ssh_host contains invalid characters".into(),
            });
        }
        if self.ssh_user.chars().any(|c| SSH_FORBIDDEN.contains(&c)) {
            return Err(BorgError::InvalidConfig {
                message: "ssh_user contains invalid characters".into(),
            });
        }
        if self.repo_path.chars().any(|c| PATH_FORBIDDEN.contains(&c)) {
            return Err(BorgError::InvalidConfig {
                message: "repo_path contains invalid characters".into(),
            });
        }
        Ok(())
    }

    pub fn ssh_url(&self) -> String {
        format!(
            "ssh://{}@{}:{}/{}",
            self.ssh_user, self.ssh_host, self.ssh_port, self.repo_path
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupProfile {
    pub name: String,
    pub source_paths: Vec<PathBuf>,
    pub excludes: Vec<String>,
    pub compression: Compression,
    pub repo: RepoConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Compression {
    None,
    Lz4,
    Zstd { level: u8 },
    Zlib { level: u8 },
}

impl Compression {
    pub fn validate(&self) -> Result<()> {
        match self {
            Compression::Zstd { level } if *level < 1 || *level > 22 => {
                Err(BorgError::InvalidConfig {
                    message: format!("zstd level must be 1-22, got {}", level),
                })
            }
            Compression::Zlib { level } if *level > 9 => Err(BorgError::InvalidConfig {
                message: format!("zlib level must be 0-9, got {}", level),
            }),
            _ => Ok(()),
        }
    }

    pub fn to_borg_arg(&self) -> String {
        match self {
            Compression::None => "none".to_string(),
            Compression::Lz4 => "lz4".to_string(),
            Compression::Zstd { level } => format!("zstd,{}", level),
            Compression::Zlib { level } => format!("zlib,{}", level),
        }
    }
}

impl Default for Compression {
    fn default() -> Self {
        Self::Zstd { level: 3 }
    }
}

pub fn validate_archive_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(BorgError::InvalidConfig {
            message: "archive_name cannot be empty".into(),
        });
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(BorgError::InvalidConfig {
            message:
                "archive_name contains invalid characters (only alphanumeric, -, _, . allowed)"
                    .into(),
        });
    }
    Ok(())
}

const VALID_ENCRYPTION_MODES: &[&str] = &[
    "none",
    "authenticated",
    "authenticated-blake2",
    "repokey",
    "keyfile",
    "repokey-blake2",
    "keyfile-blake2",
];

pub fn validate_encryption_mode(mode: &str) -> Result<()> {
    if VALID_ENCRYPTION_MODES.contains(&mode) {
        Ok(())
    } else {
        Err(BorgError::InvalidConfig {
            message: format!("invalid encryption mode: {}", mode),
        })
    }
}

pub fn validate_source_paths(paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Err(BorgError::InvalidConfig {
            message: "at least one source path is required".into(),
        });
    }
    for path in paths {
        if path.trim().is_empty() {
            return Err(BorgError::InvalidConfig {
                message: "source path cannot be empty".into(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub profiles: Vec<BackupProfile>,
    pub borg_binary_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_url_formats_correctly() {
        let repo = RepoConfig {
            ssh_host: "backup.example.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/data/backups/my-pc".into(),
            ssh_key_path: None,
        };
        assert_eq!(
            repo.ssh_url(),
            "ssh://borg@backup.example.com:22//data/backups/my-pc"
        );
    }

    #[test]
    fn ssh_url_with_custom_port() {
        let repo = RepoConfig {
            ssh_host: "10.0.0.1".into(),
            ssh_port: 2222,
            ssh_user: "admin".into(),
            repo_path: "/repos/test".into(),
            ssh_key_path: Some(PathBuf::from("/home/user/.ssh/id_ed25519")),
        };
        assert_eq!(repo.ssh_url(), "ssh://admin@10.0.0.1:2222//repos/test");
    }

    #[test]
    fn compression_default_is_zstd_3() {
        let comp = Compression::default();
        match comp {
            Compression::Zstd { level } => assert_eq!(level, 3),
            _ => panic!("expected Zstd default"),
        }
    }

    #[test]
    fn repo_config_roundtrip_json() {
        let repo = RepoConfig {
            ssh_host: "host.example.com".into(),
            ssh_port: 22,
            ssh_user: "user".into(),
            repo_path: "/backups/repo".into(),
            ssh_key_path: Some(PathBuf::from("C:\\Users\\me\\.ssh\\key")),
        };
        let json = serde_json::to_string(&repo).unwrap();
        let deserialized: RepoConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ssh_host, "host.example.com");
        assert_eq!(deserialized.ssh_port, 22);
        assert_eq!(
            deserialized.ssh_key_path,
            Some(PathBuf::from("C:\\Users\\me\\.ssh\\key"))
        );
    }

    #[test]
    fn backup_profile_roundtrip_json() {
        let profile = BackupProfile {
            name: "daily".into(),
            source_paths: vec![PathBuf::from("C:\\Users\\me\\Documents")],
            excludes: vec!["*.tmp".into(), "node_modules".into()],
            compression: Compression::Lz4,
            repo: RepoConfig {
                ssh_host: "srv".into(),
                ssh_port: 22,
                ssh_user: "borg".into(),
                repo_path: "/repo".into(),
                ssh_key_path: None,
            },
        };
        let json = serde_json::to_string(&profile).unwrap();
        let deserialized: BackupProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "daily");
        assert_eq!(deserialized.excludes.len(), 2);
        assert!(matches!(deserialized.compression, Compression::Lz4));
    }

    #[test]
    fn app_config_roundtrip_json() {
        let config = AppConfig {
            profiles: vec![],
            borg_binary_path: PathBuf::from("borg.exe"),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.profiles.is_empty());
        assert_eq!(deserialized.borg_binary_path, PathBuf::from("borg.exe"));
    }

    #[test]
    fn ssh_url_relative_path() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "repos/myrepo".into(),
            ssh_key_path: None,
        };
        assert_eq!(repo.ssh_url(), "ssh://borg@host.com:22/repos/myrepo");
    }

    #[test]
    fn all_compression_variants_roundtrip() {
        for comp in [
            Compression::None,
            Compression::Lz4,
            Compression::Zstd { level: 9 },
            Compression::Zlib { level: 6 },
        ] {
            let json = serde_json::to_string(&comp).unwrap();
            let _: Compression = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn rejects_ssh_host_with_semicolon() {
        let repo = RepoConfig {
            ssh_host: "host;rm -rf /".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/repo".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_err());
    }

    #[test]
    fn rejects_ssh_user_with_at() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "user@evil".into(),
            repo_path: "/repo".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_err());
    }

    #[test]
    fn rejects_port_zero() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 0,
            ssh_user: "borg".into(),
            repo_path: "/repo".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_err());
    }

    #[test]
    fn rejects_empty_ssh_user() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "".into(),
            repo_path: "/repo".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_err());
    }

    #[test]
    fn accepts_valid_repo_config() {
        let repo = RepoConfig {
            ssh_host: "backup.example.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/data/backups/my-pc".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_ok());
    }

    #[test]
    fn rejects_zstd_level_0() {
        assert!(Compression::Zstd { level: 0 }.validate().is_err());
    }

    #[test]
    fn rejects_zstd_level_23() {
        assert!(Compression::Zstd { level: 23 }.validate().is_err());
    }

    #[test]
    fn accepts_zstd_level_1() {
        assert!(Compression::Zstd { level: 1 }.validate().is_ok());
    }

    #[test]
    fn accepts_zstd_level_22() {
        assert!(Compression::Zstd { level: 22 }.validate().is_ok());
    }

    #[test]
    fn rejects_zlib_level_10() {
        assert!(Compression::Zlib { level: 10 }.validate().is_err());
    }

    #[test]
    fn accepts_zlib_level_9() {
        assert!(Compression::Zlib { level: 9 }.validate().is_ok());
    }

    #[test]
    fn accepts_zlib_level_0() {
        assert!(Compression::Zlib { level: 0 }.validate().is_ok());
    }

    #[test]
    fn none_and_lz4_always_valid() {
        assert!(Compression::None.validate().is_ok());
        assert!(Compression::Lz4.validate().is_ok());
    }

    #[test]
    fn rejects_repo_path_with_semicolon() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/repo;rm -rf /".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_err());
        assert!(
            repo.validate()
                .unwrap_err()
                .to_string()
                .contains("repo_path")
        );
    }

    #[test]
    fn rejects_repo_path_with_pipe() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/repo|evil".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_err());
    }

    #[test]
    fn accepts_repo_path_with_spaces() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/my backups/repo".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_ok());
    }

    #[test]
    fn accepts_repo_path_with_at_and_colon() {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "C:\\backups\\repo".into(),
            ssh_key_path: None,
        };
        assert!(repo.validate().is_ok());
    }

    #[test]
    fn validate_returns_borg_error() {
        let repo = RepoConfig {
            ssh_host: "".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/repo".into(),
            ssh_key_path: None,
        };
        let err = repo.validate().unwrap_err();
        assert!(matches!(err, crate::error::BorgError::InvalidConfig { .. }));
    }

    #[test]
    fn compression_to_borg_arg_none() {
        assert_eq!(Compression::None.to_borg_arg(), "none");
    }

    #[test]
    fn compression_to_borg_arg_lz4() {
        assert_eq!(Compression::Lz4.to_borg_arg(), "lz4");
    }

    #[test]
    fn compression_to_borg_arg_zstd() {
        assert_eq!(Compression::Zstd { level: 9 }.to_borg_arg(), "zstd,9");
    }

    #[test]
    fn compression_to_borg_arg_zlib() {
        assert_eq!(Compression::Zlib { level: 6 }.to_borg_arg(), "zlib,6");
    }

    #[test]
    fn accepts_valid_archive_name() {
        assert!(validate_archive_name("my-backup_2024.01.15").is_ok());
    }

    #[test]
    fn accepts_archive_name_alphanumeric_only() {
        assert!(validate_archive_name("backup2024").is_ok());
    }

    #[test]
    fn rejects_empty_archive_name() {
        assert!(validate_archive_name("").is_err());
    }

    #[test]
    fn rejects_whitespace_only_archive_name() {
        assert!(validate_archive_name("   ").is_err());
    }

    #[test]
    fn rejects_archive_name_with_semicolon() {
        assert!(validate_archive_name("backup;rm -rf /").is_err());
    }

    #[test]
    fn rejects_archive_name_with_space() {
        assert!(validate_archive_name("my backup").is_err());
    }

    #[test]
    fn rejects_archive_name_with_slash() {
        assert!(validate_archive_name("backup/evil").is_err());
    }

    #[test]
    fn rejects_archive_name_with_shell_expansion() {
        assert!(validate_archive_name("$(whoami)").is_err());
    }

    #[test]
    fn rejects_archive_name_with_backtick() {
        assert!(validate_archive_name("`id`").is_err());
    }

    #[test]
    fn rejects_archive_name_with_colons() {
        assert!(validate_archive_name("repo::evil").is_err());
    }

    #[test]
    fn accepts_source_paths_valid() {
        let paths = vec!["/home/user/docs".to_string(), "C:\\Users\\me".to_string()];
        assert!(validate_source_paths(&paths).is_ok());
    }

    #[test]
    fn rejects_empty_source_paths() {
        let paths: Vec<String> = vec![];
        assert!(validate_source_paths(&paths).is_err());
    }

    #[test]
    fn rejects_source_paths_with_empty_entry() {
        let paths = vec!["/valid/path".to_string(), "".to_string()];
        assert!(validate_source_paths(&paths).is_err());
    }

    #[test]
    fn rejects_source_paths_with_whitespace_entry() {
        let paths = vec!["  ".to_string()];
        assert!(validate_source_paths(&paths).is_err());
    }

    #[test]
    fn accepts_repokey_blake2_encryption() {
        assert!(validate_encryption_mode("repokey-blake2").is_ok());
    }

    #[test]
    fn accepts_all_documented_encryption_modes() {
        for mode in [
            "none",
            "authenticated",
            "authenticated-blake2",
            "repokey",
            "keyfile",
            "repokey-blake2",
            "keyfile-blake2",
        ] {
            assert!(
                validate_encryption_mode(mode).is_ok(),
                "mode should be valid: {}",
                mode
            );
        }
    }

    #[test]
    fn rejects_unknown_encryption_mode() {
        assert!(validate_encryption_mode("super-secret").is_err());
    }

    #[test]
    fn rejects_empty_encryption_mode() {
        assert!(validate_encryption_mode("").is_err());
    }

    #[test]
    fn rejects_encryption_mode_with_shell_injection() {
        assert!(validate_encryption_mode("none; rm -rf /").is_err());
        assert!(validate_encryption_mode("repokey$(whoami)").is_err());
        assert!(validate_encryption_mode("`id`").is_err());
    }
}
