use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub repo_path: String,
    pub ssh_key_path: Option<PathBuf>,
}

impl RepoConfig {
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

impl Default for Compression {
    fn default() -> Self {
        Self::Zstd { level: 3 }
    }
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
        assert_eq!(deserialized.ssh_key_path, Some(PathBuf::from("C:\\Users\\me\\.ssh\\key")));
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
}
