use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_EVENTS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEvent {
    pub id: String,
    pub timestamp: String,
    pub kind: String,
    pub archive_name: String,
    pub outcome: String,
    pub duration_seconds: u64,
    #[serde(default)]
    pub file_count: Option<u64>,
    #[serde(default)]
    pub original_size: Option<u64>,
    #[serde(default)]
    pub error_message: Option<String>,
}

pub async fn load(path: &Path) -> Result<Vec<BackupEvent>, String> {
    match tokio::fs::read_to_string(path).await {
        Ok(data) => serde_json::from_str(&data).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn append(path: &Path, event: BackupEvent) -> Result<(), String> {
    let mut events = load(path).await?;
    events.push(event);
    if events.len() > MAX_EVENTS {
        let excess = events.len() - MAX_EVENTS;
        events.drain(0..excess);
    }
    write_atomic(path, &events).await
}

pub async fn clear(path: &Path) -> Result<(), String> {
    write_atomic(path, &Vec::<BackupEvent>::new()).await
}

async fn write_atomic(path: &Path, events: &[BackupEvent]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(events).map_err(|e| e.to_string())?;
    tokio::fs::write(&tmp, &data)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| e.to_string())
}
