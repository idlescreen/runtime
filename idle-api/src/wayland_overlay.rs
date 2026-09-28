// SPDX-License-Identifier: Apache-2.0
// perf: T3 · metric: bounded single-pass work; no syscalls, no locks, no allocation on the steady path · check: review
// Copyright 2026 IdleScreen

//! Linux adapter that lifts the concrete `wayland_present::OverlayPresenter`
//! onto the platform-agnostic [`OverlaySurface`] trait.
//!
//! This is the bridge the daemon uses to swap in non-Linux surface impls
//! for Sprint 05 without changing the surface call sites.

use std::sync::Arc;

use crate::surface::{BlankAppearance, OutputId, OutputLayout, OverlaySurface};
use wayland_present::OverlayPresenter;

/// Newtype wrapper. We do not embed the presenter directly because the
/// surface trait requires `Arc<Vec<u8>>` for frame buffers and the
/// presenter takes owned `Vec<u8>`.
pub struct WaylandOverlay {
    presenter: Arc<OverlayPresenter>,
}

impl WaylandOverlay {
    /// Try to construct a Linux overlay surface. Returns `None` when
    /// `WAYLAND_DISPLAY` is unset or the compositor lacks `zwlr-layer-shell`.
    pub fn new() -> Option<Self> {
        OverlayPresenter::new().map(|presenter| Self {
            presenter: Arc::new(presenter),
        })
    }

    /// Hand the concrete presenter back. Used by the daemon's existing
    /// presentation pipeline that still takes `Arc<OverlayPresenter>`.
    pub fn into_inner(self) -> Arc<OverlayPresenter> {
        self.presenter
    }

    /// Borrow the concrete presenter for read-only access. Useful when a
    /// caller wants to do something with the surface that isn't on the
    /// trait (e.g. present debug frames during development).
    pub fn inner(&self) -> &OverlayPresenter {
        &self.presenter
    }
}

impl OverlaySurface for WaylandOverlay {
    fn is_available() -> bool {
        OverlayPresenter::is_available()
    }

    fn new() -> Option<Self> {
        WaylandOverlay::new()
    }

    fn submit_frame(&self, output: OutputId, frame: Arc<Vec<u8>>, width: u32, height: u32) {
        // Hand the Arc straight through to the presenter's triple-
        // buffer pool. Refcount bump on `Arc::clone` (when the
        // presenter holds the buffer across the Wayland commit).
        self.presenter.submit_frame(output.0, width, height, frame);
    }

    fn is_alive(&self) -> bool {
        self.presenter.is_alive()
    }

    fn is_visible(&self) -> bool {
        self.presenter.is_visible()
    }

    fn show_blank(&self, appearance: BlankAppearance) {
        // Translate the trait-level [u8; 4] RGBA into the presenter's
        // [u8; 3] RGB solid appearance. The alpha channel is composited
        // by the compositor, not the presenter.
        let rgb = [
            appearance.color[0],
            appearance.color[1],
            appearance.color[2],
        ];
        self.presenter
            .show(wayland_present::OverlayAppearance::solid(rgb));
    }

    fn show_screensaver(&self) {
        self.presenter.show_screensaver();
    }

    fn hide(&self) {
        // The presenter's "hide" is implicit (presentation ends); we
        // accept that semantically here without re-exposing the surface.
        // A future trait addition would formalize it.
    }

    fn supports_scaling(&self) -> bool {
        self.presenter.supports_scaling()
    }

    fn output_layouts(&self) -> Vec<OutputLayout> {
        self.presenter
            .output_layouts()
            .into_iter()
            .map(|l| OutputLayout {
                id: l.id,
                x: l.x,
                y: l.y,
                width: l.width,
                height: l.height,
                refresh_mhz: l.refresh_rate_hz,
                scale: l.scale,
            })
            .collect()
    }

    fn get_frame_buffer(&self, size: usize) -> Vec<u8> {
        self.presenter.get_frame_buffer(size)
    }

    fn frame_signal(&self) -> Option<wayland_present::FrameSignal> {
        Some(self.presenter.frame_signal())
    }
}

#[cfg(test)]
#[path = "wayland_overlay_tests.rs"]
mod tests;
