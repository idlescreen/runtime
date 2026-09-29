// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Pure scalar dirty-rect diffing.

use super::rect::DamageRect;

/// Compute dirty bounding box between `prev` and `next` buffers using scalar comparison.
pub fn diff_dirty_rect_scalar(
    prev: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Option<DamageRect> {
    if width == 0 || height == 0 {
        return None;
    }
    let stride = (width as usize).saturating_mul(4);
    let expected_len = stride.saturating_mul(height as usize);
    if prev.len() < expected_len || next.len() < expected_len {
        return Some(DamageRect::full(width, height));
    }

    let mut min_x = u32::MAX;
    let mut max_x = 0;
    let mut min_y = u32::MAX;
    let mut max_y = 0;
    let mut any_diff = false;

    for y in 0..height {
        let row_start = (y as usize) * stride;
        let row_end = row_start + stride;
        let row_prev = &prev[row_start..row_end];
        let row_next = &next[row_start..row_end];

        if row_prev == row_next {
            continue;
        }

        let mut row_first_x = None;
        let mut row_last_x = 0;

        for x in 0..width {
            let px_start = (x as usize) * 4;
            let px_end = px_start + 4;
            if row_prev[px_start..px_end] != row_next[px_start..px_end] {
                if row_first_x.is_none() {
                    row_first_x = Some(x);
                }
                row_last_x = x;
            }
        }

        if let Some(first_x) = row_first_x {
            any_diff = true;
            min_x = min_x.min(first_x);
            max_x = max_x.max(row_last_x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    if !any_diff {
        None
    } else {
        Some(DamageRect::new(
            min_x,
            min_y,
            max_x.saturating_sub(min_x) + 1,
            max_y.saturating_sub(min_y) + 1,
        ))
    }
}
