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
    wake_to_run: bool,
) -> Result<()> {
    validate_schtasks_input(task_name, "task_name")?;
    validate_schtasks_input(exe_path, "exe_path")?;
    validate_schtasks_input(args, "args")?;
    schedule.validate()?;

    // Always register via an XML task definition rather than the `/SC ...`
    // flag shorthand. The flag-based form cannot express
    // <StartWhenAvailable>, which is what makes Task Scheduler run a backup
    // that was missed because the PC was asleep or off as soon as it becomes
    // available again, instead of silently waiting for the next slot.
    let xml = task_xml(exe_path, args, schedule, wake_to_run);
    let path = std::env::temp_dir().join(format!(
        "borgui-task-{}-{}.xml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    tokio::fs::write(&path, xml).await?;
    let output = tokio::process::Command::new("schtasks")
        .args(["/Create", "/F", "/TN", task_name, "/XML"])
        .arg(&path)
        .output()
        .await;
    let _ = tokio::fs::remove_file(path).await;
    let output = output?;
    if output.status.success() {
        return Ok(());
    }
    Err(BorgError::ProcessFailed {
        message: "schtasks XML registration failed".into(),
        exit_code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into(),
    })
}

fn task_xml(exe_path: &str, args: &str, schedule: &Schedule, wake_to_run: bool) -> String {
    let trigger = match schedule {
        Schedule::Hourly => {
            // Anchor to the current local time, matching the plain
            // `schtasks /SC HOURLY /MO 1` default (no /ST): the task fires
            // every hour offset by whatever minute:second it's registered at,
            // not aligned to the top of the hour. The frontend depends on
            // this exact semantic -- see the `nextRun` doc comment in
            // app-tauri/src/lib/stores/schedule.svelte.ts, which can't
            // predict the exact minute and shows "Every hour" instead of a
            // clock time for that reason.
            let start = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S");
            format!(
                "<TimeTrigger><StartBoundary>{start}</StartBoundary><Repetition><Interval>PT1H</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition><Enabled>true</Enabled></TimeTrigger>"
            )
        }
        Schedule::Daily { hour, minute } => format!(
            "<CalendarTrigger><StartBoundary>2026-01-01T{hour:02}:{minute:02}:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay><Enabled>true</Enabled></CalendarTrigger>"
        ),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\
<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\
<Triggers>{trigger}</Triggers>\
<Principals><Principal id=\"Author\"><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\
<Settings><StartWhenAvailable>true</StartWhenAvailable><WakeToRun>{wake_to_run}</WakeToRun><ExecutionTimeLimit>PT0S</ExecutionTimeLimit></Settings>\
<Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions>\
</Task>",
        xml_escape(exe_path),
        xml_escape(args),
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Install the separate, opt-in monthly metadata-only integrity task.
pub async fn schedule_monthly_check(task_name: &str, exe_path: &str, args: &str) -> Result<()> {
    validate_schtasks_input(task_name, "task_name")?;
    validate_schtasks_input(exe_path, "exe_path")?;
    validate_schtasks_input(args, "args")?;

    let output = tokio::process::Command::new("schtasks")
        .args(["/Create", "/F"])
        .args(["/TN", task_name])
        .args(["/TR", &format!("\"{}\" {}", exe_path, args)])
        .args(["/SC", "MONTHLY", "/D", "1", "/ST", "03:00"])
        .output()
        .await?;
    if !output.status.success() {
        return Err(BorgError::ProcessFailed {
            message: "schtasks integrity-check scheduling failed".into(),
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

pub async fn task_exists(task_name: &str) -> Result<bool> {
    validate_schtasks_input(task_name, "task_name")?;
    let output = tokio::process::Command::new("schtasks")
        .args(["/Query", "/TN", task_name])
        .output()
        .await?;
    Ok(output.status.success())
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
    pub schedule: Schedule,
    /// When true, unattended scheduled backups skip the run while Windows reports
    /// the active network as metered. Manual backups are unaffected.
    #[serde(default)]
    pub skip_metered_networks: bool,
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

    #[test]
    fn wake_task_xml_enables_wake_and_escapes_action() {
        let xml = task_xml(
            r"C:\Program Files\Borg & UI\borg-ui.exe",
            "--scheduled-backup",
            &Schedule::Daily { hour: 2, minute: 5 },
            true,
        );
        assert!(xml.contains("<WakeToRun>true</WakeToRun>"));
        assert!(xml.contains("T02:05:00"));
        assert!(xml.contains("Borg &amp; UI"));
    }

    #[test]
    fn task_xml_enables_catch_up_for_missed_runs() {
        // <StartWhenAvailable> makes the Task Scheduler run a missed backup as
        // soon as the machine becomes available (was asleep/off at trigger
        // time). This must hold for every registration -- wake-to-run and
        // plain alike, since `schedule_backup` now always goes through XML.
        for wake_to_run in [true, false] {
            for schedule in [Schedule::Hourly, Schedule::Daily { hour: 2, minute: 5 }] {
                let xml = task_xml(
                    r"C:\Program Files\BorgUI\borg-ui.exe",
                    "--scheduled-backup",
                    &schedule,
                    wake_to_run,
                );
                assert!(xml.contains("<StartWhenAvailable>true</StartWhenAvailable>"));
                assert!(xml.contains(&format!("<WakeToRun>{wake_to_run}</WakeToRun>")));
                // Schema order within <Settings>: StartWhenAvailable precedes WakeToRun.
                let start_pos = xml.find("<StartWhenAvailable>").unwrap();
                let settings_pos = xml.find("<Settings>").unwrap();
                let wake_pos = xml.find("<WakeToRun>").unwrap();
                assert!(settings_pos < start_pos && start_pos < wake_pos);
            }
        }
    }

    #[test]
    fn non_wake_task_xml_preserves_trigger_semantics() {
        // Registering without wake-to-run must still preserve the same
        // trigger behavior the old flag-based `/SC ...` path had: an exact
        // daily time, and an hourly repetition anchored to "now" (matching
        // `/SC HOURLY /MO 1` with no `/ST`, which starts from the moment the
        // task is created rather than the top of the hour).
        let daily_xml = task_xml(
            r"C:\Program Files\BorgUI\borg-ui.exe",
            "--scheduled-backup",
            &Schedule::Daily { hour: 2, minute: 5 },
            false,
        );
        assert!(daily_xml.contains("<CalendarTrigger>"));
        assert!(daily_xml.contains("T02:05:00"));
        assert!(daily_xml.contains("<WakeToRun>false</WakeToRun>"));

        let hourly_xml = task_xml(
            r"C:\Program Files\BorgUI\borg-ui.exe",
            "--scheduled-backup",
            &Schedule::Hourly,
            false,
        );
        assert!(hourly_xml.contains("<TimeTrigger>"));
        assert!(hourly_xml.contains("<Interval>PT1H</Interval>"));
        assert!(hourly_xml.contains("<WakeToRun>false</WakeToRun>"));

        // StartBoundary should be anchored to "now" (small tolerance for test
        // execution time), not a fixed date, so the hourly repeat lands on
        // the same minute:second as when the task was registered.
        let start = hourly_xml
            .split("<StartBoundary>")
            .nth(1)
            .and_then(|s| s.split("</StartBoundary>").next())
            .expect("StartBoundary present");
        let parsed = chrono::NaiveDateTime::parse_from_str(start, "%Y-%m-%dT%H:%M:%S")
            .expect("StartBoundary is a valid local datetime");
        let now = chrono::Local::now().naive_local();
        let diff = (now - parsed).num_seconds().abs();
        assert!(
            diff < 10,
            "StartBoundary {start} not close to now (diff {diff}s)"
        );
    }

    #[test]
    fn schedule_config_defaults_metered_skip_to_false() {
        let json = r#"{
            "enabled": true,
            "source_paths": ["C:\\Users\\me"],
            "schedule": { "type": "hourly" },
            "excludes": []
        }"#;
        let config: ScheduleConfig = serde_json::from_str(json).unwrap();
        assert!(!config.skip_metered_networks);
    }
}
