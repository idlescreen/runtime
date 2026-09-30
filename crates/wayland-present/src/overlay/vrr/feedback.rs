// SPDX-License-Identifier: MIT

//! Presentation-time protocol binding (`wp_presentation_feedback`).
//!
//! Measures presentation latency, vsync timing, and hardware refresh interval
//! to enable Adaptive Sync / Variable Refresh Rate (VRR).

use wayland_client::protocol::wl_surface;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::presentation_time::client::{wp_presentation, wp_presentation_feedback};

use super::pacing_stats::VrrPacingStats;
use crate::overlay::state::SessionState;

#[derive(Debug, Default)]
pub struct VrrFeedbackState {
    pub stats: VrrPacingStats,
    pub presentation_count: u64,
    pub discarded_count: u64,
    pub last_tv_sec: u64,
    pub last_tv_nsec: u32,
    pub last_flags: u32,
    pub zero_copy_count: u64,
}

impl VrrFeedbackState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_presented(&mut self, tv_sec: u64, tv_nsec: u32, refresh_ns: u32, flags: u32) {
        self.presentation_count = self.presentation_count.saturating_add(1);
        self.last_tv_sec = tv_sec;
        self.last_tv_nsec = tv_nsec;
        self.last_flags = flags;
        if flags & 8 != 0 {
            // ZERO_COPY flag in wp_presentation_feedback
            self.zero_copy_count = self.zero_copy_count.saturating_add(1);
        }
        self.stats.record_presentation(refresh_ns);
    }

    pub fn record_discarded(&mut self) {
        self.discarded_count = self.discarded_count.saturating_add(1);
    }
}

pub fn request_presentation_feedback(
    presentation: &wp_presentation::WpPresentation,
    surface: &wl_surface::WlSurface,
    queue: &QueueHandle<SessionState>,
) -> wp_presentation_feedback::WpPresentationFeedback {
    presentation.feedback(surface, queue, ())
}

impl Dispatch<wp_presentation::WpPresentation, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &wp_presentation::WpPresentation,
        _: wp_presentation::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_presentation_feedback::WpPresentationFeedback, ()> for SessionState {
    fn event(
        state: &mut Self,
        _: &wp_presentation_feedback::WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                seq_hi: _,
                seq_lo: _,
                flags,
            } => {
                let tv_sec = ((tv_sec_hi as u64) << 32) | (tv_sec_lo as u64);
                let raw_flags = match flags {
                    wayland_client::WEnum::Value(f) => f.bits(),
                    wayland_client::WEnum::Unknown(v) => v,
                };
                state
                    .vrr_feedback
                    .record_presented(tv_sec, tv_nsec, refresh, raw_flags);
            }
            wp_presentation_feedback::Event::Discarded => {
                state.vrr_feedback.record_discarded();
            }
            _ => {}
        }
    }
}
