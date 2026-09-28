// SPDX-License-Identifier: MIT

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::appearance::OverlayAppearance;
use crate::frame_signal::FrameSignal;
use crate::output::{OutputLayout, OutputRegistry};
use crate::overlay::{PresenterCommand, spawn_event_thread};

// Frame buffer pool. Triple-buffered across the `daemon` and the
// event thread. The daemon hands the presenter an
// `Arc<Vec<u8>>` via `Arc::new(pixels)`, the presenter holds it
// across one Wayland commit, then pushes it back. The Vec keeps
// `try_unwrap` available (sized), so the recycling path can
// reclaim the heap allocation without a clone.
//
// Replaces the prior `mpsc::Sender<Vec<u8>>` + `sync_channel(1)`
// round-trip. Steady-state cost is one `Arc::clone` per frame.
// Type alias: `Arc<Mutex<VecDeque<Arc<Vec<u8>>>>>`. We can't use
// `Arc<[u8]>` here because `[u8]` is unsized and lacks
// `try_unwrap`/`into_inner`, so the recycler couldn't reclaim
// the bytes without a copy.
type FramePool = Arc<Mutex<std::collections::VecDeque<Arc<Vec<u8>>>>>;

/// Presents fullscreen Wayland overlays on top of the desktop.
pub struct OverlayPresenter {
    command_tx: SyncSender<PresenterCommand>,
    /// Frame buffer pool. `VecDeque<Arc<Vec<u8>>>` keeps one or
    /// two returned buffers around for reuse; steady-state cost is
    /// `Arc::clone` per frame instead of mpsc channel ops.
    frame_pool: FramePool,
    visible: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    outputs: OutputRegistry,
    is_alive: Arc<AtomicBool>,
    supports_scaling: Arc<AtomicBool>,
    /// Frame-presented signal. The event thread notifies this after each
    /// successful surface commit. The daemon's frame loop waits on it
    /// instead of polling a 2 ms slice.
    ///
    /// When the compositor's `wl_callback::done` dispatcher (in
    /// `handlers/buffer_objects.rs`) is wired, this signal can be
    /// notified on actual vsync — see the `FrameSignal` module docs.
    frame_signal: FrameSignal,
    /// Self-wake for the event thread: writing makes its `poll()` return so
    /// queued commands are applied immediately rather than after the next
    /// compositor event (or the 100ms poll timeout).
    wake_fd: OwnedFd,
    /// Joined in Drop so libwayland teardown finishes on the event thread
    /// before the presenter (or the process) unwinds past it — a detached
    /// thread racing process exit was the teardown SIGSEGV class.
    event_thread: Option<JoinHandle<()>>,
}

impl OverlayPresenter {
    /// Connect to the compositor and prepare the overlay session.
    pub fn new() -> Option<Self> {
        if !Self::is_available() {
            return None;
        }

        let (ready_tx, ready_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::sync_channel(1);
        // Arc<Vec<u8>> pool — owned by the daemon. Replaces the prior
        // `mpsc::Sender<Vec<u8>>` channel (`Sender::clone` +
        // `sync_channel(1).send` per frame). The new path is
        // `Arc::clone` + `Mutex<VecDeque>::push_back` per frame; the
        // daemon's `get_frame_buffer` consumes the pool and recycles
        // via `Arc::try_unwrap`. Sized (Vec is Sized) so the
        // recycler can reclaim the heap allocation cleanly.
        let frame_pool: FramePool = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let visible = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let outputs = OutputRegistry::new();
        let is_alive = Arc::new(AtomicBool::new(true));
        let supports_scaling = Arc::new(AtomicBool::new(false));
        // Frame-presence signal shared between the event thread (which
        // notifies after a successful commit) and the daemon's frame loop
        // (which waits on it instead of polling). Cheap to clone.
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
    fn wake(&self) {
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

    fn send_cmd(&self, cmd: PresenterCommand) {
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

    /// Pop a recyclable buffer from the return pool, sizing it to
    /// `size` if the cached buffer doesn't match. If the pool is
    /// empty we allocate a fresh zeroed `Vec<u8>` — the daemon's
    /// first frame.
    ///
    /// Returns a `Vec<u8>` so the daemon can write into it via
    /// `&mut [u8]`; the producer wraps it in `Arc::new(pixels)`
    /// before submitting. Recycling relies on `Arc::try_unwrap`
    /// succeeding (refcount == 1, only the pool held the Arc).
    pub fn get_frame_buffer(&self, size: usize) -> Vec<u8> {
        let mut pool = self.frame_pool.lock().unwrap_or_else(|p| {
            idle_log::warn!("wayland-present: frame_pool mutex poisoned; recovering");
            p.into_inner()
        });
        while let Some(arc) = pool.pop_front() {
            match Arc::try_unwrap(arc) {
                Ok(v) => {
                    if v.len() == size {
                        return v;
                    }
                    let mut v = v;
                    v.resize(size, 0);
                    return v;
                }
                Err(arc) => {
                    // Another caller still references this buffer
                    // (e.g. the event thread hasn't dropped its Arc
                    // yet). Skip it; the next iteration pulls the
                    // next available one. Bounded: steady-state each
                    // frame produces one buffer and consumes one.
                    idle_log::debug!("wayland-present: skip contested frame buffer (refcount > 1)");
                    drop(arc);
                }
            }
        }
        vec![0; size]
    }

    pub fn hide(&self) {
        self.send_cmd(PresenterCommand::Hide);
    }
}

impl Drop for OverlayPresenter {
    fn drop(&mut self) {
        // Set shutdown and wake FIRST. `command_tx` is a `sync_channel(1)`, so
        // a blocking `send` here would hang Drop itself when the channel is
        // full — and a wedged event thread (compositor not draining the socket)
        // would pin shutdown forever, which is exactly what the bounded join
        // below exists to prevent. A `try_send` that loses the race to a full
        // channel is fine: `shutdown` is what the loop actually polls.
        self.shutdown.store(true, Ordering::Relaxed);
        // Bare wake so the event loop sees `shutdown` promptly even if the
        // command channel is already drained.
        self.wake();

        let _ = self.command_tx.try_send(PresenterCommand::Hide);

        // Bounded join: the poll loop turns over in ≤100ms, so teardown
        // completes well under this bound on a healthy compositor. A
        // bounded wait beats an unbounded join — a wedged event thread
        // must not hang daemon shutdown forever; leaking the joiner is
        // the lesser evil.
        if let Some(handle) = self.event_thread.take() {
            let (done_tx, done_rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = handle.join();
                let _ = done_tx.send(());
            });
            if done_rx.recv_timeout(Duration::from_secs(2)).is_err() {
                idle_log::warn!("wayland-present: event thread did not exit within 2s of shutdown");
            }
        }
    }
}
