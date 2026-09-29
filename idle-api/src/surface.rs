// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Platform-agnostic overlay-surface contract (Sprint 04 G3 continuation).
//!
//! Linux today ships a Wayland `zwlr-layer-shell-v1` impl via the
//! `wayland-present` crate; macOS / Windows are stubbed and return `None`
//! until Sprint 05 lands the real `NSWindow` / DXGI shims.
//!
//! Trait shape mirrors [`crate::IdleSource`]: an `is_available()` gate, a
//! `new()` constructor returning `Option<Self>` so the daemon can refuse
//! to start rather than fall back to a less-secure surface.

use std::sync::Arc;

/// A platform-specific overlay surface that hosts a BGRA screensaver frame.
///
/// `new()` returns `None` when the platform surface is unavailable (no
/// Wayland compositor, no NSWindow, no DXGI output). Callers must treat
/// `None` as a hard refusal.
pub trait OverlaySurface: Send + Sync + 'static {
    /// True when the implementation can attach to its platform surface in
    /// this environment (e.g. `WAYLAND_DISPLAY` is set on Linux).
    fn is_available() -> bool
    where
        Self: Sized;

    /// Attach to the platform overlay surface and begin presenting.
    ///
    /// Returns `None` when the surface is unavailable.
    fn new() -> Option<Self>
    where
        Self: Sized;

    /// Submit a per-output BGRA frame for presentation. Frame buffer is
    /// `width * height * 4` bytes; row-major, BGRA.
    ///
    /// The buffer is wrapped in `Arc<Vec<u8>>` so the presenter can
    /// hold it across one Wayland commit without copying bytes at
    /// the trait seam. The recycler is the presenter's triple-
    /// buffer pool (replaces the prior `mpsc::Sender<Vec<u8>>`
    /// round-trip with a single atomic bump on `Arc::clone`).
    fn submit_frame(&self, output: OutputId, frame: Arc<Vec<u8>>, width: u32, height: u32);

    /// True when the surface is still attached and rendering. Returns
    /// `false` to signal the daemon that the surface is gone (compositor
    /// restart, display unplug) and the host needs to re-attach.
    fn is_alive(&self) -> bool;

    /// True when a screensaver frame is currently displayed. The stub
    /// returns `false`; the Wayland adapter forwards to the presenter.
    fn is_visible(&self) -> bool;

    /// Begin presenting a solid-color "screen-blank" appearance. Used when
    /// no saver is active. Stub: no-op.
    fn show_blank(&self, _appearance: BlankAppearance);

    /// Begin presenting the screensaver surface. Stub: no-op.
    fn show_screensaver(&self);

    /// Stop presenting. Stub: no-op.
    fn hide(&self);

    /// True when the surface supports hardware-scaled frame presentation.
    /// Used by the frame loop to choose GPU upscale vs CPU upscale.
    /// Stub: returns `false`.
    fn supports_scaling(&self) -> bool;

    /// Snapshot of the platform's logical outputs. Stub: empty.
    fn output_layouts(&self) -> Vec<OutputLayout>;

    /// Allocate a frame buffer the presenter can write into. The returned
    /// `Vec<u8>` is BGRA, `width * height * 4` bytes. Used by the frame
    /// loop's two-buffer ping-pong. Stub: returns an empty `Vec<u8>` of
    /// the requested length (the host never reads it).
    fn get_frame_buffer(&self, size: usize) -> Vec<u8> {
        vec![0u8; size]
    }

    /// Frame-presence signal surfaced by the surface. The daemon's
    /// frame loop waits on this instead of polling a 2 ms slice;
    /// stub surfaces return `None` and the loop falls back to its
    /// legacy slice-poll path.
    ///
    /// Notified on every successful frame commit. When
    /// `wayland_present`'s `wl_callback::done` dispatch is wired,
    /// it'll also be notified on actual vsync.
    fn frame_signal(&self) -> Option<wayland_present::FrameSignal> {
        None
    }
}

/// Solid-color "screen blank" appearance. Real impls translate this to
/// platform-specific surface config; the stub ignores it.
#[derive(Debug, Clone, Copy)]
pub struct BlankAppearance {
    pub color: [u8; 4],
}

impl Default for BlankAppearance {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
        }
    }
}

/// Per-output layout. Stub returns empty.
#[derive(Debug, Clone, Copy)]
pub struct OutputLayout {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub scale: i32,
}

/// Stable identifier for a logical output (monitor). Implementations map
/// this to whatever native id the platform uses (Wayland output, NSScreen,
/// DXGI_OUTPUT_DESC, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputId(pub u32);

/// Stub surface for non-Linux targets. Always reports dead so the daemon
/// refuses to start until Sprint 05 lands real macOS / Windows impls.
pub struct StubOverlay;

impl OverlaySurface for StubOverlay {
    fn is_available() -> bool {
        false
    }

    fn new() -> Option<Self> {
        Some(Self)
    }

    fn submit_frame(&self, _output: OutputId, _frame: Arc<Vec<u8>>, _width: u32, _height: u32) {
        // No-op: the stub never presents. Real impls forward to the
        // platform's compositor / window system.
    }

    fn is_alive(&self) -> bool {
        false
    }

    fn is_visible(&self) -> bool {
        false
    }

    fn show_blank(&self, _appearance: BlankAppearance) {}

    fn show_screensaver(&self) {}

    fn hide(&self) {}

    fn supports_scaling(&self) -> bool {
        false
    }

    fn output_layouts(&self) -> Vec<OutputLayout> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;

/// Default `OutputLayout` for test fixtures and stub returns. `scale=1`
/// matches the Wayland compositor default.
impl Default for OutputLayout {
    fn default() -> Self {
        Self {
            id: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            refresh_mhz: 60,
            scale: 1,
        }
    }
}
