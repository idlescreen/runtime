// SPDX-License-Identifier: MIT

//! Frame rendering, pacing, fade-in transitions, and presentation loop.

pub mod apply_fade_in;
pub mod frame_loop;
#[cfg(test)]
mod frame_loop_tests;
pub mod frame_pacing;
pub mod present_frame;

pub use apply_fade_in::apply_fade_in;
pub use frame_loop::{ActiveSession, FrameLoopState, run_frame_loop};
pub use present_frame::present_frame;
