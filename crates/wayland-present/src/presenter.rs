// SPDX-License-Identifier: MIT

//! Wayland overlay presenter (entry point).
//!
//! Owns the event thread, the frame pool, the wake-fd, and the
//! command channel. Supporting modules:
//!
//! - [`crate::frame_pool`] — `FramePool` type alias + recycler.
//! - [`crate::drop_presenter`] — `Drop for OverlayPresenter`
//!   (bounded teardown).

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::time::Duration;

use crate::appearance::OverlayAppearance;
use crate::frame_pool::{FramePool, empty_frame_pool, get_frame_buffer};
use crate::frame_signal::FrameSignal;
use crate::output::{OutputLayout, OutputRegistry};
use crate::overlay::{PresenterCommand, spawn_event_thread};

/// Presents fullscreen Wayland overlays on top of the desktop.
pub struct OverlayPresenter {
    pub(crate) command_tx: SyncSender<PresenterCommand>,
    /// Frame buffer pool. `VecDeque<Arc<Vec<u8>>>` keeps one or
    /// two returned buffers around for reuse; steady-state cost is
    /// `Arc::clone` per frame instead of mpsc channel ops.
    pub(crate) frame_pool: FramePool,
    pub(crate) visible: Arc<AtomicBool>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) outputs: OutputRegistry,
    pub(crate) is_alive: Arc<AtomicBool>,
    pub(crate) supports_scaling: Arc<AtomicBool>,
    /// Frame-presented signal. The event thread notifies this after each
    /// successful surface commit. The daemon's frame loop waits on it
    /// instead of polling a 2 ms slice.
    ///
    /// When the compositor's `wl_callback::done` dispatcher (in
    /// `handlers/buffer_objects.rs`) is wired, this signal can be
    /// notified on actual vsync — see the `FrameSignal` module docs.
    pub(crate) frame_signal: FrameSignal,
    /// Self-wake for the event thread: writing makes its `poll()` return so
    /// queued commands are applied immediately rather than after the next
    /// compositor event (or the 100ms poll timeout).
    pub(crate) wake_fd: OwnedFd,
    /// Joined in Drop so libwayland teardown finishes on the event thread
    /// before the presenter (or the process) unwinds past it — a detached
    /// thread racing process exit was the teardown SIGSEGV class.
    pub(crate) event_thread: Option<std::thread::JoinHandle<()>>,
}

impl OverlayPresenter {
    /// Connect to the compositor and prepare the overlay session.
    pub fn new() -> Option<Self> {
        if !Self::is_available() {
            return None;
        }

        let (ready_tx, ready_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::sync_channel(1);
        let frame_pool: FramePool = empty_frame_pool();
        let visible = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let outputs = OutputRegistry::new();
        let is_alive = Arc::new(AtomicBool::new(true));
        let supports_scaling = Arc::new(AtomicBool::new(false));
        let frame_signal = FrameSignal::new();

        // SAFETY: fresh eventfd; NONBLOCK so a wake write never stalls the
        // render loop when a wake is already pending. CLOEXEC keeps it out
        // of plugin child processes.
        let wake_fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if wake_fd < 0 {
            return None;
        }
        // SAFETY: `wake_fd` is a valid owned descriptor from eventfd above.
        let wake_fd = unsafe { OwnedFd::from_raw_fd(wake_fd) };
        let Ok(wake_rx) = wake_fd.try_clone() else {
            return None;
        };

        let event_thread = spawn_event_thread(
            ready_tx,
            command_rx,
            visible.clone(),
            shutdown.clone(),
            outputs.clone(),
            is_alive.clone(),
            supports_scaling.clone(),
            wake_rx,
            frame_signal.clone(),
        );

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Some(Self {
                command_tx,
                frame_pool,
                visible,
                shutdown,
                outputs,
                is_alive,
                supports_scaling,
                frame_signal,
                wake_fd,
                event_thread: Some(event_thread),
            }),
            other => {
                // Startup failed (thread reported Err, or the ready channel
                // timed out/closed) — still join so the failed thread's
                // teardown doesn't outlive the constructor.
                shutdown.store(true, Ordering::Relaxed);
                let _ = event_thread.join();
                idle_log::warn!("wayland-present: event thread init failed: {other:?}");
                None
            }
        }
    }

    pub fn is_available() -> bool {
        std::env::var("WAYLAND_DISPLAY").is_ok()
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    /// Returns `true` if the Wayland presentation thread is still running.
    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::SeqCst)
    }

    /// Returns `true` if the compositor supports `wp_viewporter` hardware scaling.
    pub fn supports_scaling(&self) -> bool {
        self.supports_scaling.load(Ordering::SeqCst)
    }

    /// Handle the daemon's frame loop can wait on. Cheap to clone.
    /// Currently notified on every successful frame commit; will be
    /// additionally notified on `wl_callback::done` for true vsync
    /// once the dispatch hook is wired.
    pub fn frame_signal(&self) -> FrameSignal {
        self.frame_signal.clone()
    }

    pub fn output_layouts(&self) -> Vec<OutputLayout> {
        self.outputs.layouts()
    }

    /// Wake the event thread out of poll() so queued commands are applied
    /// now. NONBLOCK fd: EAGAIN just means a wake is already pending.
    pub(crate) fn wake(&self) {
        let one: u64 = 1;
        // SAFETY: wake_fd is a live eventfd; we write a full u64.
        unsafe {
            libc::write(
                self.wake_fd.as_raw_fd(),
                std::ptr::from_ref(&one).cast::<libc::c_void>(),
                8,
            );
        }
    }

    pub(crate) fn send_cmd(&self, cmd: PresenterCommand) {
        let _ = self.command_tx.send(cmd);
        self.wake();
    }

    pub fn show(&self, appearance: OverlayAppearance) {
        self.send_cmd(PresenterCommand::ShowSolid(appearance));
    }

    pub fn show_screensaver(&self) {
        self.send_cmd(PresenterCommand::ShowScreensaver);
    }

    pub fn submit_frame(&self, output_id: u32, width: u32, height: u32, pixels: Arc<Vec<u8>>) {
        self.send_cmd(PresenterCommand::UpdateFrame {
            output_id,
            width,
            height,
            pixels,
            return_pool: self.frame_pool.clone(),
        });
    }

    /// Convenience wrapper around [`get_frame_buffer`] for the
    /// daemon's frame path. See `frame_pool` for the full contract.
    pub fn get_frame_buffer(&self, size: usize) -> Vec<u8> {
        get_frame_buffer(&self.frame_pool, size)
    }

    pub fn hide(&self) {
        self.send_cmd(PresenterCommand::Hide);
    }
}
