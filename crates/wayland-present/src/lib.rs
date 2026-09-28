// SPDX-License-Identifier: Apache-2.0
// perf: T3 · metric: contains unsafe; cost depends on what the caller passes in · check: test
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

// Re-exports for `benches/hot_path.rs`.
//
// Every module in this crate is private, so a `[[bench]]` target —
// which compiles as its own crate — can only reach the four items
// above. The T2 pages (`frame_pool`, `drop_presenter`, `overlay::epoll`)
// each name a bench target in their `// perf:` label, and the CI
// linter checks the bench source actually exercises them; without this
// seam those labels would be unfalsifiable.
//
// `#[doc(hidden])` keeps it out of the rendered docs: it is a
// measurement seam, not public API. Mirrors the convention in
// `savers/ripple/src/lib.rs::bench_exports` and
// `idle-upscaler/src/cpu/mod.rs::bench_exports`.
#[doc(hidden)]
pub mod bench_exports {
    pub use crate::drop_presenter::bench_exports::*;
    pub use crate::frame_pool::{
        FramePool, empty_frame_pool, get_frame_buffer, return_frame_buffer,
    };
    pub use crate::overlay::bench_exports::*;
}

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
