use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

const MAX_EVENTS: usize = 200;
const DATABASE_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

pub async fn initialize(config_dir: &Path) -> Result<(), String> {
    let dir = config_dir.to_path_buf();
    tokio::task::spawn_blocking(move || initialize_sync(&dir))
        .await
        .map_err(|e| e.to_string())?
}

pub async fn load(config_dir: &Path) -> Result<Vec<BackupEvent>, String> {
    initialize(config_dir).await?;
    let dir = config_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = open(&dir)?;
        read_events(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn append(config_dir: &Path, event: BackupEvent) -> Result<(), String> {
    initialize(config_dir).await?;
    let dir = config_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut conn = open(&dir)?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        insert_event(&tx, &event)?;
        tx.execute(
            "DELETE FROM operation_history WHERE rowid NOT IN (
                SELECT rowid FROM operation_history ORDER BY sequence DESC LIMIT ?1
            )",
            [MAX_EVENTS],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn clear(config_dir: &Path) -> Result<(), String> {
    initialize(config_dir).await?;
    let dir = config_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = open(&dir)?;
        conn.execute("DELETE FROM operation_history", [])
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn initialize_sync(config_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let mut conn = open(config_dir)?;
    let migrated: Option<String> = conn
        .query_row(
            "SELECT value FROM schema_metadata WHERE key = 'history_json_migrated'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if migrated.is_some() {
        return Ok(());
    }

    let legacy_path = config_dir.join("history.json");
    if !legacy_path.exists() {
        conn.execute(
            "INSERT INTO schema_metadata(key, value) VALUES ('history_json_migrated', 'absent')",
            [],
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let json = std::fs::read_to_string(&legacy_path).map_err(|e| e.to_string())?;
    let events: Vec<BackupEvent> =
        serde_json::from_str(&json).map_err(|e| format!("invalid legacy history.json: {e}"))?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for event in events.iter().rev().take(MAX_EVENTS).rev() {
        insert_event(&tx, event)?;
    }
    tx.execute(
        "INSERT INTO schema_metadata(key, value) VALUES ('history_json_migrated', 'complete')",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn open(config_dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let conn = Connection::open(database_path(config_dir)).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS schema_metadata (
             key TEXT PRIMARY KEY NOT NULL,
             value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS operation_history (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             id TEXT NOT NULL,
             timestamp TEXT NOT NULL,
             kind TEXT NOT NULL,
             archive_name TEXT NOT NULL,
             outcome TEXT NOT NULL,
             duration_seconds INTEGER NOT NULL,
             file_count INTEGER,
             original_size INTEGER,
             error_message TEXT
         );",
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO schema_metadata(key, value) VALUES ('database_schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [DATABASE_SCHEMA_VERSION.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn insert_event(conn: &Connection, event: &BackupEvent) -> Result<(), String> {
    conn.execute(
        "INSERT INTO operation_history (
            id, timestamp, kind, archive_name, outcome, duration_seconds,
            file_count, original_size, error_message
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            event.id,
            event.timestamp,
            event.kind,
            event.archive_name,
            event.outcome,
            event.duration_seconds,
            event.file_count,
            event.original_size,
            event.error_message,
        ],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn read_events(conn: &Connection) -> Result<Vec<BackupEvent>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, timestamp, kind, archive_name, outcome, duration_seconds,
                    file_count, original_size, error_message
             FROM operation_history ORDER BY sequence ASC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([MAX_EVENTS], |row| {
            Ok(BackupEvent {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                kind: row.get(2)?,
                archive_name: row.get(3)?,
                outcome: row.get(4)?,
                duration_seconds: row.get(5)?,
                file_count: row.get(6)?,
                original_size: row.get(7)?,
                error_message: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn database_path(config_dir: &Path) -> PathBuf {
    config_dir.join("borgui.sqlite3")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(n: usize) -> BackupEvent {
        BackupEvent {
            id: format!("id-{n}"),
            timestamp: format!("2026-01-01T00:{n:02}:00Z"),
            kind: "backup".into(),
            archive_name: format!("archive-{n}"),
            outcome: "success".into(),
            duration_seconds: n as u64,
            file_count: Some(n as u64),
            original_size: None,
            error_message: None,
        }
    }

    #[tokio::test]
    async fn migrates_legacy_once_and_retains_source() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("history.json"),
            serde_json::to_vec(&vec![event(1), event(2)]).unwrap(),
        )
        .unwrap();
        initialize(dir.path()).await.unwrap();
        initialize(dir.path()).await.unwrap();
        assert_eq!(load(dir.path()).await.unwrap(), vec![event(1), event(2)]);
        assert!(dir.path().join("history.json").exists());
    }

    #[tokio::test]
    async fn keeps_latest_two_hundred_events() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..205 {
            append(dir.path(), event(n)).await.unwrap();
        }
        let events = load(dir.path()).await.unwrap();
        assert_eq!(events.len(), MAX_EVENTS);
        assert_eq!(events.first().unwrap().id, "id-5");
        assert_eq!(events.last().unwrap().id, "id-204");
    }

    #[tokio::test]
    async fn corrupt_legacy_file_is_not_modified_or_marked_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(initialize(dir.path()).await.is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "not json");
    }
}
