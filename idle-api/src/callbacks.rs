use std::sync::OnceLock;

use crate::palette::ScreenPalette;
use crate::system_info::SystemInfo;

/// Host-provided factory for live [`SystemInfo`]. Set once at process startup.
pub static SYSTEM_INFO_CALLBACK: OnceLock<fn() -> SystemInfo> = OnceLock::new();

/// Host-provided factory for the active [`ScreenPalette`]. Set once at startup.
pub static PALETTE_CALLBACK: OnceLock<fn() -> ScreenPalette> = OnceLock::new();

/// Returns live system information by calling the host's registered callback.
pub fn get_system_info() -> SystemInfo {
    if let Some(callback) = SYSTEM_INFO_CALLBACK.get() {
        callback()
    } else {
        SystemInfo::default()
    }
}

/// Returns the current host's visual palette by calling the host's registered callback.
pub fn query_current_palette() -> ScreenPalette {
    if let Some(callback) = PALETTE_CALLBACK.get() {
        callback()
    } else {
        ScreenPalette::default()
    }
}
