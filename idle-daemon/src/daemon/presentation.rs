// SPDX-License-Identifier: MIT

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use idle_api::OverlaySurface;
use idle_runner::launcher::{ALLOWED_SAVERS, is_allowed_saver};

use crate::config::DaemonConfig;
use crate::presentation::{PluginPresentation, PresentationOptions};

pub enum ActivePresentation {
    None,
    Plugin(PluginPresentation),
}

impl ActivePresentation {
    pub fn is_active(&self) -> bool {
        match self {
            Self::None => false,
            Self::Plugin(plugin) => plugin.is_running(),
        }
    }

    #[allow(clippy::collapsible_if)]
    pub fn process_exits(&mut self, current_saver: &mut String, preview_name: &mut Option<String>) {
        if let Self::Plugin(plugin) = self {
            if !plugin.is_running() {
                *self = Self::None;
                current_saver.clear();
                *preview_name = None;
            }
        }
    }

    #[allow(clippy::collapsible_if)]
    pub fn check_liveness(
        &mut self,
        preview_name: &mut Option<String>,
        current_saver: &mut String,
    ) {
        if let Self::Plugin(plugin) = self {
            if !plugin.is_running() {
                *self = Self::None;
                current_saver.clear();
                *preview_name = None;
            }
        }
    }
}

pub fn start_presentation(
    overlay_presenter: &Arc<dyn OverlaySurface>,
    presentation: &mut ActivePresentation,
    current_saver: &mut String,
    saver_name: String,
    reason: &str,
    config: &DaemonConfig,
) -> bool {
    idle_log::info!("starting Wayland screensaver '{saver_name}' ({reason})...");
    if !is_allowed_saver(&saver_name) {
        idle_log::error!(
            "failed to start screensaver: invalid or disallowed saver name '{saver_name}'"
        );
        return false;
    }
    let launch_mode = if reason == "preview" {
        idle_runner::launcher::LaunchMode::Preview
    } else {
        idle_runner::launcher::LaunchMode::Daemon
    };
    let options = PresentationOptions {
        show_fps_overlay: config.show_fps_overlay,
        render_scale: config.render_scale,
        launch_mode,
        saver_params: config.saver_params.clone(),
    };
    match PluginPresentation::start(overlay_presenter.clone(), saver_name.clone(), options) {
        Ok(plugin) => {
            *current_saver = saver_name;
            *presentation = ActivePresentation::Plugin(plugin);
            true
        }
        Err(error) => {
            idle_log::error!("failed to start screensaver: {error}");
            false
        }
    }
}

pub fn stop_presentation(
    overlay_presenter: Option<&Arc<dyn OverlaySurface>>,
    presentation: &mut ActivePresentation,
) {
    if let ActivePresentation::Plugin(plugin) = presentation {
        if let Some(presenter) = overlay_presenter {
            plugin.stop(&**presenter);
        }
        *presentation = ActivePresentation::None;
    }
}

pub fn current_time_micros() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_micros() as u64,
        Err(_) => 0,
    }
}

pub fn pick_saver_name(config: &DaemonConfig, seed_micros: u64) -> String {
    if let Some(active) = config
        .active_saver
        .as_deref()
        .filter(|&s| s == "random" || s == "shuffle" || is_allowed_saver(s))
        && active != "random"
        && active != "shuffle"
    {
        return active.to_string();
    }

    let savers = idle_runner::discovery::detect_screensavers();
    if !savers.is_empty() {
        let mut seed = seed_micros;
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let index = (seed % savers.len() as u64) as usize;
        return savers[index].clone();
    }

    let mut seed = seed_micros;
    seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let index = (seed % ALLOWED_SAVERS.len() as u64) as usize;
    ALLOWED_SAVERS[index].to_string()
}
