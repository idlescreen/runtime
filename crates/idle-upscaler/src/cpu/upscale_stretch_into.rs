// SPDX-License-Identifier: Apache-2.0
// perf: T1 · bench: stretch · gate: perf-baseline.json
// Copyright 2026 IdleScreen

//! Nearest-neighbor stretch upscale (fills destination, may distort aspect).

use super::stretch_byte_rows::stretch_byte_rows;
use super::stretch_cache::StretchCache;
use super::stretch_u32_rows::stretch_u32_rows;

/// Checked `&[u8]` → `&[u32]` view (bytemuck `try_cast_slice` replacement):
/// requires 4-byte alignment and a multiple-of-4 length.
///
/// # Safety
/// Caller is responsible for `dst.len() >= 16` and the alignment /
/// length-multiple invariants checked below.
#[allow(clippy::cast_ptr_alignment)]
fn try_cast_u8_to_u32(src: &[u8]) -> Result<&[u32], ()> {
    if !src.len().is_multiple_of(4) || !src.as_ptr().addr().is_multiple_of(4) {
        return Err(());
    }
    Ok(unsafe { std::slice::from_raw_parts(src.as_ptr().cast::<u32>(), src.len() / 4) })
}

/// Mutable counterpart of [`try_cast_u8_to_u32`].
///
/// # Safety
/// See [`try_cast_u8_to_u32`].
#[allow(clippy::cast_ptr_alignment)]
fn try_cast_u8_to_u32_mut(dst: &mut [u8]) -> Result<&mut [u32], ()> {
    if !dst.len().is_multiple_of(4) || !dst.as_ptr().addr().is_multiple_of(4) {
        return Err(());
    }
    Ok(unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr().cast::<u32>(), dst.len() / 4) })
}

/// Fast integer nearest-neighbor stretch into `dst` (reuses `cache` x-map).
///
/// Picks the u32-aligned `stretch_u32_rows` fast path when both buffers
/// are 4-byte aligned; otherwise falls back to per-row
/// `stretch_byte_rows`. Identity copy when `src_w*src_h == dst_w*dst_h`.
#[allow(clippy::too_many_arguments)]
pub fn upscale_stretch_into(
    dst: &mut [u8],
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    cache: &mut StretchCache,
) {
    let needed = (dst_w as usize)
        .checked_mul(dst_h as usize)
        .and_then(|p| p.checked_mul(4))
        .unwrap_or(0);
    if dst.len() < needed {
        return;
    }
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        dst[..needed].fill(0);
        return;
    }

    if src_w == dst_w && src_h == dst_h {
        let copy_len = needed.min(src.len());
        dst[..copy_len].copy_from_slice(&src[..copy_len]);
        if needed > copy_len {
            dst[copy_len..needed].fill(0);
        }
        return;
    }

    match (
        try_cast_u8_to_u32(src),
        try_cast_u8_to_u32_mut(&mut dst[..needed]),
    ) {
        (Ok(src_u32), Ok(dst_u32)) => {
            if dst_w < src_w {
                cache.ensure(src_w, dst_w);
            }
            stretch_u32_rows(src_u32, dst_u32, src_w, src_h, dst_w, dst_h, cache)
        }
        _ => {
            cache.ensure(src_w, dst_w);
            stretch_byte_rows(dst, src, src_w, src_h, dst_w, dst_h, needed, cache)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::stretch_cache::StretchCache;
    use super::upscale_stretch_into;

    #[test]
    fn upscale_stretch_handles_zero_dim() {
        let src = vec![0u8; 4];
        let mut dst = vec![0u8; 16];
        let mut cache = StretchCache::new();
        upscale_stretch_into(&mut dst, &src, 0, 0, 2, 2, &mut cache);
        assert_eq!(dst, vec![0u8; 16]);
    }

    #[test]
    fn upscale_stretch_same_size_copies() {
        let src = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut dst = vec![0u8; 8];
        let mut cache = StretchCache::new();
        upscale_stretch_into(&mut dst, &src, 2, 1, 2, 1, &mut cache);
        assert_eq!(dst, src);
    }

    #[test]
    fn upscale_stretch_short_dst_is_noop() {
        // dst shorter than needed: silently no-op (matches the
        // documented "fast path, don't blow up on bad inputs" contract).
        let src = vec![0xFFu8; 64];
        let mut dst = vec![0u8; 4];
        let before = dst.clone();
        let mut cache = StretchCache::new();
        upscale_stretch_into(&mut dst, &src, 2, 2, 4, 4, &mut cache);
        assert_eq!(dst, before);
    }

    #[test]
    fn try_cast_u8_to_u32_rejects_non_multiple_of_4_length() {
        // Heap allocations on x86-64 are typically 16-byte aligned, so
        // the alignment check in `try_cast_u8_to_u32` always passes
        // for Vec-allocated buffers. The length check is the bit we
        // can exercise from a unit test: 15 bytes is not a multiple
        // of 4, so the cast must refuse.
        let v: Vec<u8> = vec![0u8; 15];
        assert!(super::try_cast_u8_to_u32(&v).is_err());
        let v: Vec<u8> = vec![0u8; 16];
        assert!(super::try_cast_u8_to_u32(&v).is_ok());
    }
}

