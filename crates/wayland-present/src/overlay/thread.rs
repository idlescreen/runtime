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

    let fd = connection.as_fd().as_raw_fd();
    // Index 0: Wayland socket. Index 1: self-wake eventfd — `submit_frame`
    // and friends write it so commands commit immediately instead of
    // waiting on the poll timeout or the next compositor event.
    let mut poll_fds = [
        libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_rx.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];

    while !shutdown.load(Ordering::Relaxed) {
        let _ = connection.flush();
        dispatch_pending_events(&connection, &mut event_queue, &mut state, &mut poll_fds)?;
        apply_commands(&mut state, &command_rx);
    }

    state.hide();
    Ok(())
}

fn dispatch_pending_events(
    connection: &Connection,
    event_queue: &mut wayland_client::EventQueue<SessionState>,
    state: &mut SessionState,
    poll_fds: &mut [libc::pollfd; 2],
) -> Result<(), &'static str> {
    if let Some(guard) = event_queue.prepare_read() {
        let _ = connection.flush();

        // SAFETY: `poll_fds` points to two valid `pollfd`s (wayland + wake).
        let poll_result = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, 100) };
        if poll_result > 0 {
            if poll_fds[1].revents & libc::POLLIN != 0 {
                // Drain the self-wake counter; a pending wake is enough.
                let mut buf = [0u8; 8];
                while unsafe {
                    libc::read(poll_fds[1].fd, buf.as_mut_ptr().cast::<libc::c_void>(), 8)
                } == 8
                {}
            }
            if poll_fds[0].revents & libc::POLLIN != 0 {
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
                            revents = poll_fds[0].revents,
                            "wayland-present: failed to read Wayland events (compositor may have closed the connection; often a protocol error on the previous commit)"
                        );
                        return Err("failed to read Wayland events");
                    }
                }
                if let Err(e) = event_queue.dispatch_pending(state) {
                    idle_log::error!(error = %e, "wayland-present: failed to dispatch Wayland events");
                    return Err("failed to dispatch Wayland events");
                }
            }

            if poll_fds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
                || poll_fds[1].revents & (libc::POLLERR | libc::POLLNVAL) != 0
            {
                return Err("Wayland connection closed");
            }
        } else if poll_result < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                idle_log::error!(error = %err, "wayland-present: poll failed");
                return Err("poll failed");
            }
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
