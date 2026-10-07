use std::sync::OnceLock;

use crate::color::ScreenPalette;
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

/// The wordmark every saver renders when it has no configured text of its own.
///
/// One accessor so savers cannot drift: eleven of them read
/// `get_system_info().logo_text` directly and one read a local constant, which
/// is how the same machine ended up showing different words on different
/// savers. Prefer this over touching `logo_text` directly.
pub fn wordmark() -> String {
    get_system_info().logo_text
}
