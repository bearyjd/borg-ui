#[cfg(windows)]
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    Ac,
    Battery,
    Unknown,
}

#[cfg(windows)]
pub fn power_source() -> Result<PowerSource, String> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS {
        ACLineStatus: 255,
        BatteryFlag: 0,
        BatteryLifePercent: 255,
        SystemStatusFlag: 0,
        BatteryLifeTime: 0,
        BatteryFullLifeTime: 0,
    };
    // SAFETY: status is a valid writable SYSTEM_POWER_STATUS for the call.
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(match status.ACLineStatus {
        0 => PowerSource::Battery,
        1 => PowerSource::Ac,
        _ => PowerSource::Unknown,
    })
}

#[cfg(not(windows))]
pub fn power_source() -> Result<PowerSource, String> {
    match std::env::var("BORG_UI_TEST_POWER").as_deref() {
        Ok("battery") => Ok(PowerSource::Battery),
        Ok("unknown") => Ok(PowerSource::Unknown),
        Ok("error") => Err("test power probe failure".into()),
        _ => Ok(PowerSource::Ac),
    }
}

/// RAII sleep inhibitor. Dropping it restores the thread's normal execution
/// state even after cancellation or a Borg failure.
pub struct SleepGuard {
    active: bool,
}

impl SleepGuard {
    pub fn acquire(enabled: bool) -> Result<Self, String> {
        if enabled {
            set_sleep_required(true)?;
        }
        Ok(Self { active: enabled })
    }
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = set_sleep_required(false);
        }
    }
}

#[cfg(windows)]
fn set_sleep_required(required: bool) -> Result<(), String> {
    use windows_sys::Win32::System::Power::{
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };
    let flags = if required {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    // SAFETY: flags are documented EXECUTION_STATE values.
    if unsafe { SetThreadExecutionState(flags) } == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn set_sleep_required(_required: bool) -> Result<(), String> {
    #[cfg(test)]
    TEST_SLEEP_REQUIRED.store(_required, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[cfg(all(test, not(windows)))]
static TEST_SLEEP_REQUIRED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Current connected SSID. The Windows WLAN query is deliberately path-free
/// and returns only the network name used for policy matching.
pub fn current_wifi_name() -> Result<Option<String>, String> {
    current_wifi_name_impl()
}

#[cfg(windows)]
fn current_wifi_name_impl() -> Result<Option<String>, String> {
    let output = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .output()
        .map_err(|error| format!("could not query Windows WLAN state: {error}"))?;
    if !output.status.success() {
        return Err("Windows WLAN query failed".into());
    }
    Ok(parse_netsh_ssid(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(not(windows))]
fn current_wifi_name_impl() -> Result<Option<String>, String> {
    Ok(std::env::var("BORG_UI_TEST_WIFI").ok())
}

#[cfg(any(windows, test))]
fn parse_netsh_ssid(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (label, value) = line.split_once(':')?;
        (label.trim().eq_ignore_ascii_case("SSID") && !value.trim().is_empty())
            .then(|| value.trim().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssid_without_matching_bssid() {
        let output = "    BSSID : aa:bb:cc\n    SSID : Office Wi-Fi\n";
        assert_eq!(parse_netsh_ssid(output).as_deref(), Some("Office Wi-Fi"));
    }

    #[test]
    fn sleep_guard_disabled_is_noop() {
        assert!(!SleepGuard::acquire(false).unwrap().active);
    }

    #[cfg(not(windows))]
    #[test]
    fn sleep_guard_drop_restores_execution_state() {
        {
            let _guard = SleepGuard::acquire(true).unwrap();
            assert!(TEST_SLEEP_REQUIRED.load(std::sync::atomic::Ordering::SeqCst));
        }
        assert!(!TEST_SLEEP_REQUIRED.load(std::sync::atomic::Ordering::SeqCst));
    }
}
