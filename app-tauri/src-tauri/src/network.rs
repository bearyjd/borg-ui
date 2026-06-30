//! Network-cost detection used by unattended scheduled backups.
//!
//! The user-facing control is intentionally conservative: when Windows reports
//! the active route as fixed/variable cost, roaming, or over/near the data
//! limit, scheduled backups can skip that run. Manual backups never consult
//! this module.

/// Result of the active network cost probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkCost {
    Unrestricted,
    Metered,
    Unknown,
}

impl NetworkCost {
    pub fn is_metered(self) -> bool {
        matches!(self, Self::Metered)
    }
}

const CONNECTION_COST_FIXED: u32 = 0x0000_0002;
const CONNECTION_COST_VARIABLE: u32 = 0x0000_0004;
const CONNECTION_COST_OVERDATALIMIT: u32 = 0x0001_0000;
const CONNECTION_COST_ROAMING: u32 = 0x0004_0000;
const CONNECTION_COST_APPROACHINGDATALIMIT: u32 = 0x0008_0000;

pub fn classify_connection_cost(flags: u32) -> NetworkCost {
    let metered = CONNECTION_COST_FIXED
        | CONNECTION_COST_VARIABLE
        | CONNECTION_COST_OVERDATALIMIT
        | CONNECTION_COST_ROAMING
        | CONNECTION_COST_APPROACHINGDATALIMIT;
    if flags & metered != 0 {
        NetworkCost::Metered
    } else if flags == 0 {
        NetworkCost::Unknown
    } else {
        NetworkCost::Unrestricted
    }
}

pub fn active_connection_cost() -> Result<NetworkCost, String> {
    active_connection_cost_impl()
}

#[cfg(windows)]
fn active_connection_cost_impl() -> Result<NetworkCost, String> {
    let script = concat!(
        "$p=[Windows.Networking.Connectivity.NetworkInformation,",
        "Windows.Networking.Connectivity,ContentType=WindowsRuntime]",
        "::GetInternetConnectionProfile();",
        "if($null -eq $p){'unknown';exit};",
        "$c=$p.GetConnectionCost();",
        "if($c.Roaming -or $c.OverDataLimit -or $c.ApproachingDataLimit",
        " -or $c.NetworkCostType -in @('Fixed','Variable')){'metered'}",
        "elseif($c.NetworkCostType -eq 'Unrestricted'){'unrestricted'}",
        "else{'unknown'}"
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("could not query Windows network cost: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windows network cost query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_connection_cost(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn active_connection_cost_impl() -> Result<NetworkCost, String> {
    match std::env::var("BORG_UI_TEST_CONNECTION_COST") {
        Ok(value) => value
            .parse::<u32>()
            .map(classify_connection_cost)
            .map_err(|e| format!("invalid BORG_UI_TEST_CONNECTION_COST: {e}")),
        Err(std::env::VarError::NotPresent) => parse_connection_cost("unrestricted"),
        Err(e) => Err(e.to_string()),
    }
}

fn parse_connection_cost(value: &str) -> Result<NetworkCost, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "metered" => Ok(NetworkCost::Metered),
        "unrestricted" => Ok(NetworkCost::Unrestricted),
        "unknown" => Ok(NetworkCost::Unknown),
        other => Err(format!("unexpected Windows network cost response: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_zero_as_unknown() {
        assert_eq!(classify_connection_cost(0), NetworkCost::Unknown);
    }

    #[test]
    fn classifies_unrestricted_as_not_metered() {
        assert_eq!(classify_connection_cost(1), NetworkCost::Unrestricted);
    }

    #[test]
    fn classifies_cost_flags_as_metered() {
        for flags in [
            CONNECTION_COST_FIXED,
            CONNECTION_COST_VARIABLE,
            CONNECTION_COST_OVERDATALIMIT,
            CONNECTION_COST_ROAMING,
            CONNECTION_COST_APPROACHINGDATALIMIT,
        ] {
            assert_eq!(classify_connection_cost(flags), NetworkCost::Metered);
        }
    }

    #[test]
    fn parses_windows_cost_probe_output() {
        assert_eq!(
            parse_connection_cost("metered\r\n").unwrap(),
            NetworkCost::Metered
        );
        assert_eq!(
            parse_connection_cost("Unrestricted").unwrap(),
            NetworkCost::Unrestricted
        );
        assert_eq!(
            parse_connection_cost("unknown").unwrap(),
            NetworkCost::Unknown
        );
        assert!(parse_connection_cost("noise").is_err());
    }
}
