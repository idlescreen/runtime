// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! AVX2-accelerated dirty-rect diffing for x86_64.

#![allow(unsafe_code)]

use super::rect::DamageRect;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{_mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8};

/// AVX2 diffing of two pixel buffers.
///
/// # Safety
/// Caller must ensure AVX2 is available on the host CPU.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
pub unsafe fn diff_dirty_rect_avx2(
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
        while offset + 32 <= stride {
            // SAFETY: `offset + 32 <= stride` guaranteed by loop condition, pointers are valid for 32 bytes read.
            let (v_p, v_n) = unsafe {
                (
                    _mm256_loadu_si256(row_prev.as_ptr().add(offset).cast()),
                    _mm256_loadu_si256(row_next.as_ptr().add(offset).cast()),
                )
            };
            let eq = _mm256_cmpeq_epi8(v_p, v_n);
            let mask = _mm256_movemask_epi8(eq);

            if mask != -1i32 {
                let diff_bits = (!mask) as u32;
                let first_byte = diff_bits.trailing_zeros() as usize;
                let last_byte = 31 - (diff_bits.leading_zeros() as usize);
                let first_px = ((offset + first_byte) / 4) as u32;
                let last_px = ((offset + last_byte) / 4) as u32;

                if row_first_x.is_none() {
                    row_first_x = Some(first_px);
                }
                row_last_x = row_last_x.max(last_px);
            }
            offset += 32;
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

/// Fallback for non-x86_64 platforms.
#[cfg(not(target_arch = "x86_64"))]
pub fn diff_dirty_rect_avx2(
    prev: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Option<DamageRect> {
    super::scalar::diff_dirty_rect_scalar(prev, next, width, height)
}
