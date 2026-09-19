// SPDX-License-Identifier: MIT

use std::os::fd::AsFd;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::thread;

use wayland_client::Connection;

use super::state::SessionState;

/// Owns the Wayland connection on a dedicated background thread.
pub fn spawn_event_thread(
    is_idle: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    timeout_rx: Receiver<u32>,
    initial_timeout_mins: u32,
    is_alive: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        if let Err(error) = run_event_loop(is_idle, shutdown, timeout_rx, initial_timeout_mins) {
            idle_log::warn!("wayland-idle: {error}");
        }
        is_alive.store(false, Ordering::SeqCst);
    });
}

fn run_event_loop(
    is_idle: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    timeout_rx: Receiver<u32>,
    initial_timeout_mins: u32,
) -> Result<(), &'static str> {
    let connection = Connection::connect_to_env().map_err(|_| "failed to connect to Wayland")?;

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
        .map_err(|_| "initial registry roundtrip failed")?;

    state.refresh_idle_notification();

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
) -> Result<(), &'static str> {
    if let Some(guard) = event_queue.prepare_read() {
        let _ = connection.flush();

        // SAFETY: `poll_fd` points to one valid `pollfd` for the Wayland socket.
        let poll_result = unsafe { libc::poll(poll_fd, 1, 100) };
        if poll_result > 0 {
            if poll_fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err("Wayland connection closed");
            }

            if poll_fd.revents & libc::POLLIN != 0 {
                guard.read().map_err(|_| "failed to read Wayland events")?;
                event_queue
                    .dispatch_pending(state)
                    .map_err(|_| "failed to dispatch Wayland events")?;
            }
        } else if poll_result < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                return Err("poll failed");
            }
        }
    } else {
        event_queue
            .dispatch_pending(state)
            .map_err(|_| "failed to dispatch Wayland events")?;
    }

    Ok(())
}

fn apply_timeout_updates(state: &mut SessionState, timeout_rx: &Receiver<u32>) {
    while let Ok(timeout_mins) = timeout_rx.try_recv() {
        if state.timeout_mins != timeout_mins {
            state.timeout_mins = timeout_mins;
            state.refresh_idle_notification();
        }
    }
}
