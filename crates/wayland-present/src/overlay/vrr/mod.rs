// SPDX-License-Identifier: MIT

//! Variable Refresh Rate (VRR / Adaptive Sync) support via `wp_presentation_time`.
//!
//! Exposes presentation feedback timings, refresh intervals, hardware clocks,
//! and adaptive frame-pacing telemetry for screensaver rendering.
//! Enables dynamic frame pacing transitions.

pub mod feedback;
pub mod pacing_stats;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use feedback::{VrrFeedbackState, request_presentation_feedback};
#[allow(unused_imports)]
pub use pacing_stats::{RefreshMode, VrrPacingStats};
