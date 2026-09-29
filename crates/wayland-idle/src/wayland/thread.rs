// SPDX-License-Identifier: MIT

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
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
        if let Err(error) = run_event_loop(
            ready_tx,
            is_idle,
            shutdown,
            timeout_rx,
            initial_timeout_mins,
        ) {
            idle_log::warn!("wayland-idle: {error}");
        }
        is_alive.store(false, Ordering::SeqCst);
    })
}

fn run_event_loop(
    ready_tx: Sender<Result<(), String>>,
    is_idle: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    timeout_rx: Receiver<u32>,
    initial_timeout_mins: u32,
) -> Result<(), String> {
    let connection = Connection::connect_to_env().map_err(|e| {
        let err = format!("failed to connect to Wayland: {e}");
        let _ = ready_tx.send(Err(err.clone()));
        err
    })?;

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

    if let Err(e) = event_queue.roundtrip(&mut state) {
        let err = format!("initial registry roundtrip failed: {e}");
        let _ = ready_tx.send(Err(err.clone()));
        return Err(err);
    }

    // Startup readiness. A compositor without ext-idle-notify-v1 (GNOME /
    // Mutter) or without a seat leaves this monitor permanently reporting
    // "not idle", so treat it as a failure the caller must see — never as a
    // healthy monitor that simply never fires.
    if let Err(e) = state.refresh_idle_notification() {
        let err = e.to_string();
        let _ = ready_tx.send(Err(err.clone()));
        return Err(err);
    }

    // Handshake success: signal readiness to the constructor before entering the event loop!
    let _ = ready_tx.send(Ok(()));

    // Tier-2 step 3 (perf plan §"Wayland epoll"): replace the per-iteration
    // `libc::poll(pollfd, 1, 100ms)` with an epoll fd that watches the
    // Wayland socket + an eventfd used as a wake channel. epoll scales to
    // any number of fds without per-poll allocation, and the eventfd
    // gives us a sub-millisecond shutdown signal that doesn't depend on
    // `timeout_rx.try_recv()` happening to fire in time.
    //
    // The 100ms epoll_wait timeout remains as a safety net for the rare
    // case where neither the Wayland socket nor the wake eventfd fires
    // (e.g. compositor restart mid-poll). Without it the thread would
    // block forever on a stale state.
    let wayland_fd = connection.as_fd().as_raw_fd();

    // Owned eventfd for shutdown signaling. `eventfd(0, EFD_CLOEXEC |
    // EFD_NONBLOCK)` — CLOEXEC so a child fork doesn't inherit it,
    // NONBLOCK so a stale read returns EAGAIN instead of blocking.
    let wake_fd = make_eventfd()?;
    let epoll_fd = make_epoll()?;
    epoll_add(epoll_fd.as_raw_fd(), wayland_fd, libc::EPOLLIN, 1)?;
    epoll_add(epoll_fd.as_raw_fd(), wake_fd.as_raw_fd(), libc::EPOLLIN, 2)?;

    // SAFETY: epoll_wait needs a small stack array. 4 slots is plenty —
    // we only have 2 fds registered, but a single roundtrip can produce
    // multiple events for the same fd.
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 4];

    while !shutdown.load(Ordering::Relaxed) {
        let _ = connection.flush();
        let n = unsafe {
            libc::epoll_wait(
                epoll_fd.as_raw_fd(),
                events.as_mut_ptr(),
                events.len() as libc::c_int,
                100,
            )
        };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                return Err(format!("epoll_wait failed: {err}"));
            }
            continue;
        }
        for &ev in &events[..n as usize] {
            match ev.u64 {
                1 => {
                    // Wayland socket readable. The existing
                    // `dispatch_pending_events` already handles the
                    // prepare_read / read / dispatch_pending dance.
                    dispatch_pending_events(&connection, &mut event_queue, &mut state)?;
                }
                2 => {
                    // Shutdown signal. Drain the eventfd and re-check
                    // the atomic; the atomic is the source of truth
                    // (the eventfd just avoids the 100ms wait).
                    drain_eventfd(wake_fd.as_raw_fd());
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                }
                _ => {}
            }
        }
        apply_timeout_updates(&mut state, &timeout_rx);
    }

    Ok(())
}

/// Create a non-blocking, close-on-exec eventfd for shutdown signaling.
fn make_eventfd() -> Result<OwnedFd, String> {
    // SAFETY: eventfd with the documented flag set is safe to call; the
    // returned fd is owned by Rust via `OwnedFd`.
    let raw = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if raw < 0 {
        return Err(format!(
            "eventfd() failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `raw` is a valid fd returned by `eventfd` and we own it.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

/// Create a new epoll fd.
fn make_epoll() -> Result<OwnedFd, String> {
    // SAFETY: epoll_create1 with EPOLL_CLOEXEC is the modern safe call;
    // returns a fresh fd we own.
    let raw = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if raw < 0 {
        return Err(format!(
            "epoll_create1() failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `raw` is a valid fd returned by `epoll_create1` and we own it.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

/// Add `fd` to the epoll set with the given event mask and a stable
/// tag (the `u64` slot in `epoll_event`). The tag lets us route the
/// wakeup to the right handler in the event loop.
fn epoll_add(
    epoll_fd: libc::c_int,
    fd: libc::c_int,
    mask: libc::c_int,
    tag: u64,
) -> Result<(), String> {
    let mut event = libc::epoll_event {
        events: mask as u32,
        u64: tag,
    };
    // SAFETY: `event` is a valid `epoll_event` struct; `fd` is a valid
    // descriptor (we own it or it is the Wayland socket lifetime).
    let rc = unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) };
    if rc < 0 {
        return Err(format!(
            "epoll_ctl(ADD, {fd}) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Drain an eventfd by reading 8 bytes (the counter). The NONBLOCK
/// flag means the read returns immediately even if no signal is
/// pending; the value itself is discarded — we only care that the fd
/// fired.
fn drain_eventfd(fd: libc::c_int) {
    let mut buf = [0u8; 8];
    // SAFETY: `fd` is a valid eventfd with NONBLOCK; `buf` is a valid
    // 8-byte stack buffer; the read returns EAGAIN when the counter is
    // already 0, which we ignore.
    let _ = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
}

fn dispatch_pending_events(
    connection: &Connection,
    event_queue: &mut wayland_client::EventQueue<SessionState>,
    state: &mut SessionState,
) -> Result<(), String> {
    if let Some(guard) = event_queue.prepare_read() {
        let _ = connection.flush();
        guard
            .read()
            .map_err(|_| "failed to read Wayland events".to_string())?;
        event_queue
            .dispatch_pending(state)
            .map_err(|_| "failed to dispatch Wayland events".to_string())?;
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
