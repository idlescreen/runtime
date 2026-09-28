// SPDX-License-Identifier: MIT

// perf: T1 · bench: draw_frame · gate: perf-baseline.json
//! Fade-in BGRA buffer by elapsed fractional duration.
//!
//! SSE2 fast path on x86_64 (processes 4 BGRA pixels per `_mm_loadu_si128`
//! + saturating `_mm_packus_epi16` back to u8); scalar fallback on
//! every other target. Both paths use the bias-half-up rounding
//! `(value * mult + 128) >> 8` so they're bit-identical to each other
//! and to `cpu::sample::lerp_u8`.

use std::time::Duration;

/// Fade in `pixels` by `elapsed / 500ms`. Past `500ms` the buffer is
/// returned unchanged (the composited frame is fully opaque).
pub fn apply_fade_in(pixels: &mut [u8], elapsed: Duration) {
    let fade_duration = Duration::from_millis(500);
    if elapsed >= fade_duration {
        return;
    }

    let alpha_multiplier = elapsed.as_secs_f32() / fade_duration.as_secs_f32();
    // Same integer math as `cpu::sample::lerp_u8`: bias-half-up round
    // via `(value * mult + 128) >> 8`, which matches what the prior
    // scalar `(value * mult) / 255` produced except at one rounding
    // boundary per ~256 pixels.
    let mult = (alpha_multiplier * 256.0) as u32;

    if mult == 0 {
        // The whole frame is fully transparent — clear to zero
        // instead of looping through `((v * 0 + 128) >> 8) = 64` per
        // channel. (A pre-multiplied-zero buffer is what the
        // compositor treats as "fully transparent" over a
        // HW-scaled layer surface.)
        pixels.fill(0);
        return;
    }

    // SSE2 reads/writes 16 bytes = 4 BGRA pixels at a time. Iterate
    // on 16-byte chunks first; the tail handles any 4-byte slice the
    // 16-byte grid didn't cover (0–3 leftover BGRA pixels).
    let mut chunks = pixels.chunks_exact_mut(16);

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            // SAFETY: SSE2 is baseline on x86_64 and we just
            // confirmed it via the runtime feature probe above.
            // The mult=0 case was short-circuited earlier in this fn.
            unsafe { apply_fade_sse2(&mut chunks, mult) };
        } else {
            apply_fade_scalar_16(&mut chunks, mult);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        apply_fade_scalar_16(&mut chunks, mult);
    }

    // Per-pixel tail (when the buffer length isn't a multiple of 16
    // — should be unreachable for width * height * 4 BGRA frames
    // since 1080p and 1440p both have widths divisible by 4 and
    // heights even; the safe path is non-SIMD on the leftover).
    for chunk in chunks.into_remainder().chunks_exact_mut(4) {
        chunk[0] = ((u32::from(chunk[0]) * mult + 128) >> 8) as u8;
        chunk[1] = ((u32::from(chunk[1]) * mult + 128) >> 8) as u8;
        chunk[2] = ((u32::from(chunk[2]) * mult + 128) >> 8) as u8;
        chunk[3] = ((u32::from(chunk[3]) * mult + 128) >> 8) as u8;
    }
}

/// SSE2 fade-in: each 16-byte load covers four BGRA pixels. The
/// inner loop runs one u16 multiply, a u16 bias add, and a u16
/// arithmetic shift per channel lane. Saturating pack back to u8
/// matches the truncated-to-u8 cast the scalar path uses.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[allow(unsafe_op_in_unsafe_fn, clippy::cast_ptr_alignment)]
unsafe fn apply_fade_sse2(chunks: &mut std::slice::ChunksExactMut<'_, u8>, mult: u32) {
    use std::arch::x86_64::{
        _mm_add_epi16, _mm_loadu_si128, _mm_mullo_epi16, _mm_packus_epi16, _mm_set1_epi16,
        _mm_srli_epi16, _mm_storeu_si128, _mm_unpackhi_epi8, _mm_unpacklo_epi8,
    };
    let mult_v = _mm_set1_epi16(mult as i16);
    let bias = _mm_set1_epi16(128);
    let zero = _mm_set1_epi16(0);
    for chunk in chunks {
        let ptr = chunk.as_mut_ptr();
        // SAFETY: 16 bytes — `chunks_exact_mut(16)` guarantees
        // exactly one full 16-byte window per iteration. The load
        // is `storeu` (handles unaligned); we never read past the
        // slice's end because the iterator is bounded by
        // `chunks_exact_mut`.
        unsafe {
            let v = _mm_loadu_si128(ptr as *const _);
            let lo = _mm_unpacklo_epi8(v, zero);
            let hi = _mm_unpackhi_epi8(v, zero);
            let lo = _mm_mullo_epi16(lo, mult_v);
            let hi = _mm_mullo_epi16(hi, mult_v);
            let lo = _mm_add_epi16(lo, bias);
            let hi = _mm_add_epi16(hi, bias);
            let lo = _mm_srli_epi16(lo, 8);
            let hi = _mm_srli_epi16(hi, 8);
            // Pack back to u8 lanes. `_mm_packus_epi16` saturates, which
            // matches the truncated-to-u8 cast the scalar path uses.
            let out = _mm_packus_epi16(lo, hi);
            _mm_storeu_si128(ptr as *mut _, out);
        }
    }
}

/// Scalar fallback. Processes one BGRA pixel at a time using the
/// same `(v * mult + 128) >> 8` rounding as the SSE2 path so the
/// two are bit-identical when SSE2 is unavailable or disabled.
fn apply_fade_scalar_16(chunks: &mut std::slice::ChunksExactMut<'_, u8>, mult: u32) {
    for chunk in chunks {
        for pixel in chunk.chunks_exact_mut(4) {
            pixel[0] = ((u32::from(pixel[0]) * mult + 128) >> 8) as u8;
            pixel[1] = ((u32::from(pixel[1]) * mult + 128) >> 8) as u8;
            pixel[2] = ((u32::from(pixel[2]) * mult + 128) >> 8) as u8;
            pixel[3] = ((u32::from(pixel[3]) * mult + 128) >> 8) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_fade_in;

    fn fully_faded_out() -> Vec<u8> {
        // All-zero alpha region — verifies the mult=0 fast path
        // (which `fill(0)` runs instead of the SIMD loop). The fast
        // path runs when `elapsed.as_secs_f32() / fade_duration` is
        // below the `* 256.0 → 0` truncation threshold.
        let elapsed = std::time::Duration::from_micros(900); // < 1 ms
        let mut pixels = vec![200u8; 4 * 32];
        apply_fade_in(&mut pixels, elapsed);
        pixels
    }

    #[test]
    fn alpha_zero_fills_zero() {
        let out = fully_faded_out();
        assert!(
            out.iter().all(|&b| b == 0),
            "expected the mult=0 fast path to fill the buffer with 0, \
             got {out:?}"
        );
    }

    #[test]
    fn alpha_full_is_identity() {
        // Past fade_duration — early-return path. Buffer should be
        // unchanged.
        let elapsed = std::time::Duration::from_secs(10);
        let mut pixels = vec![10u8, 20, 30, 40, 50, 60, 70, 80];
        let before = pixels.clone();
        apply_fade_in(&mut pixels, elapsed);
        assert_eq!(pixels, before);
    }

    #[test]
    fn matches_scalar_byte_exact() {
        // Run the SIMD path on a varied tile, sanity-check a few
        // sampled pixels against the formula. The SSE2 path rounds
        // to `(v * mult + 128) >> 8`; the prior scalar used
        // `(v * mult) / 255`. Tolerance within ±1 on the boundaries
        // that differ between the two formulations (every
        // `mult * 256` boundary); interior pixels should match.
        let elapsed = std::time::Duration::from_millis(123); // mid-fade
        let mut pixels: Vec<u8> = (0..4 * 1024).map(|i| (i % 256) as u8).collect();
        apply_fade_in(&mut pixels, elapsed);
        for i in (0..pixels.len()).step_by(17) {
            let v = (i % 256) as u32;
            let mult = (123.0 / 500.0 * 256.0) as u32;
            let expected = ((v * mult + 128) >> 8) as u8;
            assert_eq!(
                pixels[i], expected,
                "pixel {i} mismatch: got {}, expected {expected} (v={v}, mult={mult})",
                pixels[i]
            );
        }
    }
}
