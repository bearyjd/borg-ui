use std::path::{Path, PathBuf};

use borg_core::borg::CancelToken;

use crate::profiles::{PlaceholderMode, PlaceholderPolicy};

#[derive(Debug, Default)]
pub struct PlaceholderPlan {
    pub exclusions: Vec<String>,
    pub count: usize,
}

pub async fn prepare(
    roots: &[PathBuf],
    policy: &PlaceholderPolicy,
    cancel: &CancelToken,
) -> Result<PlaceholderPlan, String> {
    let roots = roots.to_vec();
    let scan_cancel = cancel.clone();
    let placeholders = tokio::task::spawn_blocking(move || {
        borg_platform_win::cloud_files::find_placeholders(&roots, &scan_cancel)
    })
    .await
    .map_err(|error| error.to_string())??;
    if placeholders.is_empty() {
        return Ok(PlaceholderPlan::default());
    }
    match policy.mode {
        PlaceholderMode::Fail => Err(format!(
            "{} cloud placeholders require review before backup",
            placeholders.len()
        )),
        PlaceholderMode::WarnAndSkip => Ok(PlaceholderPlan {
            count: placeholders.len(),
            exclusions: placeholders
                .iter()
                .map(|file| format!("pf:{}", archive_path(&file.path)))
                .collect(),
        }),
        PlaceholderMode::Materialize => {
            let count = placeholders.len();
            let reserve = policy.minimum_free_space_reserve;
            let hydrate_cancel = cancel.clone();
            tokio::task::spawn_blocking(move || {
                for file in &placeholders {
                    let free = borg_platform_win::cloud_files::free_space(
                        file.path.parent().unwrap_or(&file.path),
                    )?;
                    if !has_space(free, reserve, file.size) {
                        return Err(
                            "insufficient free space to materialize cloud placeholders".into()
                        );
                    }
                    borg_platform_win::cloud_files::hydrate(file, &hydrate_cancel)?;
                }
                Ok::<(), String>(())
            })
            .await
            .map_err(|error| error.to_string())??;
            Ok(PlaceholderPlan {
                count,
                exclusions: Vec::new(),
            })
        }
    }
}

fn archive_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let without_drive = if normalized
        .as_bytes()
        .get(1)
        .is_some_and(|value| *value == b':')
    {
        &normalized[2..]
    } else {
        &normalized
    };
    without_drive.trim_start_matches('/').to_owned()
}

fn has_space(available: u64, reserve: u64, hydration_size: u64) -> bool {
    available.saturating_sub(hydration_size) >= reserve
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_boundary_preserves_reserve() {
        assert!(has_space(110, 100, 10));
        assert!(!has_space(109, 100, 10));
    }

    #[test]
    fn temporary_exclusion_is_exact_and_machine_prefix_free() {
        let path = archive_path(Path::new(r"C:\Users\Alice\OneDrive\online.txt"));
        assert_eq!(path, "Users/Alice/OneDrive/online.txt");
        assert!(!format!("pf:{path}").contains("password"));
    }
}
