// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! CPU stretch and letterbox upscalers.
//!
//! Per RULES.md, the stretch + bilinear paths are split into
//! one-fn-per-page. The stretch pages are `upscale_stretch_into`,
//! `stretch_cache`, `stretch_u32_rows`, `stretch_byte_rows` (+ a
//! sibling `stretch_byte_rows_tests.rs`). The bilinear pages are
//! `bilinear_row` (public entry + scalar fallback) +
//! `bilinear_avx2` + `bilinear_neon` (per-arch SIMD
//! implementations), each paired with a sibling
//! `*_tests.rs` for QA + bench.

mod bilinear_avx2;
mod bilinear_neon;
mod bilinear_row;
mod letterbox;
mod sample;
mod stretch_byte_rows;
mod stretch_cache;
mod stretch_u32_rows;
mod upscale_stretch_into;

pub use letterbox::upscale_letterbox_into;
pub use stretch_cache::StretchCache;
pub use upscale_stretch_into::upscale_stretch_into;

#[cfg(test)]
#[path = "../cpu_tests.rs"]
mod tests;
