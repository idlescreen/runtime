//! Platform & Architecture (Deployment).
//! Vendored from `runner::toolkit::platform.rs`. The full module also includes
//! WebPlatform, MobilePlatform, and EmbeddedPlatform stubs (not used by the
//! screensaver) — those are omitted here. If the project ever needs them,
//! pull them from the upstream library.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerStatus {
    pub ac_online: bool,
    pub battery_percent: u8,
}

impl Default for PowerStatus {
    fn default() -> Self {
        Self {
            ac_online: true,
            battery_percent: 100,
        }
    }
}

impl PowerStatus {
    pub const BATTERY_PERCENT_UNKNOWN: u8 = 255;
    pub fn is_battery_percent_unknown(&self) -> bool {
        self.battery_percent == Self::BATTERY_PERCENT_UNKNOWN
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemBiosInfo {
    pub manufacturer: String,
    pub product: String,
    pub model: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskDriveInfo {
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkAdapterInfo {
    pub name: String,
    pub description: String,
    pub ip_addresses: Vec<String>,
    pub adapter_type: String,
    pub is_up: bool,
}

pub use idle_api::SystemInfo;
