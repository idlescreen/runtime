// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! CPU stretch and letterbox upscalers.
//!
//! Per RULES.md, the stretch path is split into one-fn-per-page:
//! - [`upscale_stretch_into`] is the public entry + aligned-cast helpers.
//! - [`stretch_cache::StretchCache`] holds the per-(src_w,dst_w) column map.
//! - [`stretch_u32_rows::stretch_u32_rows`] is the aligned u32 fast path.
//! - [`stretch_byte_rows::stretch_byte_rows`] is the unaligned byte fallback.

mod letterbox;
mod sample;
mod simd;
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