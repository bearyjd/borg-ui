use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zip::write::SimpleFileOptions;

use crate::history;
use crate::profiles::{self, PROFILE_SCHEMA_VERSION, Profile, ProfilesData};
use crate::redaction;

pub const EXPORT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMetadata {
    pub format_version: u32,
    pub created_at: String,
    pub borgui_version: String,
    pub source_platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationExport {
    pub metadata: ExportMetadata,
    pub configuration: ProfilesData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub format_version: u32,
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub removed: Vec<String>,
    pub active_profile: Option<String>,
}

pub async fn export_configuration(config_dir: &Path, destination: &Path) -> Result<(), String> {
    let data = profiles::load(config_dir).await?;
    let document = ConfigurationExport {
        metadata: metadata(),
        configuration: sanitized_profiles(data),
    };
    write_json_atomic(destination, &document).await
}

pub async fn preview_import(config_dir: &Path, source: &Path) -> Result<ImportPreview, String> {
    let imported = read_and_validate_import(source).await?;
    let current = profiles::load(config_dir).await?;
    let current_ids: HashSet<_> = current.profiles.iter().map(|p| p.id.as_str()).collect();
    let imported_ids: HashSet<_> = imported
        .configuration
        .profiles
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    let mut added = Vec::new();
    let mut replaced = Vec::new();
    let removed = current
        .profiles
        .iter()
        .filter(|profile| !imported_ids.contains(profile.id.as_str()))
        .map(|profile| profile.name.clone())
        .collect();
    for profile in &imported.configuration.profiles {
        if current_ids.contains(profile.id.as_str()) {
            replaced.push(profile.name.clone());
        } else {
            added.push(profile.name.clone());
        }
    }
    Ok(ImportPreview {
        format_version: imported.metadata.format_version,
        added,
        replaced,
        removed,
        active_profile: imported.configuration.active_id,
    })
}

pub async fn import_configuration(config_dir: &Path, source: &Path) -> Result<(), String> {
    let imported = read_and_validate_import(source).await?;
    tokio::fs::create_dir_all(config_dir)
        .await
        .map_err(|e| e.to_string())?;
    let profiles_path = config_dir.join("profiles.json");
    if tokio::fs::try_exists(&profiles_path)
        .await
        .map_err(|e| e.to_string())?
    {
        let rollback = config_dir.join(format!(
            "profiles.rollback-{}.json",
            Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
        tokio::fs::copy(&profiles_path, rollback)
            .await
            .map_err(|e| format!("failed to create rollback copy: {e}"))?;
    }
    profiles::save(config_dir, &imported.configuration).await
}

pub async fn export_support_bundle(
    config_dir: &Path,
    log_dir: &Path,
    destination: &Path,
) -> Result<(), String> {
    let profiles = sanitized_profiles(profiles::load(config_dir).await?);
    let mut events = history::load(config_dir).await?;
    for event in &mut events {
        event.archive_name = "[redacted]".into();
        event.error_message = None;
    }
    let config_json = serde_json::to_vec_pretty(&ConfigurationExport {
        metadata: metadata(),
        configuration: profiles,
    })
    .map_err(|e| e.to_string())?;
    let versions = serde_json::to_vec_pretty(&json!({
        "metadata": metadata(),
        "database_schema_version": 1,
        "profile_schema_version": PROFILE_SCHEMA_VERSION,
    }))
    .map_err(|e| e.to_string())?;
    let history_json = serde_json::to_vec_pretty(&events).map_err(|e| e.to_string())?;
    let log_dir = log_dir.to_path_buf();
    let destination = destination.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = temporary_path(&destination);
        let file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        add_zip_file(&mut zip, "versions.json", &versions, options)?;
        add_zip_file(&mut zip, "configuration.json", &config_json, options)?;
        add_zip_file(&mut zip, "operation-history.json", &history_json, options)?;

        if let Ok(entries) = std::fs::read_dir(log_dir) {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .collect();
            paths.sort();
            for path in paths.into_iter().rev().take(7) {
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                add_zip_file(
                    &mut zip,
                    &format!("logs/{name}"),
                    redaction::redact(&content).as_bytes(),
                    options,
                )?;
            }
        }
        zip.finish().map_err(|e| e.to_string())?;
        replace_file(&temporary, &destination)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn sanitized_profiles(mut data: ProfilesData) -> ProfilesData {
    for profile in &mut data.profiles {
        profile.repo.ssh_key_path = None;
        profile.pre_backup = profile.pre_backup.take().map(|v| redaction::redact(&v));
        profile.post_backup = profile.post_backup.take().map(|v| redaction::redact(&v));
    }
    data
}

async fn read_and_validate_import(source: &Path) -> Result<ConfigurationExport, String> {
    let json = tokio::fs::read_to_string(source)
        .await
        .map_err(|e| e.to_string())?;
    let document: ConfigurationExport =
        serde_json::from_str(&json).map_err(|e| format!("invalid configuration export: {e}"))?;
    if document.metadata.format_version != EXPORT_FORMAT_VERSION {
        return Err(format!(
            "unsupported configuration format version {}",
            document.metadata.format_version
        ));
    }
    validate_profiles(&document.configuration)?;
    Ok(document)
}

fn validate_profiles(data: &ProfilesData) -> Result<(), String> {
    if data.schema_version > PROFILE_SCHEMA_VERSION {
        return Err(format!(
            "configuration schema version {} is newer than supported version {PROFILE_SCHEMA_VERSION}",
            data.schema_version
        ));
    }
    let mut ids = HashSet::new();
    for profile in &data.profiles {
        validate_profile(profile)?;
        if !ids.insert(&profile.id) {
            return Err(format!("duplicate profile id: {}", profile.id));
        }
    }
    if let Some(active) = &data.active_id
        && !ids.contains(active)
    {
        return Err(format!("active profile does not exist: {active}"));
    }
    Ok(())
}

fn validate_profile(profile: &Profile) -> Result<(), String> {
    if profile.id.trim().is_empty() || profile.name.trim().is_empty() {
        return Err("profile ids and names cannot be empty".into());
    }
    profile.repo.validate().map_err(|e| e.to_string())?;
    if let Some(retention) = &profile.retention {
        retention.validate().map_err(|e| e.to_string())?;
    }
    if let Some(schedule) = &profile.schedule {
        schedule.schedule.validate().map_err(|e| e.to_string())?;
        borg_core::config::validate_source_paths(&schedule.source_paths)
            .map_err(|e| e.to_string())?;
        borg_core::config::validate_exclude_patterns(&schedule.excludes)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn metadata() -> ExportMetadata {
    ExportMetadata {
        format_version: EXPORT_FORMAT_VERSION,
        created_at: Utc::now().to_rfc3339(),
        borgui_version: env!("CARGO_PKG_VERSION").into(),
        source_platform: std::env::consts::OS.into(),
    }
}

async fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let temporary = temporary_path(path);
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|e| e.to_string())?;
    replace_file(&temporary, path)
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

fn replace_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        std::fs::remove_file(destination).map_err(|e| e.to_string())?;
    }
    std::fs::rename(temporary, destination).map_err(|e| e.to_string())
}

fn add_zip_file(
    zip: &mut zip::ZipWriter<std::fs::File>,
    name: &str,
    bytes: &[u8],
    options: SimpleFileOptions,
) -> Result<(), String> {
    zip.start_file(name, options).map_err(|e| e.to_string())?;
    zip.write_all(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, name: &str) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            repo: borg_core::config::RepoConfig {
                ssh_host: "host".into(),
                ssh_port: 22,
                ssh_user: "user".into(),
                repo_path: "/repo".into(),
                ssh_key_path: Some(PathBuf::from("/secret/id_ed25519")),
            },
            schedule: None,
            integrity_schedule: None,
            retention: None,
            archive_template: None,
            pre_backup: None,
            post_backup: None,
        }
    }

    #[test]
    fn duplicate_profile_ids_are_rejected() {
        let repo = borg_core::config::RepoConfig {
            ssh_host: "host".into(),
            ssh_port: 22,
            ssh_user: "user".into(),
            repo_path: "/repo".into(),
            ssh_key_path: None,
        };
        let profile = Profile {
            id: "same".into(),
            name: "One".into(),
            repo,
            schedule: None,
            integrity_schedule: None,
            retention: None,
            archive_template: None,
            pre_backup: None,
            post_backup: None,
        };
        let data = ProfilesData {
            schema_version: PROFILE_SCHEMA_VERSION,
            profiles: vec![profile.clone(), profile],
            active_id: Some("same".into()),
        };
        assert!(validate_profiles(&data).unwrap_err().contains("duplicate"));
    }

    #[tokio::test]
    async fn export_import_round_trip_excludes_key_and_creates_rollback() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let export_path = source.path().join("configuration.json");
        let source_data = ProfilesData {
            schema_version: PROFILE_SCHEMA_VERSION,
            profiles: vec![profile("work", "Work")],
            active_id: Some("work".into()),
        };
        profiles::save(source.path(), &source_data).await.unwrap();

        export_configuration(source.path(), &export_path)
            .await
            .unwrap();
        let exported = tokio::fs::read_to_string(&export_path).await.unwrap();
        assert!(!exported.contains("id_ed25519"));

        profiles::save(
            destination.path(),
            &ProfilesData {
                schema_version: PROFILE_SCHEMA_VERSION,
                profiles: vec![profile("old", "Old")],
                active_id: Some("old".into()),
            },
        )
        .await
        .unwrap();
        let preview = preview_import(destination.path(), &export_path)
            .await
            .unwrap();
        assert_eq!(preview.added, vec!["Work"]);
        assert_eq!(preview.removed, vec!["Old"]);
        import_configuration(destination.path(), &export_path)
            .await
            .unwrap();
        let imported = profiles::load(destination.path()).await.unwrap();
        assert_eq!(imported.active_id.as_deref(), Some("work"));
        assert!(imported.profiles[0].repo.ssh_key_path.is_none());
        assert!(
            std::fs::read_dir(destination.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("profiles.rollback-"))
        );
    }
}
