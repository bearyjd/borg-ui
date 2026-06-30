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
    // `GetConnectionCost` lives in iphlpapi. `windows-sys` does not currently
    // expose it through the features used by this crate, so keep the FFI surface
    // narrow and local.
    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetConnectionCost(
            connection_cost: *mut u32,
            destination_ip_address: *const core::ffi::c_void,
        ) -> u32;
    }

    let mut flags = 0_u32;
    // SAFETY: `flags` is a valid out pointer and a null destination asks Windows
    // for the default route's cost.
    let status = unsafe { GetConnectionCost(&mut flags, std::ptr::null()) };
    if status != 0 {
        return Err(format!("GetConnectionCost failed with status {status}"));
    }
    Ok(classify_connection_cost(flags))
}

#[cfg(not(windows))]
fn active_connection_cost_impl() -> Result<NetworkCost, String> {
    let flags = match std::env::var("BORG_UI_TEST_CONNECTION_COST") {
        Ok(value) => value
            .parse::<u32>()
            .map_err(|e| format!("invalid BORG_UI_TEST_CONNECTION_COST: {e}"))?,
        Err(std::env::VarError::NotPresent) => 1,
        Err(e) => return Err(e.to_string()),
    };
    Ok(classify_connection_cost(flags))
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
}
