// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Fullscreen Wayland overlays using [`zwlr_layer_shell_v1`].
//!
//! [`OverlayPresenter`] draws layer-shell surfaces above application windows.
//! Solid-color fills and screensaver frames share the same presenter thread and
//! output registry so multi-monitor layouts stay consistent across configure
//! events and refresh-rate reporting.
//!
//! Consumers submit per-output BGRA frames via [`OverlayPresenter::submit_frame`];
//! the overlay thread attaches SHM buffers and marks damage per monitor.
//!
//! [`zwlr_layer_shell_v1`]: https://wayland.app/protocols/wlr-layer-shell-unstable-v1
//!
//! Requires a compositor that implements wlr-layer-shell (COSMIC, Sway, Hyprland, etc.).

mod appearance;
mod drop_presenter;
mod frame_pool;
mod frame_signal;
mod output;
mod overlay;
mod presenter;

pub use appearance::OverlayAppearance;
pub use frame_signal::{FrameSignal, FrameWaitOutcome};
pub use output::OutputLayout;
pub use presenter::OverlayPresenter;

// Presenter commands are processed on a dedicated Wayland thread.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability_and_fallback() {
        let backup = std::env::var("WAYLAND_DISPLAY").ok();

        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
        assert!(!OverlayPresenter::is_available());
        assert!(OverlayPresenter::new().is_none());

        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", "wayland-mock-test-0");
        }
        assert!(OverlayPresenter::is_available());

        if let Some(val) = backup {
            unsafe {
                std::env::set_var("WAYLAND_DISPLAY", val);
            }
        } else {
            unsafe {
                std::env::remove_var("WAYLAND_DISPLAY");
            }
        }
    }
}
