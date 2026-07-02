use std::path::Path;

use borg_core::borg::CancelToken;
use serde::Serialize;

pub const STANDARD_EXCLUDES: &[&str] = &[
    "sh:**/$RECYCLE.BIN/**",
    "sh:**/System Volume Information/**",
    "sh:**/AppData/Local/Temp/**",
    "sh:**/node_modules/**",
];

#[derive(Debug, Clone, Serialize)]
pub struct KnownFolder {
    pub id: &'static str,
    pub label: &'static str,
    pub path: String,
    pub available: bool,
    pub preselected: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SourceEstimate {
    pub path: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub access_errors: u64,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageScan {
    pub sources: Vec<SourceEstimate>,
    pub duplicate_roots: Vec<String>,
    pub cancelled: bool,
    pub needs_review: bool,
}

/// Resolve user-facing folders at discovery time. `dirs` uses
/// `SHGetKnownFolderPath` on Windows, so redirected Known Folders are preserved.
pub fn discover_known_folders() -> Vec<KnownFolder> {
    [
        ("desktop", "Desktop", dirs::desktop_dir(), true),
        ("documents", "Documents", dirs::document_dir(), true),
        ("pictures", "Pictures", dirs::picture_dir(), true),
        ("videos", "Videos", dirs::video_dir(), true),
        ("music", "Music", dirs::audio_dir(), false),
        ("downloads", "Downloads", dirs::download_dir(), false),
    ]
    .into_iter()
    .map(|(id, label, path, preselected)| {
        let path = path.unwrap_or_default();
        KnownFolder {
            id,
            label,
            available: path.is_dir(),
            path: path.to_string_lossy().into_owned(),
            preselected,
        }
    })
    .collect()
}

pub fn scan_sources(paths: Vec<String>, cancel: &CancelToken) -> CoverageScan {
    let normalized: Vec<String> = paths.iter().map(|path| normalize(path)).collect();
    let mut duplicate_roots = Vec::new();
    let mut unique = Vec::new();
    for (index, raw) in paths.into_iter().enumerate() {
        let duplicate = normalized.iter().enumerate().any(|(other_index, other)| {
            other_index != index
                && (is_nested(&normalized[index], other)
                    || (normalized[index] == *other && other_index < index))
        });
        if duplicate {
            duplicate_roots.push(raw);
        } else {
            unique.push(raw);
        }
    }

    let mut sources = Vec::with_capacity(unique.len());
    for path in unique {
        let mut estimate = SourceEstimate {
            path: path.clone(),
            available: Path::new(&path).is_dir(),
            ..SourceEstimate::default()
        };
        if estimate.available {
            scan_dir(Path::new(&path), &mut estimate, cancel);
        }
        sources.push(estimate);
        if cancel.is_cancelled() {
            break;
        }
    }
    let cancelled = cancel.is_cancelled();
    let needs_review = !duplicate_roots.is_empty()
        || sources
            .iter()
            .any(|source| !source.available || source.access_errors > 0);
    CoverageScan {
        sources,
        duplicate_roots,
        cancelled,
        needs_review,
    }
}

fn scan_dir(path: &Path, estimate: &mut SourceEstimate, cancel: &CancelToken) {
    if cancel.is_cancelled() {
        return;
    }
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            estimate.access_errors += 1;
            return;
        }
    };
    for entry in entries {
        if cancel.is_cancelled() {
            return;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                estimate.access_errors += 1;
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => {
                estimate.access_errors += 1;
                continue;
            }
        };
        if file_type.is_symlink() {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                estimate.access_errors += 1;
                continue;
            }
        };
        if metadata.is_file() {
            estimate.file_count = estimate.file_count.saturating_add(1);
            estimate.total_bytes = estimate.total_bytes.saturating_add(metadata.len());
        } else if metadata.is_dir() {
            scan_dir(&entry.path(), estimate, cancel);
        }
    }
}

fn normalize(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .replace('\\', "/")
        .to_lowercase()
}

fn is_nested(candidate: &str, root: &str) -> bool {
    candidate
        .strip_prefix(root)
        .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_duplicate_and_nested_roots() {
        let result = scan_sources(
            vec!["C:\\Users\\me".into(), "c:/users/me/Documents".into()],
            &CancelToken::new(),
        );
        assert_eq!(result.duplicate_roots, vec!["c:/users/me/Documents"]);
    }

    #[test]
    fn estimates_files_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("child")).unwrap();
        std::fs::write(dir.path().join("child").join("a"), b"1234").unwrap();
        let result = scan_sources(
            vec![dir.path().to_string_lossy().into_owned()],
            &CancelToken::new(),
        );
        assert_eq!(result.sources[0].file_count, 1);
        assert_eq!(result.sources[0].total_bytes, 4);
    }

    #[test]
    fn cancellation_stops_scan() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = scan_sources(vec!["C:\\".into()], &cancel);
        assert!(result.cancelled);
    }

    #[test]
    fn missing_source_is_reported_as_a_coverage_gap() {
        let result = scan_sources(
            vec!["Z:\\definitely-not-connected".into()],
            &CancelToken::new(),
        );
        assert!(!result.sources[0].available);
        assert!(result.needs_review);
    }
}
