// SPDX-License-Identifier: MIT

use std::sync::atomic::Ordering;

use super::DaemonController;

impl DaemonController {
    #[allow(clippy::fn_params_excessive_bools)]
    pub fn update_live_state(
        &self,
        system_idle: bool,
        presentation_active: bool,
        preview_active: bool,
        current_saver: &str,
        effective_inhibited: bool,
    ) {
        // Lock order: config → inhibitors/logind → status (never hold status
        // across `is_inhibited()`, which may block on system D-Bus).
        let config = self
            .config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .clone();
        let session_locked = self.session_locked.load(Ordering::Relaxed);
        let inhibited = effective_inhibited || self.inhibitors.is_inhibited();
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        let changed = Self::apply_live_fields(
            &mut status,
            &config,
            system_idle,
            presentation_active,
            preview_active,
            current_saver,
            session_locked,
            inhibited,
        );
        if changed {
            self.status_dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn reload_config_if_due(&self, tick_counter: u32) -> Option<u32> {
        if !tick_counter.is_multiple_of(10) {
            return None;
        }
        let reloaded = crate::config::DaemonConfig::load();
        let mut config = self
            .config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        let previous_timeout = config.idle_timeout_mins;
        if *config != reloaded {
            *config = reloaded;
            self.mark_dirty();
        }
        if config.idle_timeout_mins != previous_timeout {
            Some(config.idle_timeout_mins)
        } else {
            None
        }
    }

    /// Update live fields in place; only allocate strings when values change.
    #[allow(clippy::fn_params_excessive_bools)]
    pub(crate) fn apply_live_fields(
        status: &mut idle_dbus::DaemonStatus,
        config: &crate::config::DaemonConfig,
        system_idle: bool,
        presentation_active: bool,
        preview_active: bool,
        current_saver: &str,
        session_locked: bool,
        inhibited: bool,
    ) -> bool {
        let mut changed = false;

        macro_rules! set_bool {
            ($field:ident, $val:expr) => {
                if status.$field != $val {
                    status.$field = $val;
                    changed = true;
                }
            };
        }

        status.running = true;
        set_bool!(system_idle, system_idle);
        set_bool!(presentation_active, presentation_active);
        set_bool!(preview_active, preview_active);
        set_bool!(session_locked, session_locked);
        set_bool!(inhibited, inhibited);
        set_bool!(idle_enabled, config.idle_enabled);
        set_bool!(show_fps_overlay, config.show_fps_overlay);

        if status.idle_timeout_mins != config.idle_timeout_mins {
            status.idle_timeout_mins = config.idle_timeout_mins;
            changed = true;
        }

        let active_saver = config.active_saver.as_deref().unwrap_or("");
        if status.active_saver != active_saver {
            status.active_saver.clear();
            status.active_saver.push_str(active_saver);
            changed = true;
        }

        if status.current_saver != current_saver {
            status.current_saver.clear();
            status.current_saver.push_str(current_saver);
            changed = true;
        }

        // Format render_scale only when the displayed value would change.
        match config.render_scale {
            None => {
                if !status.render_scale.is_empty() {
                    status.render_scale.clear();
                    changed = true;
                }
            }
            Some(scale) => {
                if !render_scale_matches(&status.render_scale, scale) {
                    status.render_scale.clear();
                    use std::fmt::Write;
                    let _ = write!(&mut status.render_scale, "{scale}");
                    changed = true;
                }
            }
        }

        changed
    }
}

/// True when `existing` is the Display form of `scale` (avoids alloc on steady state).
pub(crate) fn render_scale_matches(existing: &str, scale: f32) -> bool {
    // Fast path: parse existing and compare with a small epsilon.
    match existing.parse::<f32>() {
        Ok(v) => (v - scale).abs() <= f32::EPSILON * 8.0,
        Err(_) => false,
    }
}
