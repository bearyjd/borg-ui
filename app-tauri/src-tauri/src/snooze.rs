use std::path::Path;

use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnoozeState {
    pub until: Option<String>,
    pub indefinite: bool,
}

impl SnoozeState {
    pub fn active(&self, now: DateTime<Utc>) -> bool {
        self.indefinite
            || self
                .until
                .as_deref()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .is_some_and(|until| until.with_timezone(&Utc) > now)
    }
}

pub async fn load(config_dir: &Path) -> Result<Option<SnoozeState>, String> {
    match tokio::fs::read(config_dir.join("snooze.json")).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub async fn save(config_dir: &Path, choice: &str) -> Result<SnoozeState, String> {
    let now = Utc::now();
    let state = match choice {
        "one_hour" => timed(now + Duration::hours(1)),
        "four_hours" => timed(now + Duration::hours(4)),
        "tomorrow" => {
            let tomorrow = Local::now().date_naive() + Duration::days(1);
            let local = Local
                .from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).unwrap())
                .earliest()
                .ok_or_else(|| "could not resolve tomorrow in the local timezone".to_string())?;
            timed(local.with_timezone(&Utc))
        }
        "indefinite" => SnoozeState {
            until: None,
            indefinite: true,
        },
        "clear" => SnoozeState {
            until: None,
            indefinite: false,
        },
        _ => return Err("unknown snooze choice".into()),
    };
    tokio::fs::create_dir_all(config_dir)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::write(
        config_dir.join("snooze.json"),
        serde_json::to_vec_pretty(&state).map_err(|error| error.to_string())?,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(state)
}

fn timed(until: DateTime<Utc>) -> SnoozeState {
    SnoozeState {
        until: Some(until.to_rfc3339()),
        indefinite: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_respects_expiry_and_indefinite() {
        let now = Utc::now();
        assert!(timed(now + Duration::minutes(1)).active(now));
        assert!(!timed(now - Duration::minutes(1)).active(now));
        assert!(
            SnoozeState {
                until: None,
                indefinite: true
            }
            .active(now)
        );
    }
}
