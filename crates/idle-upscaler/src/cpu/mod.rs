// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! CPU stretch and letterbox upscalers.

mod letterbox;
mod sample;
mod simd;
mod stretch;

pub use letterbox::upscale_letterbox_into;
pub use stretch::{StretchCache, upscale_stretch_into};

#[cfg(test)]
#[path = "../cpu_tests.rs"]
mod tests;
