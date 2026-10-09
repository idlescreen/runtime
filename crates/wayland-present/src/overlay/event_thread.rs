// SPDX-License-Identifier: MIT

//! Overlay event thread: spawn + event loop.
//! Per-resource helpers live in sibling modules
//! ([`super::command`], [`super::epoll`]).

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use wayland_client::Connection;

use crate::frame_signal::FrameSignal;
use crate::output::OutputRegistry;

use super::command::PresenterCommand;
use super::epoll::{drain_eventfd, epoll_ctl_add, make_epoll};
use super::error_utils::is_wayland_would_block;
use super::state::SessionState;

#[allow(clippy::too_many_arguments)]
pub fn spawn_event_thread(
    ready_tx: Sender<Result<(), &'static str>>,
    command_rx: Receiver<PresenterCommand>,
    visible: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    outputs: OutputRegistry,
    is_alive: Arc<AtomicBool>,
    supports_scaling: Arc<AtomicBool>,
    wake_rx: std::os::fd::OwnedFd,
    frame_signal: FrameSignal,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if let Err(message) = run_event_loop(
            ready_tx,
            command_rx,
            visible,
            shutdown,
            outputs,
            supports_scaling,
            wake_rx,
            frame_signal,
        ) {
            idle_log::error!(
                fault = message,
                "wayland-present: event thread exiting — is_alive=false (daemon should recover without process exit)"
            );
        }
        is_alive.store(false, Ordering::SeqCst);
        idle_log::warn!("wayland-present: event thread stopped (is_alive=false)");
    })
}

#[allow(clippy::needless_pass_by_value)]
fn run_event_loop(
    ready_tx: Sender<Result<(), &'static str>>,
    command_rx: Receiver<PresenterCommand>,
    visible: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    outputs: OutputRegistry,
    supports_scaling: Arc<AtomicBool>,
    wake_rx: std::os::fd::OwnedFd,
    frame_signal: FrameSignal,
) -> Result<(), &'static str> {
    let connection = match Connection::connect_to_env() {
        Ok(conn) => conn,
        Err(_) => {
            let _ = ready_tx.send(Err("failed to connect to Wayland"));
            return Err("failed to connect to Wayland");
        }
    };

    let mut event_queue = connection.new_event_queue();
    let queue = event_queue.handle();
    let _registry = connection.display().get_registry(&queue, ());

    let mut state = SessionState {
        compositor: None,
        shm: None,
        layer_shell: None,
        xdg_wm_base: None,
        viewporter: None,
        presentation: None,
        vrr_feedback: crate::overlay::vrr::VrrFeedbackState::new(),
        linux_dmabuf: None,
        dmabuf_pool: crate::overlay::dmabuf::DmaBufPool::new(3),
        color_manager: None,
        color_state: crate::overlay::color::ColorManagementState::new(),
        seat: None,
        pointer: None,
        pointer_serial: 0,
        outputs: Vec::new(),
        overlays: HashMap::new(),
        appearance: None,
        screensaver_mode: false,
        visible,
        output_registry: outputs,
        output_refresh_hz: HashMap::new(),
        output_origin: HashMap::new(),
        output_mode_size: HashMap::new(),
        output_scale: HashMap::new(),
        dismiss_grace_until: None,
        queue: queue.clone(),
        frame_signal,
    };

    event_queue
        .roundtrip(&mut state)
        .map_err(|_| "initial registry roundtrip failed")?;

    if state.viewporter.is_some() {
        supports_scaling.store(true, Ordering::SeqCst);
    }

    if state.layer_shell.is_none() && state.xdg_wm_base.is_none() {
        let _ = ready_tx.send(Err("compositor lacks zwlr_layer_shell_v1 and xdg_wm_base"));
        return Err("compositor lacks zwlr_layer_shell_v1 and xdg_wm_base");
    }

    if state.compositor.is_none() || state.shm.is_none() {
        let _ = ready_tx.send(Err("compositor missing wl_compositor or wl_shm"));
        return Err("compositor missing wl_compositor or wl_shm");
    }

    let _ = ready_tx.send(Ok(()));

    let wayland_fd = connection.as_fd().as_raw_fd();
    let wake_fd = wake_rx.as_raw_fd();

    // Epoll fd: Wayland socket and wake_rx are registered once.
    let epoll_fd = make_epoll()?;
    epoll_ctl_add(epoll_fd, wayland_fd, libc::EPOLLIN, 1)?;
    epoll_ctl_add(epoll_fd, wake_fd, libc::EPOLLIN, 2)?;

    // 4 slots is plenty — only 2 fds registered but one roundtrip can
    // produce multiple events for the same fd.
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 4];

    while !shutdown.load(Ordering::Relaxed) {
        let _ = connection.flush();
        // SAFETY: events array is a valid 4-element stack buffer; epoll_wait
        // writes at most events.len() entries.
        let n = unsafe {
            libc::epoll_wait(
                epoll_fd,
                events.as_mut_ptr(),
                events.len() as libc::c_int,
                100,
            )
        };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                idle_log::error!(error = %err, "wayland-present: epoll_wait failed");
                return Err("epoll_wait failed");
            }
            continue;
        }
        for &ev in &events[..n as usize] {
            match ev.u64 {
                1 => {
                    // Wayland socket readable. The dispatch helper
                    // handles prepare_read / read / dispatch_pending.
                    dispatch_pending_events(&connection, &mut event_queue, &mut state)?;
                }
                2 => {
                    // Self-wake eventfd: drain so the counter doesn't
                    // overflow (eventfd is a u64 and we read until
                    // EAGAIN). Then drop into `apply_commands` below.
                    drain_eventfd(wake_fd);
                }
                _ => {}
            }
        }
        apply_commands(&mut state, &command_rx);
    }

    state.hide();
    Ok(())
}

/// Dispatch any Wayland events arrived since the last loop iteration.
fn dispatch_pending_events(
    connection: &Connection,
    event_queue: &mut wayland_client::EventQueue<SessionState>,
    state: &mut SessionState,
) -> Result<(), &'static str> {
    if let Some(guard) = event_queue.prepare_read() {
        let _ = connection.flush();
        match guard.read() {
            Ok(_) => {}
            Err(e) if is_wayland_would_block(&e) => {
                // Another path already drained the socket, or nothing left
                // to read. Not fatal — dispatch whatever is pending.
                idle_log::trace!(
                    error = %e,
                    "wayland-present: read WouldBlock/EAGAIN; continuing"
                );
            }
            Err(e) => {
                idle_log::error!(
                    error = %e,
                    "wayland-present: failed to read Wayland events (compositor may have closed the connection; often a protocol error on the previous commit)"
                );
                return Err("failed to read Wayland events");
            }
        }
        if let Err(e) = event_queue.dispatch_pending(state) {
            idle_log::error!(error = %e, "wayland-present: failed to dispatch Wayland events");
            return Err("failed to dispatch Wayland events");
        }
    } else if let Err(e) = event_queue.dispatch_pending(state) {
        idle_log::error!(error = %e, "wayland-present: failed to dispatch Wayland events");
        return Err("failed to dispatch Wayland events");
    }

    Ok(())
}

/// Drain pending commands from the daemon-side channel and apply each.
fn apply_commands(state: &mut SessionState, command_rx: &Receiver<PresenterCommand>) {
    while let Ok(command) = command_rx.try_recv() {
        match command {
            PresenterCommand::ShowSolid(appearance) => state.show_solid(appearance),
            PresenterCommand::ShowScreensaver => state.show_screensaver(),
            PresenterCommand::UpdateFrame {
                output_id,
                width,
                height,
                pixels,
                return_pool,
            } => {
                state.update_frame(output_id, width, height, &pixels);
                // Push the Arc back to the daemon's pool.
                crate::frame_pool::return_frame_buffer(&return_pool, pixels);
            }
            PresenterCommand::Hide => state.hide(),
        }
    }
}

// Tests + bench live in `event_thread_tests.rs`.
#[cfg(test)]
#[path = "event_thread_tests.rs"]
mod tests;
