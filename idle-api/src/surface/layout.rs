// SPDX-License-Identifier: MIT

//! Multi-monitor layout helpers for Wayland span presentation.

use crate::logo_block::render_logo_block;
use crate::{get_primary_monitor_bounds, is_secondary_monitor};

/// Centered OS logo placement in grid cell coordinates.
#[derive(Debug, Clone)]
pub struct CenteredLogo {
    pub lines: Vec<String>,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// True when the simulation grid spans multiple monitors (primary is a slice of the grid).
pub fn is_span_layout(cols: usize, rows: usize) -> bool {
    if crate::env_is_set(&["IDLE_SPAN_MODE"]) {
        return true;
    }
    let primary = get_primary_monitor_bounds(cols, rows);
    primary.start_col > 0
        || primary.start_row > 0
        || primary.end_col < cols
        || primary.end_row < rows
}

/// How far effects must travel to reach the farthest monitor edge from primary center.
pub fn span_reach_scale(cols: usize, rows: usize) -> f32 {
    if cols == 0 || rows == 0 || !is_span_layout(cols, rows) {
        return 1.0;
    }

    let primary = get_primary_monitor_bounds(cols, rows);
    let pcx = primary.center_col() as f32;
    let pcy = primary.center_row() as f32;
    let corners = [
        (0.0f32, 0.0f32),
        (cols as f32, 0.0),
        (0.0, rows as f32),
        (cols as f32, rows as f32),
    ];
    let max_dist = corners
        .iter()
        .map(|(x, y)| {
            let dx = pcx - x;
            let dy = (pcy - y) * 2.0;
            (dx * dx + dy * dy).sqrt()
        })
        .fold(0.0f32, f32::max);

    let base = (primary.width().min(primary.height()) as f32 * 0.55).max(12.0);
    (max_dist / base).clamp(1.0, 4.5)
}

/// Place a centered logo block within `(cols, rows)`.
///
/// The returned [`CenteredLogo`] carries the rendered text lines plus the
/// grid coordinates where they should be drawn. Returns `None` when running
/// on a secondary monitor, when the rendered block is empty, or when no
/// primary monitor bounds are configured.
///
/// # Example
///
/// ```
/// use idle_api::CenteredLogo;
/// // Construct a CenteredLogo directly to inspect its shape:
/// let logo = CenteredLogo {
///     lines: vec!["Hi".into()],
///     x: 0,
///     y: 0,
///     width: 2,
///     height: 1,
/// };
/// assert_eq!(logo.width, 2);
/// assert_eq!(logo.height, 1);
/// ```
pub fn place_centered_logo(
    cols: usize,
    rows: usize,
    text: &str,
    sub_text: Option<&str>,
) -> Option<CenteredLogo> {
    if is_secondary_monitor() {
        return None;
    }

    let primary = get_primary_monitor_bounds(cols, rows);
    let lines = render_logo_block(text, sub_text);
    let logo_w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let logo_h = lines.len();
    if logo_w == 0 || logo_h == 0 {
        return None;
    }

    let x = primary.start_col + primary.width().saturating_sub(logo_w) / 2;
    let y = primary.start_row + primary.height().saturating_sub(logo_h) / 2;

    Some(CenteredLogo {
        lines,
        x,
        y,
        width: logo_w,
        height: logo_h,
    })
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
