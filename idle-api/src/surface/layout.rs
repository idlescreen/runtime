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
/// Trim `text` so its block render fits `width` columns.
///
/// The 5x5 block font renders each character to six columns. Rather than let a
/// long string overflow into nothing, keep as many whole characters as fit.
fn fit_to_width(text: &str, width: usize) -> String {
    const CHARS_PER_CHAR: usize = 6;
    let budget = width / CHARS_PER_CHAR;
    if budget == 0 {
        return String::new();
    }
    let trimmed: String = text.trim().chars().take(budget).collect();
    trimmed.trim_end().to_string()
}

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
    // Fit before rendering. An OS name like "Fedora Linux 44 (Server Edition)"
    // block-renders to ~195 columns; at that width the centered x saturates
    // and the saver has nothing sensible to draw. Trimming here fixes every
    // saver that uses this helper at once, rather than one at a time.
    let text = fit_to_width(text, primary.width());
    let sub_text = sub_text.map(|t| fit_to_width(t, primary.width()));
    let lines = render_logo_block(&text, sub_text.as_deref());
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
