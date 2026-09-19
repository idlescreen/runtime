// SPDX-License-Identifier: MIT

//! Plugin loading orchestration: manifest gate → Landlock → CPU budget →
//! ABI check → `Library::new` → entry-point resolution → `PluginGuard`.
//!
//! Sandbox is applied **before** `Library::new` so ELF constructors fire
//! under the policy. A failure at any step fails closed and refuses to load.

use super::entry::{check_entry, resolve_entry};
use super::{PluginGuard, PluginSession, manifest_gate};
use crate::budget::{self, AttachOutcome};
use crate::cell_renderer::CellRenderer;
use crate::launcher::{LaunchMode, PluginError, resolve_saver_binary};

use crate::dylib::Library;
use idle_upscaler::{FilterMode, FrameUpscaler, resolve_render_scale};
use std::path::Path;
use std::time::Duration;

/// Assert the manifest's entry block describes the library we resolved.
impl PluginSession {
    pub fn load(saver_name: &str) -> Result<Self, PluginError> {
        Self::load_with_options(saver_name, &LaunchMode::Daemon, None)
    }

    pub fn load_with_options(
        saver_name: &str,
        launch_mode: &LaunchMode,
        render_scale: Option<f32>,
    ) -> Result<Self, PluginError> {
        let path = resolve_saver_binary(saver_name, launch_mode)?;
        idle_log::info!(
            "idle-runner: loading plugin '{}' from {}",
            saver_name,
            path.display()
        );
        Self::load_path_with_options(&path, render_scale)
    }

    pub fn load_path_with_options(
        path: &Path,
        render_scale: Option<f32>,
    ) -> Result<Self, PluginError> {
        let renderer = CellRenderer::new().map_err(|error| {
            PluginError::Io(std::io::Error::new(std::io::ErrorKind::Other, error))
        })?;
        let render_scale = resolve_render_scale(render_scale);
        let upscaler = FrameUpscaler::new(FilterMode::from_env());
        idle_log::info!("CPU upscale (render scale {:.0}%)", render_scale * 100.0);

        if std::env::var_os("IDLESCREEN_RENDER_SEED").is_some()
            || std::env::var_os("RENDER_SEED").is_some()
            || std::env::var_os("IDLE_RENDER_SEED").is_some()
        {
            idle_api::set_env("IDLE_EXPORT_MODE", "1");
        }
        let sys_info = if idle_api::SystemInfo::export_mode_enabled() {
            idle_api::SystemInfo::export_fixture()
        } else {
            crate::toolkit::sys_info::get_system_info()
        };
        idle_api::set_env("IDLE_OS_NAME", &sys_info.os);
        idle_api::set_env("IDLE_LOGO_TEXT", &sys_info.logo_text);

        crate::caption_overlay::init_font();

        // Manifest gate
        let manifest = manifest_gate::load_manifest_for(path)?;
        match manifest.as_deref() {
            Some(m) => crate::sandbox::enforce_sandbox_for_plugin_with_manifest(path, m),
            None => crate::sandbox::enforce_sandbox_for_plugin(path),
        }
        .map_err(crate::launcher::PluginError::Sandbox)?;

        // CPU budget (Sprint 03 B)
        let cpu_budget = match budget::attach_for_path(path)? {
            AttachOutcome::Enforced(b) => {
                idle_log::info!(
                    quota_us = b.quota_us(),
                    period_us = b.period_us(),
                    "CPU budget enforced via cgroup v2"
                );
                Some(b)
            }
            AttachOutcome::Unenforced(b) => {
                if std::env::var_os("IDLE_REQUIRE_CPU_BUDGET").is_some() {
                    b.release();
                    return Err(PluginError::Io(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "IDLE_REQUIRE_CPU_BUDGET=1 but cgroup v2 is not writable",
                    )));
                }
                idle_log::warn!(
                    "CPU budget UNENFORCED — cgroup v2 unavailable; \
                     in-process measurement only. Set IDLE_REQUIRE_CPU_BUDGET=1 \
                     to refuse this state."
                );
                Some(b)
            }
        };

        // GPU budget (Sprint 04 G1). Opt-in via IDLE_GPU_BUDGET=1; default
        // off because vendor tools are optional and the probe is a child
        // process per sample. Vendor detection is best-effort; if no
        // backend is found, we leave the field None and log once.
        let gpu_budget = if crate::gpu_budget::gpu_budget_enabled() {
            match crate::gpu_budget::GpuBudget::detect() {
                crate::gpu_budget::GpuStatus::Active(backend) => {
                    idle_log::info!(
                        backend = backend.as_str(),
                        quota_pct = crate::gpu_budget::DEFAULT_GPU_QUOTA_PCT,
                        "GPU budget active"
                    );
                    Some(crate::gpu_budget::GpuBudget::new_active(backend))
                }
                crate::gpu_budget::GpuStatus::Unavailable => {
                    idle_log::warn!(
                        "IDLE_GPU_BUDGET=1 but no vendor tool found (nvidia-smi / \
                         intel_gpu_top / amdgpu_top); budget unenforced"
                    );
                    None
                }
            }
        } else {
            None
        };

        unsafe {
            let lib = Library::new(path)?;

            // Manifest entry check before any plugin code runs: ELF
            // constructors already fired inside the sandbox at `Library::new`,
            // but we still validate the declared entry point before calling
            // into the plugin for the first time.
            if let Some(m) = manifest.as_deref() {
                check_entry(m, path)?;
            }

            // ABI negotiation: REQUIRED. Every idle-saver-* crate ships
            // an `idle_api_version` symbol (added in this rotation); plugins
            // that don't are refused as `MissingVersion` so a malicious or
            // stale plugin cannot slip past the version check.
            let ver_sym = lib.get::<unsafe extern "C" fn() -> u32>(b"idle_api_version");
            let ver_fn = match ver_sym {
                Ok(f) => *f,
                Err(_) => {
                    return Err(PluginError::MissingVersion);
                }
            };
            let found = ver_fn();
            let expected = idle_api::API_VERSION;
            if found != expected {
                return Err(PluginError::ApiVersionMismatch { found, expected });
            }
            idle_log::info!(found, expected, "plugin API version ok");

            let (raw_ptr, destroy) = resolve_entry(&lib)?;

            let guard = PluginGuard {
                ptr: raw_ptr,
                destroy,
                _lib: lib,
            };

            Ok(Self {
                plugin: Some(guard),
                plugin_path: path.to_path_buf(),
                manifest,
                renderer,
                upscaler,
                render_scale,
                grid: Vec::new(),
                content_buf: Vec::new(),
                pixel_buf: std::sync::Arc::new(Vec::new()),
                physics_accumulator: Duration::ZERO,
                physics_duration: Duration::from_secs_f32(1.0 / 120.0),
                time_elapsed: Duration::ZERO,
                simulation_cols: 0,
                simulation_rows: 0,
                hardware_scaling: false,
                watcher: None,
                needs_reload: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                cpu_budget,
                gpu_budget,
            })
        }
    }
}
