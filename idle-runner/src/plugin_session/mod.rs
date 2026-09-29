// SPDX-License-Identifier: MIT

#![allow(clippy::too_many_arguments)]

use crate::budget::CpuBudget;
use crate::cell_renderer::CellRenderer;
use idle_api::{Screensaver, ScreensaverInstance, TerminalCell};
use idle_upscaler::FrameUpscaler;
use std::time::Duration;

pub(crate) mod entry;
pub(crate) mod loading;
pub(crate) mod manifest_gate;
mod reloading;
mod viewport;

pub(crate) struct PluginGuard {
    pub(crate) ptr: *mut ScreensaverInstance,
    pub(crate) destroy: unsafe extern "C" fn(*mut ScreensaverInstance),
    pub(crate) _lib: crate::dylib::Library,
}

impl Drop for PluginGuard {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(self.ptr);
        }
    }
}

impl PluginGuard {
    pub(crate) fn saver_mut(&mut self) -> &mut dyn Screensaver {
        unsafe { &mut *(*self.ptr).inner }
    }
}

/// Headless screensaver plugin host for Wayland frame presentation.
pub struct PluginSession {
    pub(crate) plugin: Option<PluginGuard>,
    pub(crate) plugin_path: std::path::PathBuf,
    /// Capability declaration this plugin was admitted under. `None` only for
    /// the operator-gated unsigned escape hatch.
    pub(crate) manifest: Option<std::sync::Arc<idle_api::plugin_manifest::Manifest>>,
    pub(crate) renderer: CellRenderer,
    pub(crate) upscaler: FrameUpscaler,
    pub(crate) render_scale: f32,
    pub(crate) grid: Vec<TerminalCell>,
    pub(crate) content_buf: Vec<u8>,
    pub(crate) pixel_buf: std::sync::Arc<Vec<u8>>,
    pub(crate) physics_accumulator: Duration,
    pub(crate) physics_duration: Duration,
    pub(crate) time_elapsed: Duration,
    pub(crate) simulation_cols: usize,
    pub(crate) simulation_rows: usize,
    pub(crate) hardware_scaling: bool,
    pub(crate) watcher: Option<crate::filewatch::DirWatcher>,
    pub(crate) needs_reload: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) cpu_budget: Option<CpuBudget>,
    pub(crate) gpu_budget: Option<crate::gpu_budget::GpuBudget>,
}

impl PluginSession {
    pub fn grid(&self) -> &[TerminalCell] {
        &self.grid
    }

    /// Capability declaration this session was admitted under, if any.
    pub fn manifest(&self) -> Option<&idle_api::plugin_manifest::Manifest> {
        self.manifest.as_deref()
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

    pub fn init(&mut self, cols: usize, rows: usize) {
        self.simulation_cols = cols;
        self.simulation_rows = rows;
        let cells = cols.checked_mul(rows).unwrap_or(0);
        self.grid = vec![TerminalCell::default(); cells];
        if let Some(plugin) = self.plugin.as_mut() {
            plugin.saver_mut().init(cols, rows);
        }
    }

    pub fn set_simulation_rate(&mut self, fps: f32) {
        // Finite, bounded Hz only — NaN/inf must not yield zero-duration busy loops.
        let hz = if fps.is_finite() {
            fps.clamp(30.0, 240.0)
        } else {
            30.0
        };
        self.physics_duration = Duration::from_secs_f32(1.0 / hz);
    }

    pub fn tick(&mut self, frame_dt: Duration) {
        if let Some(budget) = &self.cpu_budget
            && budget.exceeded_hard_limit()
        {
            idle_log::error!(
                plugin = %self.plugin_path.display(),
                usage_us = budget.usage_micros(),
                limit_us = budget.hard_limit_us(),
                "CPU budget exceeded — dropping plugin session"
            );
            self.plugin = None; // Drop calls destroy_screensaver.
            self.needs_reload
                .store(true, std::sync::atomic::Ordering::Release);
            return;
        }
        if let Some(budget) = &mut self.gpu_budget
            && budget.sample_due()
        {
            match budget.sample() {
                Ok(pct) if budget.exceeded() => {
                    idle_log::error!(
                        plugin = %self.plugin_path.display(),
                        backend = budget.backend().as_str(),
                        usage_pct = pct,
                        ceiling_pct = budget.hard_ceiling(),
                        "GPU budget exceeded — dropping plugin session"
                    );
                    self.plugin = None;
                    self.needs_reload
                        .store(true, std::sync::atomic::Ordering::Release);
                    return;
                }
                Ok(_) => {
                    if budget.unhealthy() {
                        idle_log::warn!(
                            plugin = %self.plugin_path.display(),
                            backend = budget.backend().as_str(),
                            consecutive_failures = crate::gpu_budget::DEFAULT_FAILURE_STREAK,
                            "GPU budget tool reporting persistent failures — \
                             budget is silently unenforced; \
                             set IDLE_GPU_BUDGET=0 to disable until resolved"
                        );
                    }
                }
                Err(err) => {
                    idle_log::debug!(
                        backend = budget.backend().as_str(),
                        "gpu sample failed: {err}"
                    );
                }
            }
        }
        if let Some(plugin) = self.plugin.as_mut() {
            plugin.saver_mut().update_frame_time(frame_dt);
        }
        self.time_elapsed += frame_dt;

        self.physics_accumulator += frame_dt;
        if self.physics_accumulator > Duration::from_millis(100) {
            self.physics_accumulator = Duration::from_millis(100);
        }

        while self.physics_accumulator >= self.physics_duration {
            let dt = self.physics_duration;
            let cols = self.simulation_cols;
            let rows = self.simulation_rows;
            if let Some(plugin) = self.plugin.as_mut() {
                // Watchdog (Sprint 03 C): if the plugin runs longer than the
                // configured tick budget, drop the session rather than letting
                // a runaway saver stall the frame loop. This is wall-clock
                // only — true infinite-loop protection would require process
                // isolation, tracked as a residual.
                let guard = crate::watchdog::CallGuard::new(crate::watchdog::watchdog_timeout());
                plugin.saver_mut().update(dt, cols, rows);
                if guard.overflowed() {
                    idle_log::error!(
                        plugin = %self.plugin_path.display(),
                        elapsed_ms = guard.elapsed().as_millis(),
                        budget_ms = crate::watchdog::watchdog_timeout().as_millis(),
                        "plugin tick exceeded watchdog — dropping session"
                    );
                    self.plugin = None;
                    self.needs_reload
                        .store(true, std::sync::atomic::Ordering::Release);
                    break;
                }
            }
            self.physics_accumulator -= dt;
        }
    }

    pub fn blit_to_monitor_into(
        &mut self,
        src: &[u8],
        src_w: u32,
        src_h: u32,
        dst_w: u32,
        dst_h: u32,
        out: &mut Vec<u8>,
    ) {
        self.upscaler
            .upscale_letterbox_into(src, src_w, src_h, dst_w, dst_h, out);
    }

    pub fn draw_frame(&mut self, grid_cols: usize, grid_rows: usize) -> bool {
        if self.grid.len() != grid_cols * grid_rows {
            self.grid = vec![TerminalCell::default(); grid_cols * grid_rows];
        }
        if let Some(plugin) = self.plugin.as_mut() {
            let saver = plugin.saver_mut();
            saver.draw(&mut self.grid, grid_cols, grid_rows);
            saver.has_scanlines()
        } else {
            false
        }
    }
}
