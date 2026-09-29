// SPDX-License-Identifier: MIT

use idle_err::{Context, anyhow};
use idle_runner::launcher::{LaunchMode, resolve_saver_binary, sanitize_saver_name};

use super::{DaemonCommand, DaemonController};
use crate::config::DaemonConfig;

impl DaemonController {
    pub fn mutate_config<F>(&self, f: F) -> idle_err::Result<()>
    where
        F: FnOnce(&mut DaemonConfig),
    {
        let mut config = self
            .config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        let previous = config.clone();
        f(&mut config);
        if *config != previous {
            config.save().context("saving config")?;
            self.mark_dirty();
        }
        Ok(())
    }

    /// Apply on-disk config without writing back (file-watcher path).
    pub fn reload_config_from_disk(&self) -> idle_err::Result<()> {
        let mut config = self
            .config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        // Load while holding the lock so a concurrent mutate_config cannot
        // lose its in-memory write to a stale pre-lock snapshot.
        let fresh = DaemonConfig::load();
        if *config != fresh {
            *config = fresh;
            self.mark_dirty();
        }
        Ok(())
    }

    pub fn apply_command(&self, command: DaemonCommand) -> idle_err::Result<()> {
        match command {
            DaemonCommand::Enable => self
                .mutate_config(|c| c.idle_enabled = true)
                .context("persisting config after Enable command"),
            DaemonCommand::Disable => self
                .mutate_config(|c| c.idle_enabled = false)
                .context("persisting config after Disable command"),
            DaemonCommand::SetTimeout(minutes) => {
                validate_idle_timeout(minutes)?;
                self.mutate_config(|c| c.idle_timeout_mins = minutes)
                    .context("persisting config after SetTimeout command")
            }
            DaemonCommand::SetSaver(name) => {
                // Empty / random / none / shuffle (any case) → random rotation.
                // Capital "Random" is what the TUI label uses; must not resolve as a plugin name.
                let normalized = match name.as_deref() {
                    Some(s)
                        if s.is_empty()
                            || s.eq_ignore_ascii_case("random")
                            || s.eq_ignore_ascii_case("none")
                            || s.eq_ignore_ascii_case("shuffle") =>
                    {
                        None
                    }
                    other => other.map(String::from),
                };
                validate_saver_choice(normalized.as_deref())?;
                self.mutate_config(|c| c.active_saver = normalized)
                    .context("persisting config after SetSaver command")
            }
            DaemonCommand::SetShowFpsOverlay(enabled) => self
                .mutate_config(|c| c.show_fps_overlay = enabled)
                .context("persisting config after SetShowFpsOverlay command"),
            DaemonCommand::SetRenderScale(scale) => {
                let stored = normalize_render_scale(scale)?;
                self.mutate_config(|c| c.render_scale = stored)
                    .context("persisting config after SetRenderScale command")
            }
            // Presentation lifecycle commands own no config keys.
            DaemonCommand::Preview(_)
            | DaemonCommand::Activate
            | DaemonCommand::StopPresentation => Ok(()),
        }
    }
}

fn validate_idle_timeout(minutes: u32) -> idle_err::Result<()> {
    if minutes == 0 || minutes > 240 {
        idle_err::bail!("timeout must be between 1 and 240 minutes");
    }
    Ok(())
}

fn validate_saver_choice(saver: Option<&str>) -> idle_err::Result<()> {
    if let Some(name) = saver {
        if name.is_empty()
            || name.eq_ignore_ascii_case("random")
            || name.eq_ignore_ascii_case("none")
            || name.eq_ignore_ascii_case("shuffle")
        {
            return Ok(());
        }
        sanitize_saver_name(name)
            .ok_or_else(|| anyhow!("unknown or invalid screensaver name: {name}"))?;
        resolve_saver_binary(name, &LaunchMode::Daemon)
            .with_context(|| format!("resolving saver binary for {name}"))?;
    }
    Ok(())
}

fn validate_render_scale(scale: f32) -> idle_err::Result<()> {
    if !scale.is_finite() || !(0.25..=1.0).contains(&scale) {
        idle_err::bail!("render_scale must be between 0.25 and 1.0");
    }
    Ok(())
}

fn normalize_render_scale(scale: Option<f32>) -> idle_err::Result<Option<f32>> {
    let stored = match scale {
        None => None,
        Some(value) if value <= 0.0 => None,
        Some(value) => {
            validate_render_scale(value)?;
            Some(value)
        }
    };
    Ok(stored)
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "commands_validation_tests.rs"]
mod validation_tests;
