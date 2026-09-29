// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! SIMD AVX2/NEON and scalar dirty-rectangle diffing for Wayland partial damage.

pub mod rect;
pub mod scalar;
pub mod simd_avx2;
pub mod simd_neon;

#[cfg(test)]
mod damage_tests;

pub use rect::DamageRect;

/// Maximum damage area fraction (85%) above which full damage is committed.
pub const DAMAGE_FULL_THRESHOLD_PCT: u64 = 85;

/// Compute dirty damage bounding box with automatic CPU feature detection.
pub fn compute_damage_rect(
    prev: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Option<DamageRect> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            // SAFETY: feature detected via CPUID before calling AVX2 function.
            return unsafe { simd_avx2::diff_dirty_rect_avx2(prev, next, width, height) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is baseline on aarch64.
        return unsafe { simd_neon::diff_dirty_rect_neon(prev, next, width, height) };
    }

    #[allow(unreachable_code)]
    scalar::diff_dirty_rect_scalar(prev, next, width, height)
}

/// Compute dirty damage bounding box guarded by the 85% area threshold.
///
/// Returns:
/// - `None` if buffers are identical (no damage to submit).
/// - `Some(DamageRect::full(width, height))` if dirty area >= 85% of total area.
/// - `Some(rect)` for smaller localized dirty regions.
pub fn compute_damage_with_threshold(
    prev: &[u8],
    next: &[u8],
    width: u32,
    height: u32,
) -> Option<DamageRect> {
    let dirty = compute_damage_rect(prev, next, width, height)?;
    let total_area = (width as u64) * (height as u64);
    if total_area == 0 {
        return None;
    }

    let dirty_area = dirty.area();
    if dirty_area.saturating_mul(100) >= total_area.saturating_mul(DAMAGE_FULL_THRESHOLD_PCT) {
        Some(DamageRect::full(width, height))
    } else {
        Some(dirty)
    }
}
