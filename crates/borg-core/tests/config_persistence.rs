use std::path::PathBuf;

use borg_core::config::{AppConfig, BackupProfile, Compression, RepoConfig};

#[test]
fn repo_config_roundtrip_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("repo.json");

    let config = RepoConfig {
        ssh_host: "backup.example.com".into(),
        ssh_port: 2222,
        ssh_user: "borguser".into(),
        repo_path: "/mnt/backups/workstation".into(),
        ssh_key_path: Some(PathBuf::from("C:\\Users\\me\\.ssh\\id_ed25519")),
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&path, &json).unwrap();

    let read_back = std::fs::read_to_string(&path).unwrap();
    let loaded: RepoConfig = serde_json::from_str(&read_back).unwrap();

    assert_eq!(loaded.ssh_host, "backup.example.com");
    assert_eq!(loaded.ssh_port, 2222);
    assert_eq!(loaded.ssh_user, "borguser");
    assert_eq!(loaded.repo_path, "/mnt/backups/workstation");
    assert_eq!(
        loaded.ssh_key_path,
        Some(PathBuf::from("C:\\Users\\me\\.ssh\\id_ed25519"))
    );
    loaded.validate().unwrap();
}

#[test]
fn backup_profile_roundtrip_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.json");

    let profile = BackupProfile {
        name: "nightly".into(),
        source_paths: vec![
            PathBuf::from("C:\\Users\\me\\Documents"),
            PathBuf::from("C:\\Users\\me\\Projects"),
        ],
        excludes: vec![
            "*.tmp".into(),
            "node_modules".into(),
            "target".into(),
            ".git".into(),
        ],
        compression: Compression::Zstd { level: 6 },
        repo: RepoConfig {
            ssh_host: "nas.local".into(),
            ssh_port: 22,
            ssh_user: "backup".into(),
            repo_path: "/volume1/borg/workstation".into(),
            ssh_key_path: None,
        },
    };

    let json = serde_json::to_string_pretty(&profile).unwrap();
    std::fs::write(&path, &json).unwrap();

    let read_back = std::fs::read_to_string(&path).unwrap();
    let loaded: BackupProfile = serde_json::from_str(&read_back).unwrap();

    assert_eq!(loaded.name, "nightly");
    assert_eq!(loaded.source_paths.len(), 2);
    assert_eq!(loaded.excludes.len(), 4);
    assert!(matches!(loaded.compression, Compression::Zstd { level: 6 }));
    loaded.repo.validate().unwrap();
    loaded.compression.validate().unwrap();
}

#[test]
fn app_config_roundtrip_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");

    let config = AppConfig {
        profiles: vec![
            BackupProfile {
                name: "docs".into(),
                source_paths: vec![PathBuf::from("/home/user/docs")],
                excludes: vec![],
                compression: Compression::Lz4,
                repo: RepoConfig {
                    ssh_host: "server.com".into(),
                    ssh_port: 22,
                    ssh_user: "borg".into(),
                    repo_path: "/repo/docs".into(),
                    ssh_key_path: None,
                },
            },
            BackupProfile {
                name: "photos".into(),
                source_paths: vec![PathBuf::from("/home/user/photos")],
                excludes: vec!["thumbs.db".into()],
                compression: Compression::None,
                repo: RepoConfig {
                    ssh_host: "server.com".into(),
                    ssh_port: 22,
                    ssh_user: "borg".into(),
                    repo_path: "/repo/photos".into(),
                    ssh_key_path: None,
                },
            },
        ],
        borg_binary_path: PathBuf::from("C:\\Program Files\\borg\\borg.exe"),
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&path, &json).unwrap();

    let read_back = std::fs::read_to_string(&path).unwrap();
    let loaded: AppConfig = serde_json::from_str(&read_back).unwrap();

    assert_eq!(loaded.profiles.len(), 2);
    assert_eq!(loaded.profiles[0].name, "docs");
    assert_eq!(loaded.profiles[1].name, "photos");
    assert_eq!(
        loaded.borg_binary_path,
        PathBuf::from("C:\\Program Files\\borg\\borg.exe")
    );

    for profile in &loaded.profiles {
        profile.repo.validate().unwrap();
        profile.compression.validate().unwrap();
    }
}

#[test]
fn corrupt_config_file_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");

    std::fs::write(&path, "{ not valid json }").unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    let result = serde_json::from_str::<RepoConfig>(&content);
    assert!(result.is_err());
}

#[test]
fn partial_config_missing_fields_returns_error() {
    let json = r#"{"ssh_host": "server.com", "ssh_port": 22}"#;
    let result = serde_json::from_str::<RepoConfig>(json);
    assert!(result.is_err());
}

#[test]
fn config_atomic_write_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("repo.json");
    let tmp_path = dir.path().join("repo.json.tmp");

    let config = RepoConfig {
        ssh_host: "host.com".into(),
        ssh_port: 22,
        ssh_user: "borg".into(),
        repo_path: "/repo".into(),
        ssh_key_path: None,
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(&tmp_path, &json).unwrap();
    std::fs::rename(&tmp_path, &final_path).unwrap();

    assert!(!tmp_path.exists());
    assert!(final_path.exists());

    let loaded: RepoConfig =
        serde_json::from_str(&std::fs::read_to_string(&final_path).unwrap()).unwrap();
    assert_eq!(loaded.ssh_host, "host.com");
}

#[test]
fn missing_config_file_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let result = std::fs::read_to_string(&path);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
}
