use std::path::{Path, PathBuf};

use borg_core::config::{RepoConfig, RetentionConfig};
use borg_platform_win::scheduler::ScheduleConfig;
use serde::{Deserialize, Serialize};

pub const PROFILE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegritySchedule {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub repo: RepoConfig,
    #[serde(default)]
    pub schedule: Option<ScheduleConfig>,
    #[serde(default)]
    pub integrity_schedule: Option<IntegritySchedule>,
    #[serde(default)]
    pub retention: Option<RetentionConfig>,
    #[serde(default)]
    pub archive_template: Option<String>,
    /// Shell command run immediately before a backup. A non-zero exit aborts the
    /// backup. Supports `$repo_url` / `$archive_name` substitution.
    #[serde(default)]
    pub pre_backup: Option<String>,
    /// Shell command run after a successful backup. A failure here is surfaced as
    /// a warning (the backup already completed). Same substitutions as above.
    #[serde(default)]
    pub post_backup: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilesData {
    #[serde(default)]
    pub schema_version: u32,
    pub profiles: Vec<Profile>,
    pub active_id: Option<String>,
}

impl Default for ProfilesData {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            profiles: Vec::new(),
            active_id: None,
        }
    }
}

impl ProfilesData {
    pub fn active(&self) -> Option<&Profile> {
        let id = self.active_id.as_ref()?;
        self.profiles.iter().find(|p| &p.id == id)
    }

    pub fn active_mut(&mut self) -> Option<&mut Profile> {
        let id = self.active_id.clone()?;
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    pub fn set_active(&mut self, id: &str) -> Result<(), String> {
        if self.profiles.iter().any(|p| p.id == id) {
            self.active_id = Some(id.into());
            Ok(())
        } else {
            Err(format!("profile not found: {}", id))
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        if self.profiles.len() == before {
            return Err(format!("profile not found: {}", id));
        }
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self.profiles.first().map(|p| p.id.clone());
        }
        Ok(())
    }
}

pub async fn load(config_dir: &Path) -> Result<ProfilesData, String> {
    let path = config_dir.join("profiles.json");
    match tokio::fs::read_to_string(&path).await {
        Ok(data) => {
            let value: serde_json::Value =
                serde_json::from_str(&data).map_err(|e| format!("invalid profiles.json: {e}"))?;
            let version = value
                .get("schema_version")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if version > u64::from(PROFILE_SCHEMA_VERSION) {
                return Err(format!(
                    "profiles.json schema version {version} is newer than supported version {PROFILE_SCHEMA_VERSION}"
                ));
            }
            let mut parsed: ProfilesData =
                serde_json::from_value(value).map_err(|e| format!("invalid profiles.json: {e}"))?;
            if version < u64::from(PROFILE_SCHEMA_VERSION) {
                parsed.schema_version = PROFILE_SCHEMA_VERSION;
                save(config_dir, &parsed).await?;
            }
            Ok(parsed)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let data = migrate_legacy(config_dir).await?;
            if !data.profiles.is_empty() {
                save(config_dir, &data).await?;
            }
            Ok(data)
        }
        Err(e) => Err(e.to_string()),
    }
}

pub async fn save(config_dir: &Path, data: &ProfilesData) -> Result<(), String> {
    tokio::fs::create_dir_all(config_dir)
        .await
        .map_err(|e| e.to_string())?;
    let path = config_dir.join("profiles.json");
    let tmp = config_dir.join("profiles.json.tmp");
    let json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    tokio::fs::write(&tmp, &json)
        .await
        .map_err(|e| e.to_string())?;
    replace_atomic(&tmp, &path).await
}

#[cfg(not(windows))]
async fn replace_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    tokio::fs::rename(source, destination)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
async fn replace_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    tokio::task::spawn_blocking(move || {
        // SAFETY: both pointers reference NUL-terminated UTF-16 buffers that
        // remain alive for the duration of the call.
        let succeeded = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if succeeded == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn migrate_legacy(config_dir: &Path) -> Result<ProfilesData, String> {
    let repo = read_json::<RepoConfig>(&config_dir.join("repo.json")).await?;

    let Some(repo) = repo else {
        return Ok(ProfilesData::default());
    };

    let schedule = read_json::<ScheduleConfig>(&config_dir.join("schedule.json"))
        .await
        .unwrap_or(None);
    let retention = read_json::<RetentionConfig>(&config_dir.join("retention.json"))
        .await
        .unwrap_or(None);

    let profile = Profile {
        id: "default".into(),
        name: "Default".into(),
        repo,
        schedule,
        integrity_schedule: None,
        retention,
        archive_template: None,
        pre_backup: None,
        post_backup: None,
    };

    Ok(ProfilesData {
        schema_version: PROFILE_SCHEMA_VERSION,
        active_id: Some(profile.id.clone()),
        profiles: vec![profile],
    })
}

pub fn make_profile_id(name: &str, data: &ProfilesData) -> String {
    let base: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let base = base.trim_matches('-').to_string();
    let base = if base.is_empty() {
        "profile".into()
    } else {
        base
    };
    if !data.profiles.iter().any(|p| p.id == base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{}-{}", base, n);
        if !data.profiles.iter().any(|p| p.id == candidate) {
            return candidate;
        }
        n += 1;
    }
}

async fn read_json<T: for<'de> Deserialize<'de>>(path: &PathBuf) -> Result<Option<T>, String> {
    match tokio::fs::read_to_string(path).await {
        Ok(data) => serde_json::from_str(&data)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_repo() -> RepoConfig {
        RepoConfig {
            ssh_host: "backup.example.com".into(),
            ssh_port: 22,
            ssh_user: "borg".into(),
            repo_path: "/data/repo".into(),
            ssh_key_path: None,
        }
    }

    fn profile_with_id(id: &str) -> Profile {
        Profile {
            id: id.into(),
            name: id.into(),
            repo: sample_repo(),
            schedule: None,
            integrity_schedule: None,
            retention: None,
            archive_template: None,
            pre_backup: None,
            post_backup: None,
        }
    }

    #[test]
    fn make_id_kebab_case() {
        let data = ProfilesData::default();
        assert_eq!(make_profile_id("Work Laptop", &data), "work-laptop");
    }

    #[test]
    fn make_id_strips_non_alphanumeric_edges() {
        let data = ProfilesData::default();
        assert_eq!(make_profile_id("!Work!", &data), "work");
        assert_eq!(make_profile_id("--leading", &data), "leading");
    }

    #[test]
    fn make_id_empty_name_falls_back() {
        let data = ProfilesData::default();
        assert_eq!(make_profile_id("!!!", &data), "profile");
        assert_eq!(make_profile_id("", &data), "profile");
    }

    #[test]
    fn make_id_handles_collision() {
        let mut data = ProfilesData::default();
        data.profiles.push(profile_with_id("work"));
        assert_eq!(make_profile_id("Work", &data), "work-2");
    }

    #[test]
    fn make_id_handles_multiple_collisions() {
        let mut data = ProfilesData::default();
        data.profiles.push(profile_with_id("work"));
        data.profiles.push(profile_with_id("work-2"));
        data.profiles.push(profile_with_id("work-3"));
        assert_eq!(make_profile_id("Work", &data), "work-4");
    }

    #[test]
    fn set_active_rejects_unknown_id() {
        let mut data = ProfilesData::default();
        data.profiles.push(profile_with_id("a"));
        assert!(data.set_active("ghost").is_err());
        assert!(data.set_active("a").is_ok());
        assert_eq!(data.active_id.as_deref(), Some("a"));
    }

    #[test]
    fn remove_reassigns_active_to_first_remaining() {
        let mut data = ProfilesData::default();
        data.profiles.push(profile_with_id("a"));
        data.profiles.push(profile_with_id("b"));
        data.active_id = Some("a".into());

        data.remove("a").unwrap();
        assert_eq!(data.active_id.as_deref(), Some("b"));
    }

    #[test]
    fn remove_clears_active_when_last_profile_deleted() {
        let mut data = ProfilesData::default();
        data.profiles.push(profile_with_id("a"));
        data.active_id = Some("a".into());

        data.remove("a").unwrap();
        assert!(data.active_id.is_none());
        assert!(data.profiles.is_empty());
    }

    #[test]
    fn remove_preserves_active_when_removing_inactive() {
        let mut data = ProfilesData::default();
        data.profiles.push(profile_with_id("a"));
        data.profiles.push(profile_with_id("b"));
        data.active_id = Some("a".into());

        data.remove("b").unwrap();
        assert_eq!(data.active_id.as_deref(), Some("a"));
    }

    #[test]
    fn remove_rejects_unknown_id() {
        let mut data = ProfilesData::default();
        data.profiles.push(profile_with_id("a"));
        assert!(data.remove("ghost").is_err());
    }

    #[tokio::test]
    async fn migrate_returns_empty_when_no_legacy_files() {
        let dir = tempfile::tempdir().unwrap();
        let data = migrate_legacy(dir.path()).await.unwrap();
        assert!(data.profiles.is_empty());
        assert!(data.active_id.is_none());
    }

    #[tokio::test]
    async fn migrate_creates_default_from_repo_only() {
        let dir = tempfile::tempdir().unwrap();
        let repo_json = serde_json::to_string(&sample_repo()).unwrap();
        tokio::fs::write(dir.path().join("repo.json"), repo_json)
            .await
            .unwrap();

        let data = migrate_legacy(dir.path()).await.unwrap();
        assert_eq!(data.profiles.len(), 1);
        assert_eq!(data.active_id.as_deref(), Some("default"));
        assert_eq!(data.profiles[0].name, "Default");
        assert!(data.profiles[0].schedule.is_none());
        assert!(data.profiles[0].retention.is_none());
    }

    #[tokio::test]
    async fn migrate_tolerates_corrupt_schedule_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo_json = serde_json::to_string(&sample_repo()).unwrap();
        tokio::fs::write(dir.path().join("repo.json"), repo_json)
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("schedule.json"), "{ not json }")
            .await
            .unwrap();

        let data = migrate_legacy(dir.path()).await.unwrap();
        assert_eq!(data.profiles.len(), 1);
        assert!(data.profiles[0].schedule.is_none());
    }

    #[tokio::test]
    async fn unversioned_profiles_are_migrated_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "profiles": [profile_with_id("work")],
            "active_id": "work"
        });
        tokio::fs::write(
            dir.path().join("profiles.json"),
            serde_json::to_vec_pretty(&json).unwrap(),
        )
        .await
        .unwrap();

        let data = load(dir.path()).await.unwrap();
        assert_eq!(data.schema_version, PROFILE_SCHEMA_VERSION);
        let saved: serde_json::Value = serde_json::from_slice(
            &tokio::fs::read(dir.path().join("profiles.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved["schema_version"],
            serde_json::Value::from(PROFILE_SCHEMA_VERSION)
        );
    }

    #[tokio::test]
    async fn future_schema_is_rejected_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let original = format!(
            r#"{{"schema_version":{},"profiles":[],"active_id":null}}"#,
            PROFILE_SCHEMA_VERSION + 1
        );
        tokio::fs::write(&path, &original).await.unwrap();

        assert!(load(dir.path()).await.unwrap_err().contains("newer"));
        assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), original);
    }
}
