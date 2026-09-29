//! Multi-monitor cell-grid bounds and host-registered callbacks.

use std::sync::OnceLock;

mod bounds;
mod env;

pub use bounds::MonitorCellBounds;
use env::{cached_primary_bounds_from_env, clear_stored_primary_bounds, store_primary_bounds};

pub static MONITOR_BOUNDS_CALLBACK: OnceLock<fn(usize, usize) -> MonitorCellBounds> =
    OnceLock::new();
pub static IS_SECONDARY_MONITOR_CALLBACK: OnceLock<fn() -> bool> = OnceLock::new();

pub fn get_primary_monitor_bounds(cols: usize, rows: usize) -> MonitorCellBounds {
    if let Some(callback) = MONITOR_BOUNDS_CALLBACK.get() {
        return callback(cols, rows);
    }
    if let Some(bounds) = cached_primary_bounds_from_env() {
        return bounds;
    }
    MonitorCellBounds {
        start_col: 0,
        end_col: cols,
        start_row: 0,
        end_row: rows,
        is_primary: true,
    }
}

pub fn publish_primary_bounds(bounds: MonitorCellBounds) {
    store_primary_bounds(bounds);
}

pub fn clear_primary_bounds() {
    clear_stored_primary_bounds();
}

pub fn is_secondary_monitor() -> bool {
    if let Some(callback) = IS_SECONDARY_MONITOR_CALLBACK.get() {
        callback()
    } else {
        crate::env_is_set(&["IDLE_SECONDARY_MONITOR"])
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "env_tests.rs"]
mod env_tests;
