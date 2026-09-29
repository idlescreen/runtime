//! In-process cache for primary monitor cell bounds.
//!
//! `idle-api::publish_primary_bounds` and `idle-api::clear_primary_bounds` are
//! the canonical writers; they update the process-local cache only. A separate,
//! dedicated IPC channel between the daemon and its helpers is the planned
//! replacement for the previous env-var shim (which called
//! `unsafe { std::env::set_var }` from arbitrary threads). Until that channel
//! ships, cross-process publishing must be done by setting the
//! `IDLE_PRIMARY_*` environment variables on the **child** before exec so the
//! kernel inherits them; do not rely on `set_var` from a publisher.

use std::sync::{OnceLock, RwLock};

use super::bounds::MonitorCellBounds;

static ENV_PRIMARY_BOUNDS: OnceLock<RwLock<Option<MonitorCellBounds>>> = OnceLock::new();

fn env_bounds_cache() -> &'static RwLock<Option<MonitorCellBounds>> {
    ENV_PRIMARY_BOUNDS.get_or_init(|| RwLock::new(None))
}

pub(crate) fn cached_primary_bounds_from_env() -> Option<MonitorCellBounds> {
    if let Ok(read_guard) = env_bounds_cache().read()
        && let Some(bounds) = *read_guard
    {
        return Some(bounds);
    }
    let mut cache = env_bounds_cache()
        .write()
        .unwrap_or_else(|p| p.into_inner());
    if cache.is_none() {
        *cache = read_primary_bounds_from_env();
    }
    *cache
}

fn read_primary_bounds_from_env() -> Option<MonitorCellBounds> {
    let start_col = crate::env_var_first(&["IDLE_PRIMARY_START_COL"])?
        .parse()
        .ok()?;
    let end_col = crate::env_var_first(&["IDLE_PRIMARY_END_COL"])?
        .parse()
        .ok()?;
    let start_row = crate::env_var_first(&["IDLE_PRIMARY_START_ROW"])?
        .parse()
        .ok()?;
    let end_row = crate::env_var_first(&["IDLE_PRIMARY_END_ROW"])?
        .parse()
        .ok()?;
    if end_col <= start_col || end_row <= start_row {
        return None;
    }
    const MAX_GRID: usize = 16_384;
    if end_col > MAX_GRID || end_row > MAX_GRID {
        return None;
    }
    Some(MonitorCellBounds {
        start_col,
        end_col,
        start_row,
        end_row,
        is_primary: true,
    })
}

pub(crate) fn store_primary_bounds(bounds: MonitorCellBounds) {
    *env_bounds_cache()
        .write()
        .unwrap_or_else(|p| p.into_inner()) = Some(bounds);
}

pub(crate) fn clear_stored_primary_bounds() {
    *env_bounds_cache()
        .write()
        .unwrap_or_else(|p| p.into_inner()) = None;
}
