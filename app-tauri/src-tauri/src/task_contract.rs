//! Single source of truth for the headless task names and CLI flags shared
//! between the Task Scheduler writers (`commands::*`) and the `lib.rs` parser.
//! Values are persisted in existing Windows scheduled tasks: do not change them.

pub const BACKUP_TASK: &str = "BorgUI-Backup";
pub const INTEGRITY_TASK: &str = "BorgUI-Integrity-Check";
pub const RESTORE_DRILL_TASK: &str = "BorgUI-Monthly-Restore-Drill";
pub const HEALTH_REPORT_TASK: &str = "BorgUI-Daily-Health-Report";

pub const SCHEDULED_BACKUP_FLAG: &str = "--scheduled-backup";
pub const SCHEDULED_INTEGRITY_FLAG: &str = "--scheduled-integrity-check";
pub const SCHEDULED_RESTORE_DRILL_FLAG: &str = "--scheduled-restore-drill";
pub const SCHEDULED_HEALTH_REPORT_FLAG: &str = "--scheduled-health-report";
/// Also written by `borg-platform-win` autostart (separate crate, kept in sync
/// by a test there and here).
pub const START_MINIMIZED_FLAG: &str = "--minimized";

/// Returns true when `flag` appears verbatim in `args`.
pub fn has_flag<I, S>(args: I, flag: &str) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|a| a.as_ref() == flag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_are_unchanged() {
        assert_eq!(BACKUP_TASK, "BorgUI-Backup");
        assert_eq!(INTEGRITY_TASK, "BorgUI-Integrity-Check");
        assert_eq!(RESTORE_DRILL_TASK, "BorgUI-Monthly-Restore-Drill");
        assert_eq!(HEALTH_REPORT_TASK, "BorgUI-Daily-Health-Report");
    }

    #[test]
    fn flag_values_are_unchanged_and_recognised() {
        let flags = [
            (SCHEDULED_BACKUP_FLAG, "--scheduled-backup"),
            (SCHEDULED_INTEGRITY_FLAG, "--scheduled-integrity-check"),
            (SCHEDULED_RESTORE_DRILL_FLAG, "--scheduled-restore-drill"),
            (SCHEDULED_HEALTH_REPORT_FLAG, "--scheduled-health-report"),
            (START_MINIMIZED_FLAG, "--minimized"),
        ];
        for (konst, literal) in flags {
            assert_eq!(konst, literal);
            assert!(has_flag(["app.exe", literal], konst));
            assert!(!has_flag(["app.exe"], konst));
        }
    }
}
