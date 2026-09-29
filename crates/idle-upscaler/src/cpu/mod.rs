// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! CPU stretch and letterbox upscalers.
//!
//! The stretch and bilinear paths are split into separate modules.
//! The stretch pages are `upscale_stretch_into`,
//! `stretch_cache`, `stretch_u32_rows`, `stretch_byte_rows` (+ a
//! sibling `stretch_byte_rows_tests.rs`). The bilinear pages are
//! `bilinear_row` (public entry + scalar fallback) +
//! `bilinear_avx2` + `bilinear_neon` (per-arch SIMD
//! implementations), each paired with a sibling
//! `*_tests.rs` for QA + bench.

pub mod bilinear;
mod letterbox;
mod sample;
pub mod stretch;

pub use letterbox::upscale_letterbox_into;
pub use stretch::{StretchCache, upscale_stretch_into};

// Measurement seam, re-exported to `lib.rs::bench_exports`.
//
// It has to live *here* rather than in `lib.rs`: Rust privacy flows
// downward, so a private submodule is visible to itself and its
// descendants only. `lib` is an ancestor of `cpu`, not a descendant,
// so `lib.rs` cannot name `bilinear_row::bilinear_row` at all. From
// inside `cpu` the `super::` paths below resolve — and because they
// sit in a nested module rather than directly in `cpu`'s namespace,
// they do not collide with the identically-named submodules above.
#[doc(hidden)]
pub mod bench_exports {
    #[cfg(target_arch = "x86_64")]
    pub use super::bilinear::avx2::bilinear_row_avx2;
    pub use super::bilinear::bilinear_row;
    pub use super::letterbox::upscale_letterbox_into;
    pub use super::stretch::byte_rows::stretch_byte_rows;
    pub use super::stretch::cache::StretchCache;
    pub use super::stretch::u32_rows::stretch_u32_rows;
    pub use super::stretch::upscale_stretch_into;
}

#[cfg(test)]
#[path = "../cpu_tests.rs"]
mod tests;
