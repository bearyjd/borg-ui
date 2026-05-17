use borg_core::error::{BorgError, Result};

/// Schedules a backup task via Windows Task Scheduler.
///
/// Uses schtasks.exe CLI for simplicity (not COM API).
pub async fn schedule_backup(
    task_name: &str,
    exe_path: &str,
    args: &str,
    schedule: &Schedule,
) -> Result<()> {
    let (sc, mo) = match schedule {
        Schedule::Hourly => ("HOURLY", "1"),
        Schedule::Daily { hour, minute } => {
            // schtasks uses /ST for start time
            let _ = (hour, minute);
            ("DAILY", "1")
        }
    };

    let mut cmd = tokio::process::Command::new("schtasks");
    cmd.args(["/Create", "/F"])
        .args(["/TN", task_name])
        .args(["/TR", &format!("{} {}", exe_path, args)])
        .args(["/SC", sc])
        .args(["/MO", mo]);

    if let Schedule::Daily { hour, minute } = schedule {
        cmd.args(["/ST", &format!("{:02}:{:02}", hour, minute)]);
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

pub enum Schedule {
    Hourly,
    Daily { hour: u8, minute: u8 },
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
        let schedule = Schedule::Daily { hour: 14, minute: 30 };
        match schedule {
            Schedule::Daily { hour, minute } => {
                assert_eq!(hour, 14);
                assert_eq!(minute, 30);
            }
            _ => panic!("expected Daily"),
        }
    }
}
