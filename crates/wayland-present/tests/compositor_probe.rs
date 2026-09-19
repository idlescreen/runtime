// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Live-compositor probe: connects to the real Wayland session when one is
//! present and verifies the globals the overlay path depends on
//! (`zwlr_layer_shell_v1`, `wl_compositor`, `wl_shm`). Creates no surfaces —
//! a registry read only — so it is safe on a running desktop. Skips cleanly
//! on headless CI (`WAYLAND_DISPLAY` unset).

use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

struct Globals(Vec<String>);

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = event {
            state.0.push(interface);
        }
    }
}

#[test]
fn real_compositor_exposes_overlay_globals() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("SKIP: WAYLAND_DISPLAY unset — no live compositor to probe");
        return;
    }

    let conn = Connection::connect_to_env()
        .expect("WAYLAND_DISPLAY is set but connect_to_env failed");
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());

    let mut globals = Globals(Vec::new());
    event_queue
        .roundtrip(&mut globals)
        .expect("registry roundtrip failed against live compositor");

    for required in ["zwlr_layer_shell_v1", "wl_compositor", "wl_shm"] {
        assert!(
            globals.0.iter().any(|i| i == required),
            "live compositor is missing required global {required} — got: {:?}",
            globals.0
        );
    }
}
