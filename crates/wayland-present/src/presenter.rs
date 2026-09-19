// SPDX-License-Identifier: MIT

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::appearance::OverlayAppearance;
use crate::output::{OutputLayout, OutputRegistry};
use crate::overlay::{PresenterCommand, spawn_event_thread};

/// Presents fullscreen Wayland overlays on top of the desktop.
pub struct OverlayPresenter {
    command_tx: SyncSender<PresenterCommand>,
    buffer_tx: Sender<Vec<u8>>,
    buffer_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    visible: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    outputs: OutputRegistry,
    is_alive: Arc<AtomicBool>,
    supports_scaling: Arc<AtomicBool>,
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
        let (buffer_tx, buffer_rx) = mpsc::channel();
        let visible = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let outputs = OutputRegistry::new();
        let is_alive = Arc::new(AtomicBool::new(true));
        let supports_scaling = Arc::new(AtomicBool::new(false));

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
        );

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Some(Self {
                command_tx,
                buffer_tx,
                buffer_rx: Mutex::new(buffer_rx),
                visible,
                shutdown,
                outputs,
                is_alive,
                supports_scaling,
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

    pub fn submit_frame(&self, output_id: u32, width: u32, height: u32, pixels: Vec<u8>) {
        self.send_cmd(PresenterCommand::UpdateFrame {
            output_id,
            width,
            height,
            pixels,
            return_pool: self.buffer_tx.clone(),
        });
    }

    pub fn get_frame_buffer(&self, size: usize) -> Vec<u8> {
        if let Ok(mut buf) = self.buffer_rx.lock().unwrap().try_recv() {
            if buf.len() != size {
                buf.resize(size, 0);
            }
            return buf;
        }
        vec![0; size]
    }

    pub fn hide(&self) {
        self.send_cmd(PresenterCommand::Hide);
    }
}

impl Drop for OverlayPresenter {
    fn drop(&mut self) {
        self.send_cmd(PresenterCommand::Hide);
        self.shutdown.store(true, Ordering::Relaxed);
        // Bare wake so the event loop sees `shutdown` promptly even if the
        // command channel is already drained.
        self.wake();

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
