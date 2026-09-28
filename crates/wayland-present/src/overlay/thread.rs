// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::os::fd::AsFd;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use wayland_client::Connection;

use crate::appearance::OverlayAppearance;
use crate::frame_signal::FrameSignal;
use crate::output::OutputRegistry;

use super::error_utils::is_wayland_would_block;
use super::state::SessionState;

pub enum PresenterCommand {
    ShowSolid(OverlayAppearance),
    ShowScreensaver,
    UpdateFrame {
        output_id: u32,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        return_pool: Sender<Vec<u8>>,
    },
    Hide,
}

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
        viewporter: None,
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

    if state.layer_shell.is_none() {
        let _ = ready_tx.send(Err("compositor does not expose zwlr_layer_shell_v1"));
        return Err("compositor does not expose zwlr_layer_shell_v1");
    }

    if state.compositor.is_none() || state.shm.is_none() {
        let _ = ready_tx.send(Err("compositor missing wl_compositor or wl_shm"));
        return Err("compositor missing wl_compositor or wl_shm");
    }

    let _ = ready_tx.send(Ok(()));

    let wayland_fd = connection.as_fd().as_raw_fd();
    let wake_fd = wake_rx.as_raw_fd();

    // Tier-2 step 3 (perf plan §"Wayland epoll"): replace the per-iteration
    // `libc::poll(2, 100ms)` with an epoll fd. The Wayland socket and the
    // self-wake eventfd (`wake_rx`, written by `submit_frame` et al.) are
    // registered once. epoll scales to any number of fds without
    // per-poll allocation and gives us a single syscall to wake on either
    // fd. The 100 ms epoll_wait timeout is a safety net for the rare
    // case where neither fd fires (e.g. compositor restart mid-poll).
    let epoll_fd = make_epoll()?;
    // SAFETY: epoll_ctl_add wraps the unsafe ctl call and validates the
    // return code.
    epoll_ctl_add(epoll_fd, wayland_fd, libc::EPOLLIN, 1)?;
    epoll_ctl_add(epoll_fd, wake_fd, libc::EPOLLIN, 2)?;

    // SAFETY: small stack array for epoll_wait. 4 slots is plenty — we
    // only have 2 fds registered but a single roundtrip can produce
    // multiple events for the same fd.
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 4];

    while !shutdown.load(Ordering::Relaxed) {
        let _ = connection.flush();
        // SAFETY: `events` is a valid 4-element array, lifetime tied to
        // this stack frame; `epoll_wait` writes at most `events.len()`
        // entries.
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

/// Create a new epoll fd. CLOEXEC so a child fork doesn't inherit it.
fn make_epoll() -> Result<libc::c_int, &'static str> {
    // SAFETY: epoll_create1 with EPOLL_CLOEXEC returns a fresh fd.
    let raw = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if raw < 0 {
        idle_log::error!(
            error = %std::io::Error::last_os_error(),
            "wayland-present: epoll_create1 failed"
        );
        return Err("epoll_create1 failed");
    }
    Ok(raw)
}

/// Add `fd` to the epoll set with the given event mask and a stable
/// tag (the `u64` slot in `epoll_event`). The tag lets us route the
/// wakeup to the right handler in the event loop.
fn epoll_ctl_add(
    epoll_fd: libc::c_int,
    fd: libc::c_int,
    mask: libc::c_int,
    tag: u64,
) -> Result<(), &'static str> {
    let mut event = libc::epoll_event {
        events: mask as u32,
        u64: tag,
    };
    // SAFETY: `event` is a valid `epoll_event` struct; `fd` is a valid
    // descriptor (we own it or it is the Wayland socket lifetime).
    let rc = unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) };
    if rc < 0 {
        idle_log::error!(
            error = %std::io::Error::last_os_error(),
            fd,
            "wayland-present: epoll_ctl ADD failed"
        );
        return Err("epoll_ctl ADD failed");
    }
    Ok(())
}

/// Drain an eventfd by reading 8 bytes (the counter) until EAGAIN.
fn drain_eventfd(fd: libc::c_int) {
    let mut buf = [0u8; 8];
    loop {
        // SAFETY: `fd` is a valid eventfd; `buf` is a valid 8-byte
        // stack buffer. EAGAIN (EWOULDBLOCK) means the counter is 0 —
        // we exit the loop.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return;
            }
            // Any other error is fatal-ish; log and exit the loop to
            // avoid spinning on the same error forever.
            idle_log::warn!(
                error = %err,
                fd,
                "wayland-present: eventfd read failed"
            );
            return;
        }
        if n == 0 {
            return;
        }
    }
}

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
                let _ = return_pool.send(pixels);
            }
            PresenterCommand::Hide => state.hide(),
        }
    }
}
