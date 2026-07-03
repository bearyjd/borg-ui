use serde::Serialize;

use crate::profiles::BackupSelection;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResolvedTemplate {
    pub id: String,
    pub version: u32,
    pub name: String,
    pub explanation: String,
    pub source_paths: Vec<String>,
    pub unavailable_folders: Vec<String>,
    pub excludes: Vec<String>,
    pub suggested_schedule: String,
}

struct Template {
    id: &'static str,
    version: u32,
    name: &'static str,
    explanation: &'static str,
    folders: &'static [&'static str],
    excludes: &'static [&'static str],
    schedule: &'static str,
}

const TEMPLATES: &[Template] = &[
    Template {
        id: "personal-files",
        version: 1,
        name: "Personal Files",
        explanation: "Documents, desktop, pictures, videos, music, and downloads.",
        folders: &[
            "documents",
            "desktop",
            "pictures",
            "videos",
            "music",
            "downloads",
        ],
        excludes: &["sh:**/*.tmp", "sh:**/Thumbs.db"],
        schedule: "Daily at 20:00",
    },
    Template {
        id: "entire-user-profile",
        version: 1,
        name: "Entire User Profile",
        explanation: "The current Windows user profile, excluding caches and temporary data.",
        folders: &["home"],
        excludes: &[
            "sh:**/AppData/Local/Temp/**",
            "sh:**/AppData/Local/Microsoft/Windows/INetCache/**",
        ],
        schedule: "Daily at 20:00",
    },
    Template {
        id: "developer-workstation",
        version: 1,
        name: "Developer Workstation",
        explanation: "Documents and common source-code roots without reproducible build outputs.",
        folders: &["documents", "desktop"],
        excludes: &[
            "sh:**/.git/**",
            "sh:**/node_modules/**",
            "sh:**/target/**",
            "sh:**/.venv/**",
            "sh:**/dist/**",
            "sh:**/build/**",
        ],
        schedule: "Hourly",
    },
    Template {
        id: "photography",
        version: 1,
        name: "Photography",
        explanation: "Pictures and photo catalogs while excluding generated previews.",
        folders: &["pictures"],
        excludes: &[
            "sh:**/Previews.lrdata/**",
            "sh:**/Smart Previews.lrdata/**",
            "sh:**/Thumbs.db",
        ],
        schedule: "Daily at 22:00",
    },
    Template {
        id: "outlook",
        version: 1,
        name: "Outlook",
        explanation: "Outlook data under Documents and the current roaming application-data folder.",
        folders: &["documents", "data"],
        excludes: &[
            "sh:**/AppData/Local/Microsoft/Outlook/*.ost",
            "sh:**/AppData/Local/Temp/**",
        ],
        schedule: "Daily at 19:00",
    },
];

pub fn list() -> Vec<ResolvedTemplate> {
    TEMPLATES.iter().map(resolve).collect()
}

pub fn apply(id: &str) -> Result<BackupSelection, String> {
    let template = TEMPLATES
        .iter()
        .find(|template| template.id == id)
        .ok_or_else(|| "unknown profile template".to_string())?;
    let resolved = resolve(template);
    if resolved.source_paths.is_empty() {
        return Err("none of this template's Windows folders are available".into());
    }
    Ok(BackupSelection {
        source_paths: resolved.source_paths,
        excludes: resolved.excludes,
        template_id: Some(resolved.id),
        template_version: Some(resolved.version),
    })
}

fn resolve(template: &Template) -> ResolvedTemplate {
    let mut source_paths = Vec::new();
    let mut unavailable_folders = Vec::new();
    for id in template.folders {
        let path = resolve_folder(id);
        match path.filter(|path| path.is_dir()) {
            Some(path) => source_paths.push(path.to_string_lossy().into_owned()),
            None => unavailable_folders.push((*id).to_string()),
        }
    }
    source_paths.sort();
    source_paths.dedup();
    ResolvedTemplate {
        id: template.id.into(),
        version: template.version,
        name: template.name.into(),
        explanation: template.explanation.into(),
        source_paths,
        unavailable_folders,
        excludes: template
            .excludes
            .iter()
            .map(|value| (*value).into())
            .collect(),
        suggested_schedule: template.schedule.into(),
    }
}

fn resolve_folder(id: &str) -> Option<std::path::PathBuf> {
    match id {
        "home" => dirs::home_dir(),
        "desktop" => dirs::desktop_dir(),
        "documents" => dirs::document_dir(),
        "pictures" => dirs::picture_dir(),
        "videos" => dirs::video_dir(),
        "music" => dirs::audio_dir(),
        "downloads" => dirs::download_dir(),
        "data" => dirs::data_dir(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_are_versioned_and_machine_path_free() {
        assert_eq!(TEMPLATES.len(), 5);
        for template in TEMPLATES {
            assert!(template.version > 0);
            assert!(
                template
                    .folders
                    .iter()
                    .all(|folder| !folder.contains(['\\', '/']))
            );
        }
    }

    #[test]
    fn applying_preserves_template_metadata() {
        if dirs::home_dir().is_some_and(|path| path.is_dir()) {
            let selection = apply("entire-user-profile").unwrap();
            assert_eq!(
                selection.template_id.as_deref(),
                Some("entire-user-profile")
            );
            assert_eq!(selection.template_version, Some(1));
        }
    }
}
