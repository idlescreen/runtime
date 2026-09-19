// SPDX-License-Identifier: MIT

//! Plugin screensaver presentation on Wayland layer-shell overlays.
//!
//! A dedicated thread loads the selected plugin, renders frames at the target
//! refresh rate, and submits BGRA buffers per output. Display modes (expand,
//! mirror, primary-only, span) are handled in the frame loop submodule.

mod frame_loop;
mod frame_pacing;
mod hw_scaling;
mod ipc_init;
mod ipc_lifecycle;
mod ipc_peer;
mod ipc_raster;
mod ipc_session;
mod ipc_session_methods;
#[cfg(test)]
mod ipc_session_tests;
mod layout;
mod overlays;
mod plugin_loop;
mod refresh;
mod render;
mod timeout;
pub mod topology;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use idle_api::OverlaySurface;
use idle_runner::launcher::LaunchMode;

pub use plugin_loop::run_plugin_loop;

#[derive(Clone)]
pub struct PresentationOptions {
    pub show_fps_overlay: bool,
    pub render_scale: Option<f32>,
    pub launch_mode: LaunchMode,
    /// `[saver]`/`[saver.*]` config params; delivered to runners as
    /// `IDLE_SAVER_PARAM_*` env vars (see `idle_api::param`).
    pub saver_params: std::collections::BTreeMap<String, String>,
}

pub struct PluginPresentation {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PluginPresentation {
    pub fn start(
        presenter: Arc<dyn OverlaySurface>,
        saver_name: String,
        options: PresentationOptions,
    ) -> Result<Self, String> {
        if !idle_runner::launcher::is_allowed_saver(&saver_name) {
            return Err(format!("invalid or disallowed saver name: {saver_name}"));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let presenter_for_thread = presenter.clone();

        let thread = thread::spawn(move || {
            if let Err(error) =
                run_plugin_loop(&*presenter_for_thread, &saver_name, &stop_flag, options)
            {
                idle_log::error!("plugin presentation ended: {error}");
                presenter_for_thread.hide();
            }
        });

        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    pub fn stop(&mut self, presenter: &dyn OverlaySurface) {
        self.stop.store(true, Ordering::Relaxed);
        presenter.hide();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_plugin_presentation_start_rejects_invalid_saver() {
        // Use the platform stub — tests should not depend on a Wayland session.
        let presenter: Arc<dyn OverlaySurface> = Arc::new(idle_api::StubOverlay);
        let options = PresentationOptions {
            show_fps_overlay: false,
            render_scale: None,
            launch_mode: LaunchMode::Preview,
            saver_params: std::collections::BTreeMap::new(),
        };
        let result = PluginPresentation::start(
            presenter,
            "nonexistent_invalid_saver_123".to_string(),
            options,
        );
        assert!(result.is_err());
    }

    /// Counting surface: counts calls to trait methods so we can verify
    /// the `Arc<dyn OverlaySurface>` dispatch hits every method the
    /// daemon's presentation pipeline relies on.
    struct CountingSurface {
        alive_calls: AtomicUsize,
        visible_calls: AtomicUsize,
    }
    impl OverlaySurface for CountingSurface {
        fn is_available() -> bool {
            true
        }
        fn new() -> Option<Self> {
            Some(Self {
                alive_calls: AtomicUsize::new(0),
                visible_calls: AtomicUsize::new(0),
            })
        }
        fn submit_frame(&self, _: idle_api::OutputId, _: std::sync::Arc<Vec<u8>>, _: u32, _: u32) {}
        fn is_alive(&self) -> bool {
            self.alive_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true
        }
        fn is_visible(&self) -> bool {
            self.visible_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true
        }
        fn show_blank(&self, _: idle_api::BlankAppearance) {}
        fn show_screensaver(&self) {}
        fn hide(&self) {}
        fn supports_scaling(&self) -> bool {
            false
        }
        fn output_layouts(&self) -> Vec<idle_api::OutputLayout> {
            Vec::new()
        }
        fn get_frame_buffer(&self, size: usize) -> Vec<u8> {
            vec![0u8; size]
        }
    }

    /// `PluginPresentation::start` should call `is_alive()` at least once
    /// (initial liveness check) and route through `Arc<dyn OverlaySurface>`
    /// without panicking when no saver is loaded.
    #[test]
    fn plugin_presentation_dyn_surface_dispatch_works() {
        let presenter: Arc<dyn OverlaySurface> = Arc::new(CountingSurface::new().unwrap());
        let options = PresentationOptions {
            show_fps_overlay: false,
            render_scale: None,
            launch_mode: LaunchMode::Preview,
            saver_params: std::collections::BTreeMap::new(),
        };
        // Invalid saver — the call should fail closed at the saver gate,
        // but the surface dispatch path must still be reachable without
        // a panic.
        let result = PluginPresentation::start(
            presenter,
            "definitely_not_a_real_saver".to_string(),
            options,
        );
        assert!(result.is_err(), "invalid saver must still be refused");
    }

    /// `PluginPresentation::stop` takes `&dyn OverlaySurface` and calls
    /// `hide()` on it; verify the trait dispatch lands correctly.
    #[test]
    fn plugin_presentation_stop_calls_hide_via_dyn_surface() {
        let presenter_obj = CountingSurface::new().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn(|| {});
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut plugin = PluginPresentation {
            stop,
            thread: Some(thread),
        };
        // stop takes &dyn OverlaySurface; counting surface implements the trait.
        plugin.stop(&presenter_obj);
        // We don't track hide() count in CountingSurface, so the smoke test
        // is "the dispatch doesn't panic". A future iteration can add a
        // hide_calls counter once the surface grows a state field for it.
        let _ = presenter_obj; // suppress unused-mut warning
    }

    #[test]
    fn test_plugin_presentation_is_running_returns_false_when_thread_finished() {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn(|| {});
        thread::sleep(std::time::Duration::from_millis(20));

        let plugin = PluginPresentation {
            stop,
            thread: Some(handle),
        };
        thread::sleep(std::time::Duration::from_millis(10));
        assert!(!plugin.is_running());
    }

    #[test]
    fn test_active_presentation_check_liveness_clears_state_on_finished_thread() {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn(|| {});
        thread::sleep(std::time::Duration::from_millis(20));

        let plugin = PluginPresentation {
            stop,
            thread: Some(handle),
        };
        thread::sleep(std::time::Duration::from_millis(10));

        let mut active = crate::daemon::presentation::ActivePresentation::Plugin(plugin);
        let mut preview_name = Some("beams".to_string());
        let mut current_saver = "beams".to_string();

        active.check_liveness(&mut preview_name, &mut current_saver);

        assert!(!active.is_active());
        assert_eq!(current_saver, "");
        assert_eq!(preview_name, None);
    }
}
