// SPDX-License-Identifier: MIT

//! Battery / AC heuristics that force inhibit when on battery.

/// Pure policy: should idle presentation be treated as inhibited for power?
///
/// - AC present and offline → on battery
/// - Battery reporting Discharging → on battery
/// - No power_supply info → not inhibited by this path
pub fn battery_should_inhibit(has_ac: bool, ac_online: bool, battery_discharging: bool) -> bool {
    (has_ac && !ac_online) || battery_discharging
}

/// Read `/sys/class/power_supply` and apply [`battery_should_inhibit`].
pub fn is_on_battery() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        if std::env::var("IDLE_TEST_MOCK_BATTERY").is_ok() {
            return true;
        }
        if std::env::var("IDLE_TEST_MOCK_AC").is_ok() {
            return false;
        }

        let path = std::path::Path::new("/sys/class/power_supply");
        let Ok(entries) = std::fs::read_dir(path) else {
            return false;
        };

        let mut has_ac = false;
        let mut ac_online = true;
        let mut battery_discharging = false;

        for entry in entries.flatten() {
            let p = entry.path();
            let Ok(t) = std::fs::read_to_string(p.join("type")) else {
                continue;
            };
            let type_str = t.trim();
            if type_str == "Mains" {
                has_ac = true;
                if let Ok(o) = std::fs::read_to_string(p.join("online")) {
                    ac_online = o.trim() != "0";
                }
            } else if type_str == "Battery"
                && let Ok(s) = std::fs::read_to_string(p.join("status"))
                && s.trim() == "Discharging"
            {
                battery_discharging = true;
            }
        }

        battery_should_inhibit(has_ac, ac_online, battery_discharging)
    }
}

#[cfg(test)]
mod tests {
    use super::battery_should_inhibit;

    #[test]
    fn ac_offline_inhibits() {
        assert!(battery_should_inhibit(true, false, false));
    }

    #[test]
    fn ac_online_does_not_inhibit() {
        assert!(!battery_should_inhibit(true, true, false));
    }

    #[test]
    fn discharging_battery_inhibits() {
        assert!(battery_should_inhibit(false, true, true));
        assert!(battery_should_inhibit(true, true, true));
    }

    #[test]
    fn no_ac_no_discharge_does_not_inhibit() {
        // Desktop without Mains node: do not invent inhibit.
        assert!(!battery_should_inhibit(false, true, false));
        assert!(!battery_should_inhibit(false, false, false));
    }
}
