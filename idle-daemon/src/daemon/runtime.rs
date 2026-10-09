// SPDX-License-Identifier: MIT

//! Platform-agnostic runtime initialization and liveness checks.
//!
//! On Linux the idle source is `wayland_idle::IdleMonitor` (the `ext-idle-notify-v1`
//! implementation); on other targets `idle_api::platform_idle()` returns a
//! `Box<dyn IdleSource>` stub that fails closed (Sprint 05 will replace it).
//!
//! Overlay surface: Linux returns `Arc<dyn OverlaySurface>` via the
//! `WaylandOverlay` adapter that lifts `OverlayPresenter` onto the
//! platform-agnostic trait. Sprint 05 H1/H2 drop in macOS/Windows impls
//! without changing daemon call sites.

use idle_err::anyhow;
use std::sync::Arc;
use std::time::Duration;

use crate::controller::DaemonController;
use idle_api::{BlankAppearance, IdleSource, OutputId, OutputLayout, OverlaySurface};

pub use super::recovery::*;

/// Gracefully degraded idle source for GNOME/compositors without ext-idle-notify-v1.
#[derive(Debug)]
pub struct DegradedIdleSource {
    _timeout: Duration,
}

impl IdleSource for DegradedIdleSource {
    fn is_available() -> bool {
        false
    }

    fn new(timeout: Duration) -> Option<Self> {
        Some(Self { _timeout: timeout })
    }

    fn is_idle(&self) -> bool {
        false
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn set_timeout(&self, timeout: Duration) {
        let _ = timeout;
    }
}

/// Gracefully degraded overlay surface for GNOME/compositors without zwlr_layer_shell_v1.
#[derive(Debug)]
pub struct DegradedOverlay;

impl OverlaySurface for DegradedOverlay {
    fn is_available() -> bool {
        false
    }

    fn new() -> Option<Self> {
        Some(Self)
    }

    fn submit_frame(&self, _output: OutputId, _frame: Arc<Vec<u8>>, _width: u32, _height: u32) {}

    fn is_alive(&self) -> bool {
        true
    }

    fn is_visible(&self) -> bool {
        false
    }

    fn show_blank(&self, _appearance: BlankAppearance) {}

    fn show_screensaver(&self) {}

    fn hide(&self) {}

    fn supports_scaling(&self) -> bool {
        false
    }

    fn output_layouts(&self) -> Vec<OutputLayout> {
        Vec::new()
    }
}

/// Log the daemon's posture w.r.t. fail-OPEN defaults. Operators who want
/// full enforcement must opt in (see `DEPLOYMENT.md` in the org repo).
/// The daemon never refuses to start on permissive defaults — but it
/// does log them loudly so the deployment audit log shows the posture.
pub fn log_posture() {
    let manifest_sig = std::env::var_os("IDLE_REQUIRE_MANIFEST_SIGNATURE").is_some();
    let gpu_budget = std::env::var_os("IDLE_GPU_BUDGET").is_some();
    let cpu_fail_closed = std::env::var_os("IDLE_REQUIRE_CPU_BUDGET").is_some();
    let sandbox_off = std::env::var_os("IDLE_DISABLE_SANDBOX").is_some();
    let unsigned_off = std::env::var_os("IDLE_ALLOW_UNSIGNED_PLUGINS").is_some();

    if !manifest_sig || !gpu_budget || !cpu_fail_closed {
        idle_log::warn!(
            manifest_signature_enforced = manifest_sig,
            gpu_budget_enforced = gpu_budget,
            cpu_budget_fail_closed = cpu_fail_closed,
            "IdleScreen starting with one or more fail-OPEN defaults; \
             see DEPLOYMENT.md for the recommended systemd Environment= lines. \
             At minimum set IDLE_REQUIRE_MANIFEST_SIGNATURE=1, IDLE_GPU_BUDGET=1, \
             IDLE_REQUIRE_CPU_BUDGET=1 in production."
        );
    }
    if sandbox_off {
        idle_log::error!(
            "IDLE_DISABLE_SANDBOX=1 — Landlock sandbox BYPASSED. \
             Plugins run with full filesystem + network access. \
             This is a debug-only flag; production deployments MUST NOT set it."
        );
    }
    if unsigned_off {
        idle_log::error!(
            "IDLE_ALLOW_UNSIGNED_PLUGINS=1 — manifest gate BYPASSED. \
             Plugins without an .idleplugin.toml are accepted. \
             This is a debug-only flag; production deployments MUST NOT set it."
        );
    }
}

pub fn initialize_runtime(
    controller: &DaemonController,
) -> idle_err::Result<(Box<dyn IdleSource>, Arc<dyn OverlaySurface>)> {
    let idle_timeout = controller
        .config
        .lock()
        .unwrap_or_else(|p| crate::locks::poison_or_exit("config", p))
        .idle_timeout_mins;

    #[cfg(target_os = "linux")]
    let idle_monitor: Box<dyn IdleSource> = {
        use wayland_idle::IdleMonitor;
        let timeout = Duration::from_secs(idle_timeout.saturating_mul(60) as u64);
        if let Some(monitor) = IdleMonitor::new_timeout(timeout) {
            idle_log::info!("using platform idle source");
            Box::new(monitor)
        } else if let Some(gnome) = crate::monitors::GnomeIdleMonitor::new(timeout) {
            idle_log::info!("using GNOME Mutter idle monitor");
            Box::new(gnome)
        } else {
            idle_log::warn!(
                "DEGRADED: Wayland idle monitoring unavailable (need ext-idle-notify-v1 or org.gnome.Mutter.IdleMonitor). \
                 IdleScreen is running in degraded mode on compositor without idle protocol. \
                 D-Bus interface remains active."
            );
            Box::new(DegradedIdleSource { _timeout: timeout })
        }
    };
    #[cfg(not(target_os = "linux"))]
    let idle_monitor: Box<dyn IdleSource> = {
        let timeout = Duration::from_secs(idle_timeout.saturating_mul(60) as u64);
        if let Some(source) = idle_api::platform_idle(timeout) {
            source
        } else {
            Box::new(DegradedIdleSource { _timeout: timeout })
        }
    };

    if idle_monitor.is_alive() {
        idle_log::info!("platform idle source is alive");
    } else {
        idle_log::warn!(
            "DEGRADED: idle source reports dead or degraded at startup; continuing in degraded mode"
        );
    }

    if !idle_runner::cell_renderer::font_available() {
        return Err(anyhow!(
            "DEGRADED: no monospace font found; install fonts-dejavu-core (or equivalent) before running idle. Run: idle doctor"
        ));
    }
    if let Some(path) = idle_runner::cell_renderer::resolve_font_path() {
        idle_log::info!("using monospace font: {path}");
    }

    // Trait seam: prefer `WaylandOverlay::new()` so the daemon can later
    // hold `Arc<dyn OverlaySurface>` instead of `Arc<OverlayPresenter>`.
    // For now we unwrap to the concrete presenter (the trait's `is_visible`,
    // `submit_frame`, `is_alive` are all the same signature as the
    // presenter's, except for Arc<Vec<u8>> vs Vec<u8>). The concrete path
    // is used by the presentation pipeline (which has additional methods
    // like `show_screensaver`); Sprint 05 will move those onto the trait.
    let overlay_presenter: Arc<dyn OverlaySurface> = match idle_api::WaylandOverlay::new() {
        Some(presenter) => {
            idle_log::info!("using Wayland presenter");
            Arc::new(presenter)
        }
        None => {
            idle_log::warn!(
                "DEGRADED: Wayland presenter unavailable (need zwlr_layer_shell_v1 or xdg_wm_base). \
                 IdleScreen is running in degraded mode on compositor without presentation protocol. \
                 D-Bus interface remains active."
            );
            Arc::new(DegradedOverlay)
        }
    };
    Ok((idle_monitor, overlay_presenter))
}

pub fn check_runtime_alive(
    idle_monitor: &dyn IdleSource,
    overlay_presenter: &dyn OverlaySurface,
) -> Result<(), RuntimeFault> {
    classify_runtime(idle_monitor.is_alive(), overlay_presenter.is_alive())
}
