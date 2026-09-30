// SPDX-License-Identifier: MIT

//! GPU hardware acceleration and compute stretch pass.
//!
//! Provides compute shader upscaling for high-resolution displays.
//! Gracefully downgrades to CPU upscaling when GPU compute is unavailable.
//! Includes WGSL shaders and GPU capability detection.
//! Supports bilinear and nearest-neighbor filtering.
//! Preserves frame presentation stability.

pub mod compute;

#[cfg(test)]
mod tests;

pub use compute::{COMPUTE_STRETCH_WGSL, GpuComputeStretch};
