// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! NEON-accelerated dirty-rect diffing for aarch64.

#![allow(unsafe_code)]

use super::rect::DamageRect;

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// NEON diffing of two pixel buffers.
///
/// # Safety
/// Caller must ensure NEON instructions are supported (standard on aarch64).
#[cfg(target_arch = "aarch64")]
pub unsafe fn diff_dirty_rect_neon(
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
        let row_prev = &prev[row_start..row_start + stride];
        let row_next = &next[row_start..row_start + stride];

        let mut row_first_x = None;
        let mut row_last_x = 0;

        let mut offset = 0;
        while offset + 16 <= stride {
            // SAFETY: `offset + 16 <= stride` guaranteed by loop condition, pointer is valid for 16 bytes read.
            let (v_p, v_n) = unsafe {
                (
                    vld1q_u8(row_prev.as_ptr().add(offset)),
                    vld1q_u8(row_next.as_ptr().add(offset)),
                )
            };
            // SAFETY: valid uint8x16_t vectors passed to vceqq_u8.
            let eq = unsafe { vceqq_u8(v_p, v_n) };
            // SAFETY: valid uint8x16_t vector passed to vminvq_u8; returns 0xFF if all equal.
            let min_val = unsafe { vminvq_u8(eq) };

            if min_val != 0xFF {
                for b in 0..16 {
                    if row_prev[offset + b] != row_next[offset + b] {
                        let px = ((offset + b) / 4) as u32;
                        if row_first_x.is_none() {
                            row_first_x = Some(px);
                        }
                        row_last_x = row_last_x.max(px);
                    }
                }
            }
            offset += 16;
        }

        while offset + 4 <= stride {
            if row_prev[offset..offset + 4] != row_next[offset..offset + 4] {
                let px = (offset / 4) as u32;
                if row_first_x.is_none() {
                    row_first_x = Some(px);
                }
                row_last_x = row_last_x.max(px);
            }
            offset += 4;
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

/// Fallback for non-aarch64 platforms.
#[cfg(not(target_arch = "aarch64"))]
#[allow(dead_code)]
pub fn diff_dirty_rect_neon(
    prev: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Option<DamageRect> {
    super::scalar::diff_dirty_rect_scalar(prev, next, width, height)
}
