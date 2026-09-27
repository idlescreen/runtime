// SPDX-License-Identifier: MIT

use std::os::fd::AsFd;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{self, JoinHandle};

use wayland_client::Connection;

use super::state::SessionState;

/// Owns the Wayland connection on a dedicated background thread.
///
/// `ready_tx` is the startup handshake: exactly one value is sent before the
/// event loop is entered, reporting whether the compositor actually offered
/// `ext-idle-notify-v1` and a seat. The caller must wait on it — returning a
/// handle whose thread died with `is_alive` stuck at `true` is how a
/// screensaver ends up silently inert on compositors like GNOME/Mutter that
/// do not implement the protocol.
///
/// The returned handle is joined in `IdleMonitor::drop`; see the matching note
/// on `wayland_present::OverlayPresenter` about detached threads racing
/// process exit during teardown.
pub fn spawn_event_thread(
    ready_tx: Sender<Result<(), String>>,
    is_idle: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    timeout_rx: Receiver<u32>,
    initial_timeout_mins: u32,
    is_alive: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        match run_event_loop(is_idle, shutdown, timeout_rx, initial_timeout_mins) {
            Ok(()) => {
                let _ = ready_tx.send(Ok(()));
            }
            Err(error) => {
                let _ = ready_tx.send(Err(error.to_string()));
                idle_log::warn!("wayland-idle: {error}");
            }
        }
        is_alive.store(false, Ordering::SeqCst);
    })
}

fn run_event_loop(
    is_idle: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    timeout_rx: Receiver<u32>,
    initial_timeout_mins: u32,
) -> Result<(), String> {
    let connection =
        Connection::connect_to_env().map_err(|_| "failed to connect to Wayland".to_string())?;

    let mut event_queue = connection.new_event_queue();
    let queue = event_queue.handle();
    let _registry = connection.display().get_registry(&queue, ());

    let mut state = SessionState {
        notifier: None,
        seat: None,
        notification: None,
        is_idle,
        queue: queue.clone(),
        timeout_mins: initial_timeout_mins,
    };

    event_queue
        .roundtrip(&mut state)
        .map_err(|_| "initial registry roundtrip failed".to_string())?;

    // Startup readiness. A compositor without ext-idle-notify-v1 (GNOME /
    // Mutter) or without a seat leaves this monitor permanently reporting
    // "not idle", so treat it as a failure the caller must see — never as a
    // healthy monitor that simply never fires.
    state.refresh_idle_notification()?;

    let fd = connection.as_fd().as_raw_fd();
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    while !shutdown.load(Ordering::Relaxed) {
        let _ = connection.flush();
        dispatch_pending_events(&connection, &mut event_queue, &mut state, &mut poll_fd)?;
        apply_timeout_updates(&mut state, &timeout_rx);
    }

    Ok(())
}

fn dispatch_pending_events(
    connection: &Connection,
    event_queue: &mut wayland_client::EventQueue<SessionState>,
    state: &mut SessionState,
    poll_fd: &mut libc::pollfd,
) -> Result<(), String> {
    if let Some(guard) = event_queue.prepare_read() {
        let _ = connection.flush();

        // SAFETY: `poll_fd` points to one valid `pollfd` for the Wayland socket.
        let poll_result = unsafe { libc::poll(poll_fd, 1, 100) };
        if poll_result > 0 {
            if poll_fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err("Wayland connection closed".to_string());
            }

            if poll_fd.revents & libc::POLLIN != 0 {
                guard
                    .read()
                    .map_err(|_| "failed to read Wayland events".to_string())?;
                event_queue
                    .dispatch_pending(state)
                    .map_err(|_| "failed to dispatch Wayland events".to_string())?;
            }
        } else if poll_result < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                return Err("poll failed".to_string());
            }
        }
    } else {
        event_queue
            .dispatch_pending(state)
            .map_err(|_| "failed to dispatch Wayland events".to_string())?;
    }

    Ok(())
}

fn apply_timeout_updates(state: &mut SessionState, timeout_rx: &Receiver<u32>) {
    while let Ok(timeout_mins) = timeout_rx.try_recv() {
        if state.timeout_mins != timeout_mins {
            state.timeout_mins = timeout_mins;
            // A timeout change after startup cannot be fatal — the monitor is
            // already live — so a failure here only loses the new deadline.
            if state.refresh_idle_notification().is_err() {
                idle_log::warn!("wayland-idle: could not re-register idle notification");
            }
        }
    }
}
