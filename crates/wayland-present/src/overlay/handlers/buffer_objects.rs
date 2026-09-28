// SPDX-License-Identifier: MIT

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_buffer, wl_callback, wl_compositor, wl_shm, wl_shm_pool, wl_surface},
};

use super::super::state::SessionState;

impl Dispatch<wl_compositor::WlCompositor, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, u32> for SessionState {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for SessionState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The compositor fires `done` after it has presented the frame
        // (or after a configured timeout — Wayland spec). This is the
        // true-vsync wakeup the daemon's frame loop waits on: the
        // round-trip from `surface.frame(queue, ())` to this dispatch
        // covers one frame period on the compositor.
        //
        // We bump the same `frame_signal` generation that
        // `state::overlay::OverlayState::update_frame` bumps on
        // commit. Either path advances the predicate; consumers
        // (daemon frame loop) coalesce — only one iteration runs per
        // frame regardless of how many notifies fired.
        //
        // We deliberately match `wl_callback::Event::Done { .. }` and
        // ignore the others — the spec only defines `done` and that's
        // all wayland-client delivers.
        if let wl_callback::Event::Done { .. } = event {
            state.frame_signal.notify();
        }
    }
}
