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
use idle_api::{IdleSource, OverlaySurface};

pub use super::recovery::*;

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
        Box::new(IdleMonitor::new_timeout(timeout).ok_or_else(|| {
            anyhow!(
                "DEGRADED: Wayland idle monitoring unavailable (need ext-idle-notify-v1). \
                 IdleScreen is a compositor client — this DE/compositor does not expose the \
                 idle protocol. See docs/BOUNDARIES.md. Run: idle doctor --json"
            )
        })?)
    };
    #[cfg(not(target_os = "linux"))]
    let idle_monitor: Box<dyn IdleSource> =
        idle_api::platform_idle(Duration::from_secs(idle_timeout.saturating_mul(60) as u64))
            .ok_or_else(|| anyhow!("DEGRADED: idle source unavailable on this platform."))?;

    idle_log::info!("using platform idle source");
    if !idle_monitor.is_alive() {
        return Err(anyhow!(
            "DEGRADED: idle source reports dead at startup; refusing to load"
        ));
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
    let overlay_presenter: Arc<dyn OverlaySurface> = idle_api::WaylandOverlay::new()
        .map(|w| Arc::new(w) as Arc<dyn OverlaySurface>)
        .ok_or_else(|| {
            anyhow!(
                "DEGRADED: Wayland layer-shell presenter unavailable (need zwlr_layer_shell_v1). \
                 IdleScreen presents as a guest overlay — compositors without layer-shell cannot \
                 host it (e.g. some GNOME configurations). See docs/BOUNDARIES.md. Run: idle doctor --json"
            )
        })?;
    idle_log::info!("using Wayland layer-shell presenter");
    Ok((idle_monitor, overlay_presenter))
}

pub fn check_runtime_alive(
    idle_monitor: &dyn IdleSource,
    overlay_presenter: &dyn OverlaySurface,
) -> Result<(), RuntimeFault> {
    classify_runtime(idle_monitor.is_alive(), overlay_presenter.is_alive())
}
