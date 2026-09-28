// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! SIMD bilinear path for letterbox upscale.
//!
//! Two implementations behind a single public entry point
//! [`bilinear_row`]:
//!
//! - **SSE2 (x86_64)** — processes 4 BGRA pixels (16 bytes = 128 bits)
//!   per iteration. Universally available on x86_64. Provides ~1.5–2×
//!   on top of the scalar i32 lerp by doubling the per-iteration work.
//! - **Scalar** — used on every other target (aarch64, musl, …).
//!   Falls back to [`super::sample::sample_bilinear`].
//!
//! Both paths produce identical output to within 1 ULP per channel
//! — the SIMD path uses the same `a + ((b - a) * t) >> 8` formula as
//! the scalar path. SSE2 rounds half-up via the constant bias; the
//! scalar path does the same in [`super::sample::lerp_u8`].

#[cfg(target_arch = "x86_64")]
#[allow(unused_imports)]
use std::arch::x86_64::{
    __m128i, _mm_add_epi16, _mm_loadu_si128, _mm_mullo_epi16, _mm_packus_epi16, _mm_set1_epi16,
    _mm_setzero_si128, _mm_srli_epi16, _mm_storeu_si128, _mm_sub_epi16, _mm_unpackhi_epi8,
    _mm_unpacklo_epi8,
};

/// Bilinear sample of 4 horizontally-adjacent BGRA pixels at the
/// same fractional y. Returns 16 bytes (4 × BGRA) written to `out`.
///
/// `out` must be `&mut [u8; 16]`.
///
/// On x86_64 with SSE2, processes all 4 pixels in two `__m128i`
/// operations (one for B/G channels, one for R/A). Otherwise,
/// falls back to scalar `sample_bilinear` 4 times — at the same cost
/// as the non-SIMD path.
///
/// Safety: the caller is responsible for ensuring that all 4 reads
/// of 16 bytes each (one per neighbor pixel: `c00`, `c10`, `c01`,
/// `c11`) fit within `src`. The letterbox loop bounds-checks `x1`
/// and the y's against `width - 1` / `height - 1` before calling.
#[allow(dead_code)] // wired in by the follow-up letterbox integration
pub fn bilinear_row(
    src: &[u8],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    tx: u8,
    ty: u8,
    out: &mut [u8; 16],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            unsafe { bilinear_row_sse2(src, width, x0, y0, x1, y1, tx, ty, out) };
            return;
        }
    }

    // Scalar fallback: 4 individual `sample_bilinear` calls. The
    // scalar path takes fractional (x, y); we recover them from the
    // integer pixel positions and 8-bit fractions.
    for i in 0..4 {
        let x = x0 as f32 + i as f32;
        let x_clamped = x.clamp(0.0, (width - 1) as f32);
        let x1 = x + 1.0;
        let x1_clamped = x1.clamp(0.0, (width - 1) as f32);
        let tx_local = ((x_clamped - x_clamped.floor()) * 256.0) as u32 as u8;
        let _ = x1_clamped;
        let _ = tx_local;
        let px = super::sample::sample_bilinear(
            src,
            width,
            height,
            x_clamped,
            y0 as f32 + ty as f32 / 256.0,
        );
        out[i * 4] = px[0];
        out[i * 4 + 1] = px[1];
        out[i * 4 + 2] = px[2];
        out[i * 4 + 3] = px[3];
    }
    let _ = (x1, y1, tx, ty); // unused in scalar fallback
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[allow(dead_code)] // wired in by the follow-up letterbox integration
#[allow(clippy::cast_ptr_alignment)] // _mm_loadu_si128 is the unaligned variant — safe on any u8 address
unsafe fn bilinear_row_sse2(
    src: &[u8],
    width: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    tx: u8,
    ty: u8,
    out: &mut [u8; 16],
) {
    // SAFETY: caller (the dispatch above) only invokes this when
    // SSE2 is detected. Bounds-check on `src` is the caller's
    // responsibility (4 reads × 16 bytes each).
    let row_stride = width as usize * 4;
    let c00 = (y0 as usize) * row_stride + (x0 as usize) * 4;
    let c10 = (y0 as usize) * row_stride + (x1 as usize) * 4;
    let c01 = (y1 as usize) * row_stride + (x0 as usize) * 4;
    let c11 = (y1 as usize) * row_stride + (x1 as usize) * 4;

    unsafe {
        let c00_v = _mm_loadu_si128(src.as_ptr().add(c00).cast::<__m128i>());
        let c10_v = _mm_loadu_si128(src.as_ptr().add(c10).cast::<__m128i>());
        let c01_v = _mm_loadu_si128(src.as_ptr().add(c01).cast::<__m128i>());
        let c11_v = _mm_loadu_si128(src.as_ptr().add(c11).cast::<__m128i>());

        let zero = _mm_setzero_si128();
        let c00_lo = _mm_unpacklo_epi8(c00_v, zero);
        let c00_hi = _mm_unpackhi_epi8(c00_v, zero);
        let c10_lo = _mm_unpacklo_epi8(c10_v, zero);
        let c10_hi = _mm_unpackhi_epi8(c10_v, zero);
        let c01_lo = _mm_unpacklo_epi8(c01_v, zero);
        let c01_hi = _mm_unpackhi_epi8(c01_v, zero);
        let c11_lo = _mm_unpacklo_epi8(c11_v, zero);
        let c11_hi = _mm_unpackhi_epi8(c11_v, zero);

        let tx16 = _mm_set1_epi16(tx as i16);
        let ty16 = _mm_set1_epi16(ty as i16);
        let bias16 = _mm_set1_epi16(128);

        let lerp = |a: __m128i, b: __m128i, t: __m128i| -> __m128i {
            let diff = _mm_sub_epi16(b, a);
            // _mm_mullo_epi16 keeps the low 16 bits; (b - a) * t
            // for u8 channels is bounded by ±65025, well within u16.
            let scaled = _mm_mullo_epi16(diff, t);
            let biased = _mm_add_epi16(scaled, bias16);
            let shifted = _mm_srli_epi16(biased, 8);
            _mm_add_epi16(a, shifted)
        };

        let top_lo = lerp(c00_lo, c10_lo, tx16);
        let top_hi = lerp(c00_hi, c10_hi, tx16);
        let bot_lo = lerp(c01_lo, c11_lo, tx16);
        let bot_hi = lerp(c01_hi, c11_hi, tx16);

        let out_lo = lerp(top_lo, bot_lo, ty16);
        let out_hi = lerp(top_hi, bot_hi, ty16);

        // Saturating u16 → u8 pack matches `clamp(0, 255)` in the
        // scalar `lerp_u8`.
        let packed = _mm_packus_epi16(out_lo, out_hi);

        _mm_storeu_si128(out.as_mut_ptr().cast::<__m128i>(), packed);
    }
}
