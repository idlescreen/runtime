// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use wayland_client::QueueHandle;
use wayland_client::protocol::{wl_compositor, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};

use crate::frame_signal::FrameSignal;
use crate::output::OutputRegistry;

use super::super::buffer::MappedBuffer;

pub struct OutputTarget {
    pub id: u32,
    pub output: wl_output::WlOutput,
}

pub struct MonitorOverlay {
    pub surface: wl_surface::WlSurface,
    pub layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    pub width: u32,
    pub height: u32,
    pub buffers: [Option<MappedBuffer>; 2],
    pub current_buffer: usize,
    pub viewport: Option<wp_viewport::WpViewport>,
}

/// Mutable Wayland session state owned by the presenter thread.
pub struct SessionState {
    pub compositor: Option<wl_compositor::WlCompositor>,
    pub shm: Option<wl_shm::WlShm>,
    pub layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    pub seat: Option<wl_seat::WlSeat>,
    pub pointer: Option<wl_pointer::WlPointer>,
    pub pointer_serial: u32,
    pub outputs: Vec<OutputTarget>,
    pub overlays: HashMap<u32, MonitorOverlay>,
    pub appearance: Option<crate::appearance::OverlayAppearance>,
    pub screensaver_mode: bool,
    pub visible: Arc<AtomicBool>,
    pub output_registry: OutputRegistry,
    pub output_refresh_hz: HashMap<u32, u32>,
    pub output_origin: HashMap<u32, (i32, i32)>,
    pub output_mode_size: HashMap<u32, (u32, u32)>,
    pub output_scale: HashMap<u32, i32>,
    pub dismiss_grace_until: Option<Instant>,
    pub queue: QueueHandle<SessionState>,
    /// Frame-presented signal. Notified by `update_frame` after a
    /// successful `commit_frame_buffer` so the daemon's frame loop
    /// wakes from its vsync-relative sleep without 2 ms polling.
    pub frame_signal: FrameSignal,
}
