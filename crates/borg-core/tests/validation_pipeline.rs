use std::path::PathBuf;

use borg_core::config::{
    BackupProfile, Compression, RepoConfig, validate_archive_name, validate_source_paths,
};

fn valid_repo() -> RepoConfig {
    RepoConfig {
        ssh_host: "backup.example.com".into(),
        ssh_port: 22,
        ssh_user: "borg".into(),
        repo_path: "/data/backups/my-pc".into(),
        ssh_key_path: None,
    }
}

#[test]
fn full_backup_validation_flow() {
    let repo = valid_repo();
    repo.validate().unwrap();

    let archive_name = "daily-2024.01.15";
    validate_archive_name(archive_name).unwrap();

    let source_paths = vec![
        "C:\\Users\\me\\Documents".to_string(),
        "C:\\Users\\me\\Pictures".to_string(),
    ];
    validate_source_paths(&source_paths).unwrap();

    let compression = Compression::default();
    compression.validate().unwrap();

    let profile = BackupProfile {
        name: "daily".into(),
        source_paths: source_paths.into_iter().map(PathBuf::from).collect(),
        excludes: vec!["*.tmp".into(), "node_modules".into()],
        compression,
        repo: repo.clone(),
    };

    let archive_ref = format!("{}::{}", repo.ssh_url(), archive_name);
    assert!(archive_ref.contains("ssh://"));
    assert!(archive_ref.contains("::daily-2024.01.15"));
    assert_eq!(profile.source_paths.len(), 2);
    assert_eq!(profile.excludes.len(), 2);
}

#[test]
fn invalid_repo_blocks_backup_flow() {
    let repo = RepoConfig {
        ssh_host: "".into(),
        ssh_port: 22,
        ssh_user: "borg".into(),
        repo_path: "/repo".into(),
        ssh_key_path: None,
    };
    let err = repo.validate().unwrap_err();
    assert!(err.to_string().contains("ssh_host"));
}

#[test]
fn invalid_archive_name_blocks_backup_flow() {
    let repo = valid_repo();
    repo.validate().unwrap();

    let err = validate_archive_name("backup;rm -rf /").unwrap_err();
    assert!(err.to_string().contains("invalid characters"));
}

#[test]
fn empty_source_paths_blocks_backup_flow() {
    let repo = valid_repo();
    repo.validate().unwrap();
    validate_archive_name("valid-name").unwrap();

    let err = validate_source_paths(&[]).unwrap_err();
    assert!(err.to_string().contains("at least one"));
}

const INJECTION_PAYLOADS: &[&str] = &[
    "; rm -rf /",
    "| cat /etc/passwd",
    "$(whoami)",
    "`id`",
    "name\nContent-Length: 0",
    "repo::injected",
    "../../../etc/passwd",
    "name&& curl evil.com",
    "name' OR '1'='1",
    "${IFS}cat${IFS}/etc/passwd",
    "name\0hidden",
];

#[test]
fn archive_name_rejects_all_injection_payloads() {
    for payload in INJECTION_PAYLOADS {
        assert!(
            validate_archive_name(payload).is_err(),
            "archive_name should reject: {:?}",
            payload
        );
    }
}

#[test]
fn repo_path_rejects_shell_injection() {
    let dangerous_paths = &[
        "/repo;rm -rf /",
        "/repo|evil",
        "/repo`id`",
        "/repo$(whoami)",
        "/repo'DROP TABLE",
        "/repo\"injected",
    ];
    for path in dangerous_paths {
        let repo = RepoConfig {
            ssh_host: "host.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: (*path).into(),
            ssh_key_path: None,
        };
        assert!(
            repo.validate().is_err(),
            "repo_path should reject: {:?}",
            path
        );
    }
}

#[test]
fn ssh_host_rejects_injection() {
    let dangerous_hosts = &[
        "host;evil",
        "host@evil",
        "host:evil",
        "host evil",
        "host'evil",
        "host\"evil",
        "host|evil",
        "host`evil`",
        "host$var",
    ];
    for host in dangerous_hosts {
        let repo = RepoConfig {
            ssh_host: (*host).into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/repo".into(),
            ssh_key_path: None,
        };
        assert!(
            repo.validate().is_err(),
            "ssh_host should reject: {:?}",
            host
        );
    }
}

#[test]
fn compression_validate_and_format_all_variants() {
    let cases: Vec<(Compression, &str)> = vec![
        (Compression::None, "none"),
        (Compression::Lz4, "lz4"),
        (Compression::Zstd { level: 1 }, "zstd,1"),
        (Compression::Zstd { level: 22 }, "zstd,22"),
        (Compression::Zlib { level: 0 }, "zlib,0"),
        (Compression::Zlib { level: 9 }, "zlib,9"),
    ];
    for (comp, expected_arg) in cases {
        comp.validate().unwrap();
        assert_eq!(comp.to_borg_arg(), expected_arg);
    }
}

#[test]
fn invalid_compression_rejected_before_formatting() {
    let invalid = vec![
        Compression::Zstd { level: 0 },
        Compression::Zstd { level: 23 },
        Compression::Zlib { level: 10 },
    ];
    for comp in invalid {
        assert!(comp.validate().is_err());
    }
}

#[test]
fn multiple_validation_errors_are_independent() {
    let bad_repo = RepoConfig {
        ssh_host: "host;evil".into(),
        ssh_port: 0,
        ssh_user: "".into(),
        repo_path: "".into(),
        ssh_key_path: None,
    };
    let err = bad_repo.validate().unwrap_err();
    // First validation error wins (ssh_user empty check comes before character check)
    assert!(
        err.to_string().contains("ssh_host")
            || err.to_string().contains("ssh_user")
            || err.to_string().contains("repo_path")
            || err.to_string().contains("ssh_port")
    );
}
