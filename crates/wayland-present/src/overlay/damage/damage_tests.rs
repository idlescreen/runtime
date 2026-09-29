// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Unit tests for SIMD and scalar dirty-rectangle diffing.

use super::rect::DamageRect;
use super::scalar::diff_dirty_rect_scalar;
use super::{compute_damage_rect, compute_damage_with_threshold};

fn fill_buffer(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut buf = vec![0u8; (width * height * 4) as usize];
    for chunk in buf.as_chunks_mut::<4>().0 {
        *chunk = color;
    }
    buf
}

#[test]
fn test_identical_buffers_no_damage() {
    let buf1 = fill_buffer(64, 64, [0x11, 0x22, 0x33, 0xFF]);
    let buf2 = fill_buffer(64, 64, [0x11, 0x22, 0x33, 0xFF]);
    let damage = compute_damage_rect(&buf1, &buf2, 64, 64);
    assert_eq!(damage, None);
}

#[test]
fn test_completely_different_buffers() {
    let buf1 = fill_buffer(64, 64, [0x11, 0x22, 0x33, 0xFF]);
    let buf2 = fill_buffer(64, 64, [0x44, 0x55, 0x66, 0xFF]);
    let damage = compute_damage_rect(&buf1, &buf2, 64, 64);
    assert_eq!(damage, Some(DamageRect::new(0, 0, 64, 64)));
}

#[test]
fn test_localized_damage_box() {
    let width = 100u32;
    let height = 80u32;
    let buf1 = fill_buffer(width, height, [0x00, 0x00, 0x00, 0xFF]);
    let mut buf2 = buf1.clone();

    // Modify a 10x10 subregion starting at (20, 15)
    for y in 15..25 {
        for x in 20..30 {
            let offset = ((y * width + x) * 4) as usize;
            buf2[offset] = 0xAA;
            buf2[offset + 1] = 0xBB;
            buf2[offset + 2] = 0xCC;
            buf2[offset + 3] = 0xFF;
        }
    }

    let scalar_res = diff_dirty_rect_scalar(&buf1, &buf2, width, height);
    let expected = Some(DamageRect::new(20, 15, 10, 10));
    assert_eq!(scalar_res, expected);

    let detected_res = compute_damage_rect(&buf1, &buf2, width, height);
    assert_eq!(detected_res, expected);
}

#[test]
fn test_85_percent_threshold_guard() {
    let width = 100u32;
    let height = 100u32;
    let buf1 = fill_buffer(width, height, [0x00, 0x00, 0x00, 0xFF]);

    // 1. Below 85% area (e.g. 50x50 = 25% of area)
    let mut buf_small = buf1.clone();
    for y in 0..50 {
        for x in 0..50 {
            let offset = ((y * width + x) * 4) as usize;
            buf_small[offset] = 0xFF;
        }
    }
    let res_small = compute_damage_with_threshold(&buf1, &buf_small, width, height);
    assert_eq!(res_small, Some(DamageRect::new(0, 0, 50, 50)));

    // 2. At or above 85% area (e.g. 95x95 = 9025 / 10000 = 90.25%)
    let mut buf_large = buf1.clone();
    for y in 0..95 {
        for x in 0..95 {
            let offset = ((y * width + x) * 4) as usize;
            buf_large[offset] = 0xFF;
        }
    }
    let res_large = compute_damage_with_threshold(&buf1, &buf_large, width, height);
    // Should be clamped to full rect
    assert_eq!(res_large, Some(DamageRect::full(width, height)));
}

#[test]
fn test_damage_zero_dimensions_or_short_buffer() {
    let buf = vec![0u8; 64];
    assert_eq!(compute_damage_rect(&buf, &buf, 0, 10), None);
    assert_eq!(compute_damage_rect(&buf, &buf, 10, 0), None);

    // Short buffer returns full damage fallback
    let short_buf = vec![0u8; 10];
    assert_eq!(
        compute_damage_rect(&short_buf, &buf, 4, 4),
        Some(DamageRect::full(4, 4))
    );
}

#[test]
fn test_damage_irregular_stride_and_boundary_pixels() {
    // 17x11: width 17 is not a multiple of 8 (AVX2 32B) or 4 (NEON 16B)
    let width = 17u32;
    let height = 11u32;
    let buf1 = fill_buffer(width, height, [0x00, 0x00, 0x00, 0xFF]);
    let mut buf2 = buf1.clone();

    // Modify top-left pixel (0, 0)
    buf2[0] = 0xAA;
    // Modify bottom-right pixel (16, 10)
    let last_offset = (((10 * width) + 16) * 4) as usize;
    buf2[last_offset] = 0xBB;

    let res = compute_damage_rect(&buf1, &buf2, width, height);
    assert_eq!(res, Some(DamageRect::new(0, 0, 17, 11)));
}

#[test]
fn test_damage_rect_union_and_empty() {
    let r1 = DamageRect::new(10, 10, 20, 20);
    let r2 = DamageRect::new(15, 15, 30, 30);
    let union = r1.union(&r2);
    assert_eq!(union, DamageRect::new(10, 10, 35, 35));

    let empty = DamageRect::new(0, 0, 0, 0);
    assert!(empty.is_empty());
    assert_eq!(r1.union(&empty), r1);
    assert_eq!(empty.union(&r2), r2);
}
