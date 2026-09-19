// SPDX-License-Identifier: MIT

use idle_api::TerminalCell;
use idle_ipc::{IpcCommand, IpcResponse, SharedMemory};
use idle_runner::cell_renderer::CellRenderer;
use idle_runner::launcher::LaunchMode;
use idle_upscaler::{FilterMode, FrameUpscaler, resolve_render_scale};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::ipc_init::initialize_ipc_session;
use super::timeout::is_timeout;

pub struct IpcPluginSession {
    pub(crate) saver_name: String,
    pub(crate) render_scale: f32,
    pub(crate) saver_params: std::collections::BTreeMap<String, String>,
    pub(crate) renderer: CellRenderer,
    pub(crate) upscaler: FrameUpscaler,
    pub(crate) grid: Vec<TerminalCell>,
    pub(crate) content_buf: Vec<u8>,
    pub(crate) hardware_scaling: bool,

    pub(crate) child: Option<Child>,
    pub(crate) socket: Option<UnixStream>,
    pub(crate) shm: Option<SharedMemory>,
    pub(crate) socket_path: Option<PathBuf>,
    pub(crate) expected_stop: Arc<AtomicBool>,
    /// When true, an unexpected child exit may arm the failsafe locker once.
    pub(crate) failsafe_armed: Arc<AtomicBool>,
}

impl IpcPluginSession {
    pub fn load_with_options(
        saver_name: &str,
        _launch_mode: &LaunchMode,
        render_scale: Option<f32>,
        saver_params: std::collections::BTreeMap<String, String>,
        want_gpu: bool,
    ) -> Result<Self, String> {
        // The wgpu probe costs ~400MB of transient driver mappings; only pay
        // it when the caller expects the raster load to justify it.
        let renderer = if want_gpu {
            CellRenderer::new_with_gpu().map_err(|e| e.to_string())?
        } else {
            CellRenderer::new().map_err(|e| e.to_string())?
        };
        let render_scale = resolve_render_scale(render_scale);
        let upscaler = FrameUpscaler::new(FilterMode::from_env());

        Ok(Self {
            saver_name: saver_name.to_string(),
            render_scale,
            saver_params,
            renderer,
            upscaler,
            grid: Vec::new(),
            content_buf: Vec::new(),
            hardware_scaling: false,
            child: None,
            socket: None,
            shm: None,
            socket_path: None,
            expected_stop: Arc::new(AtomicBool::new(false)),
            failsafe_armed: Arc::new(AtomicBool::new(true)),
        })
    }

    pub fn render_scale(&self) -> f32 {
        self.render_scale
    }

    pub fn using_gpu_upscale(&self) -> bool {
        self.upscaler.using_gpu()
    }

    pub fn set_hardware_scaling(&mut self, enabled: bool) {
        self.hardware_scaling = enabled;
    }

    pub fn content_width(&self, cols: usize) -> u32 {
        self.renderer.content_width(cols)
    }

    pub fn content_height(&self, rows: usize) -> u32 {
        self.renderer.content_height(rows)
    }

    pub fn grid_for_pixels(&self, width: u32, height: u32) -> (usize, usize) {
        self.renderer
            .grid_for_pixels_scaled(width, height, self.render_scale)
    }

    pub fn init(&mut self, cols: usize, rows: usize) -> Result<(), String> {
        let cells = cols
            .checked_mul(rows)
            .ok_or_else(|| format!("grid size overflow: {cols}x{rows}"))?;
        self.grid = vec![TerminalCell::default(); cells];

        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(path) = self.socket_path.take() {
            let _ = std::fs::remove_file(path);
        }

        let init_res = initialize_ipc_session(
            &self.saver_name,
            cols,
            rows,
            self.render_scale,
            &self.saver_params,
        )?;

        self.child = Some(init_res.child);
        self.socket = Some(init_res.socket);
        self.shm = Some(init_res.shm);
        self.socket_path = Some(init_res.socket_path);

        Ok(())
    }

    pub fn set_simulation_rate(&mut self, fps: f32) {
        if let Some(ref mut socket) = self.socket {
            let cmd = IpcCommand::SetSimulationRate { hz: fps };
            if let Err(e) = cmd.write_to(&mut *socket) {
                idle_log::error!("failed to send SetSimulationRate: {}", e);
                self.socket = None;
                return;
            }
            match IpcResponse::read_from(&mut *socket) {
                Ok(IpcResponse::Ack) => {}
                Ok(resp) => {
                    idle_log::error!("unexpected response to SetSimulationRate: {:?}", resp);
                    self.socket = None;
                }
                Err(e) => {
                    idle_log::error!("failed to read SetSimulationRate Ack: {}", e);
                    self.socket = None;
                }
            }
        }
    }

    pub fn tick(&mut self, frame_dt: Duration) {
        if let Some(ref mut socket) = self.socket {
            let cmd = IpcCommand::TickAndDraw {
                dt_micros: frame_dt.as_micros() as u64,
            };
            if let Err(e) = cmd.write_to(&mut *socket) {
                if is_timeout(&e) {
                    idle_log::error!(
                        saver = %self.saver_name,
                        "IPC write timed out — saver hung inside TickAndDraw; killing child"
                    );
                    self.kill_child();
                    return;
                }
                idle_log::error!("failed to send TickAndDraw: {}", e);
                self.socket = None;
            }
        }
    }

    /// Kill the saver child process. Idempotent: a second call after the
    /// child has already exited is a no-op. The caller is responsible for
    /// clearing `socket` / `shm` / `socket_path` after kill — `init` will
    /// recover them on the next session start.
    ///
    /// Subprocess isolation primitive: when the saver hangs inside an IPC
    /// command, the daemon calls this from the per-plugin watchdog. We send
    /// `SIGKILL` (not `SIGTERM`) because plugin code that ignores signals
    /// inside its own `update()` cannot be reasoned with politely.
    pub fn kill_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            idle_log::warn!(
                saver = %self.saver_name,
                "subprocess isolation: killing hung saver child (pid {})",
                child.id()
            );
            let _ = child.kill();
            let _ = child.wait();
        }
        self.expected_stop
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
