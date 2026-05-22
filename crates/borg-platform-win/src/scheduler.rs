use serde::{Deserialize, Serialize};

use borg_core::error::{BorgError, Result};

const FORBIDDEN_CHARS: &[char] = &[
    '&', '|', ';', '`', '$', '(', ')', '{', '}', '<', '>', '"', '\'', '\n', '\r',
];

fn validate_schtasks_input(value: &str, field_name: &str) -> Result<()> {
    if value.is_empty() {
        return Err(BorgError::InvalidConfig {
            message: format!("{} cannot be empty", field_name),
        });
    }
    if value.chars().any(|c| FORBIDDEN_CHARS.contains(&c)) {
        return Err(BorgError::InvalidConfig {
            message: format!("{} contains forbidden characters", field_name),
        });
    }
    Ok(())
}

pub async fn schedule_backup(
    task_name: &str,
    exe_path: &str,
    args: &str,
    schedule: &Schedule,
) -> Result<()> {
    validate_schtasks_input(task_name, "task_name")?;
    validate_schtasks_input(exe_path, "exe_path")?;
    validate_schtasks_input(args, "args")?;
    schedule.validate()?;

    let mut cmd = tokio::process::Command::new("schtasks");
    cmd.args(["/Create", "/F"])
        .args(["/TN", task_name])
        .args(["/TR", &format!("\"{}\" {}", exe_path, args)]);

    match schedule {
        Schedule::Hourly => {
            cmd.args(["/SC", "HOURLY"]).args(["/MO", "1"]);
        }
        Schedule::Daily { hour, minute } => {
            cmd.args(["/SC", "DAILY"]).args(["/MO", "1"]);
            cmd.args(["/ST", &format!("{:02}:{:02}", hour, minute)]);
        }
    }

    let output = cmd.output().await?;
    if !output.status.success() {
        return Err(BorgError::ProcessFailed {
            message: "schtasks failed".into(),
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    Ok(())
}

pub async fn unschedule_backup(task_name: &str) -> Result<()> {
    validate_schtasks_input(task_name, "task_name")?;

    let output = tokio::process::Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", task_name])
        .output()
        .await?;

    if !output.status.success() {
        return Err(BorgError::ProcessFailed {
            message: "schtasks delete failed".into(),
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Schedule {
    #[serde(rename = "hourly")]
    Hourly,
    #[serde(rename = "daily")]
    Daily { hour: u8, minute: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub enabled: bool,
    pub source_paths: Vec<String>,
    pub schedule: Schedule,
    #[serde(default)]
    pub excludes: Vec<String>,
}

impl Schedule {
    pub fn validate(&self) -> Result<()> {
        if let Schedule::Daily { hour, minute } = self
            && (*hour >= 24 || *minute >= 60)
        {
            return Err(BorgError::InvalidConfig {
                message: format!("invalid schedule time {:02}:{:02}", hour, minute),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_hourly_variant() {
        let schedule = Schedule::Hourly;
        assert!(matches!(schedule, Schedule::Hourly));
    }

    #[test]
    fn schedule_daily_variant() {
        let schedule = Schedule::Daily {
            hour: 14,
            minute: 30,
        };
        match schedule {
            Schedule::Daily { hour, minute } => {
                assert_eq!(hour, 14);
                assert_eq!(minute, 30);
            }
            _ => panic!("expected Daily"),
        }
    }

    #[test]
    fn rejects_injection_in_task_name() {
        let result = validate_schtasks_input("test & del /s", "task_name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("forbidden"));
    }

    #[test]
    fn rejects_pipe_in_exe_path() {
        let result = validate_schtasks_input("notepad.exe | evil", "exe_path");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_input() {
        let result = validate_schtasks_input("", "task_name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn accepts_valid_task_name() {
        assert!(validate_schtasks_input("BorgUI-Daily-Backup", "task_name").is_ok());
    }

    #[test]
    fn accepts_valid_exe_path() {
        assert!(validate_schtasks_input(r"C:\Program Files\BorgUI\borg.exe", "exe_path").is_ok());
    }

    #[test]
    fn rejects_hour_24() {
        let schedule = Schedule::Daily {
            hour: 24,
            minute: 0,
        };
        assert!(schedule.validate().is_err());
    }

    #[test]
    fn rejects_minute_60() {
        let schedule = Schedule::Daily {
            hour: 0,
            minute: 60,
        };
        assert!(schedule.validate().is_err());
    }

    #[test]
    fn accepts_hour_23_minute_59() {
        let schedule = Schedule::Daily {
            hour: 23,
            minute: 59,
        };
        assert!(schedule.validate().is_ok());
    }

    #[test]
    fn hourly_validates_ok() {
        assert!(Schedule::Hourly.validate().is_ok());
    }
}
