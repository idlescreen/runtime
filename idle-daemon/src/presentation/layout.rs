// SPDX-License-Identifier: MIT

#![allow(clippy::too_many_arguments)]

use std::sync::{OnceLock, RwLock};

use super::ipc_session::IpcPluginSession;
use idle_api::MonitorCellBounds;
use idle_api::OutputLayout;

#[derive(Clone, Copy)]
struct ScaledBoundsState {
    bounds: MonitorCellBounds,
    initial_cols: usize,
    initial_rows: usize,
}

static PRIMARY_BOUNDS_STATE: OnceLock<RwLock<ScaledBoundsState>> = OnceLock::new();

/// Saturating cast of a pixel extent to `i32` (Wayland logical coords).
fn extent_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// Non-negative i32 → usize without wrapping on negative inputs.
fn nonneg_usize(v: i32) -> usize {
    if v <= 0 { 0 } else { v as usize }
}

/// Caps span simulation cost: full virtual-desktop coverage with a bounded cell count.
pub fn span_simulation_grid(
    session: &IpcPluginSession,
    total_w: u32,
    total_h: u32,
) -> (usize, usize) {
    const MAX_SPAN_CELLS: usize = 12_000;
    let (cols, rows) = session.grid_for_pixels(total_w, total_h);
    let cells = cols.saturating_mul(rows);
    if cells <= MAX_SPAN_CELLS {
        return (cols, rows);
    }

    let scale = (MAX_SPAN_CELLS as f32 / cells as f32).sqrt();
    let capped_cols = ((cols as f32 * scale).floor() as usize).max(1);
    let capped_rows = ((rows as f32 * scale).floor() as usize).max(1);
    idle_log::warn!(
        "span grid capped from {cols}x{rows} ({cells} cells) to {capped_cols}x{capped_rows}",
        capped_cols = capped_cols,
        capped_rows = capped_rows,
    );
    (capped_cols, capped_rows)
}

pub fn virtual_desktop(layouts: &[OutputLayout]) -> (i32, i32, u32, u32) {
    let min_x = layouts.iter().map(|layout| layout.x).min().unwrap_or(0);
    let min_y = layouts.iter().map(|layout| layout.y).min().unwrap_or(0);
    let max_x = layouts
        .iter()
        .map(|layout| layout.x.saturating_add(extent_i32(layout.width)))
        .max()
        .unwrap_or(0);
    let max_y = layouts
        .iter()
        .map(|layout| layout.y.saturating_add(extent_i32(layout.height)))
        .max()
        .unwrap_or(0);
    let span_x = max_x.saturating_sub(min_x).max(1);
    let span_y = max_y.saturating_sub(min_y).max(1);
    (
        min_x,
        min_y,
        u32::try_from(span_x).unwrap_or(u32::MAX).max(1),
        u32::try_from(span_y).unwrap_or(u32::MAX).max(1),
    )
}

pub fn monitor_cell_bounds(
    layout: OutputLayout,
    min_x: i32,
    min_y: i32,
    total_w: u32,
    total_h: u32,
    virtual_cols: usize,
    virtual_rows: usize,
    is_primary: bool,
) -> MonitorCellBounds {
    let tw = (total_w as usize).max(1);
    let th = (total_h as usize).max(1);
    // Use saturating sub so inverted / partial layouts never wrap to huge usize.
    let rel_x1 = layout.x.saturating_sub(min_x);
    let rel_y1 = layout.y.saturating_sub(min_y);
    let rel_x2 = rel_x1.saturating_add(extent_i32(layout.width));
    let rel_y2 = rel_y1.saturating_add(extent_i32(layout.height));

    MonitorCellBounds {
        start_col: nonneg_usize(rel_x1).saturating_mul(virtual_cols) / tw,
        end_col: nonneg_usize(rel_x2).saturating_mul(virtual_cols) / tw,
        start_row: nonneg_usize(rel_y1).saturating_mul(virtual_rows) / th,
        end_row: nonneg_usize(rel_y2).saturating_mul(virtual_rows) / th,
        is_primary,
    }
}

pub fn primary_bounds_in_grid(
    primary: OutputLayout,
    min_x: i32,
    min_y: i32,
    total_w: u32,
    total_h: u32,
    virtual_cols: usize,
    virtual_rows: usize,
) -> MonitorCellBounds {
    monitor_cell_bounds(
        primary,
        min_x,
        min_y,
        total_w,
        total_h,
        virtual_cols,
        virtual_rows,
        true,
    )
}

pub fn install_primary_bounds_callback(
    bounds: MonitorCellBounds,
    initial_cols: usize,
    initial_rows: usize,
) {
    let _ = PRIMARY_BOUNDS_STATE.set(RwLock::new(ScaledBoundsState {
        bounds,
        initial_cols: initial_cols.max(1),
        initial_rows: initial_rows.max(1),
    }));
    let _ = idle_api::MONITOR_BOUNDS_CALLBACK.set(|cols, rows| {
        PRIMARY_BOUNDS_STATE
            .get()
            .and_then(|lock| lock.read().ok())
            .map(|guard| {
                let state = *guard;
                let col_scale = cols as f32 / state.initial_cols as f32;
                let row_scale = rows as f32 / state.initial_rows as f32;
                MonitorCellBounds {
                    start_col: (state.bounds.start_col as f32 * col_scale).round() as usize,
                    end_col: (state.bounds.end_col as f32 * col_scale).round() as usize,
                    start_row: (state.bounds.start_row as f32 * row_scale).round() as usize,
                    end_row: (state.bounds.end_row as f32 * row_scale).round() as usize,
                    is_primary: state.bounds.is_primary,
                }
            })
            .unwrap_or(MonitorCellBounds {
                start_col: 0,
                end_col: cols,
                start_row: 0,
                end_row: rows,
                is_primary: true,
            })
    });
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
