// SPDX-License-Identifier: MIT

use std::sync::atomic::Ordering;

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_keyboard, wl_pointer, wl_seat},
};

use super::super::state::SessionState;

impl Dispatch<wl_seat::WlSeat, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for SessionState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { serial, .. } => {
                state.pointer_serial = serial;
                if state.visible.load(Ordering::SeqCst) {
                    state.hide_pointer(serial);
                }
            }
            wl_pointer::Event::Motion { .. } => {
                // Within the 1-second grace window after begin_presentation,
                // dismiss_from_input() is a no-op (see state/mod.rs). We
                // still hide the cursor immediately so the user sees the
                // screensaver without a visible pointer hovering over it;
                // the grace window only suppresses the *dismiss* action.
                if state.visible.load(Ordering::SeqCst) && state.pointer_serial != 0 {
                    state.hide_pointer(state.pointer_serial);
                }
                state.dismiss_from_input();
            }
            wl_pointer::Event::Button { .. } => {
                state.dismiss_from_input();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for SessionState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key { .. } = event {
            state.dismiss_from_input();
        }
    }
}
