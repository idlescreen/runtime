// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

use std::fs;
use std::path::PathBuf;

use crate::config_parse::apply_config_line;

#[derive(Debug, Clone, PartialEq)]
pub struct DaemonConfig {
    pub active_saver: Option<String>,
    pub idle_enabled: bool,
    pub idle_timeout_mins: u32,

    pub show_fps_overlay: bool,
    /// Simulation grid scale override in `(0.25, 1.0]`; `None` uses CPU
    /// defaults (the GPU path was removed in 2026).
    pub render_scale: Option<f32>,
    /// Per-saver custom parameters (e.g. speed, density)
    pub saver_params: std::collections::BTreeMap<String, String>,
    pub theme: idle_api::Theme,
    /// When true, D-Bus control auth refuses the `/proc/pid/comm` fallback
    /// (exe must resolve to a trusted basename). Env `IDLE_STRICT_CONTROL=1`
    /// also enables this at process start.
    pub strict_control: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            active_saver: Some("beams".to_string()),
            idle_enabled: true,
            idle_timeout_mins: 5,
            show_fps_overlay: false,
            render_scale: None,
            saver_params: std::collections::BTreeMap::new(),
            theme: idle_api::Theme::default(),
            strict_control: false,
        }
    }
}

/// Absolute path without `..` or NUL — blocks env-based config path traversal.
fn is_safe_config_root(path: &str) -> bool {
    if path.is_empty() || path.contains('\0') {
        return false;
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    !p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

impl DaemonConfig {
    /// `IDLE_CONFIG_DIR` override (absolute, no `..`) for tests/debug.
    fn config_dir_override() -> Option<PathBuf> {
        std::env::var("IDLE_CONFIG_DIR")
            .ok()
            .filter(|d| is_safe_config_root(d))
            .map(PathBuf::from)
    }

    /// Config directory candidates: IdleScreen first, legacy `trance` second.
    pub fn config_dir_candidates() -> Vec<PathBuf> {
        let mut bases = Vec::new();
        if let Some(xdg) = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|s| is_safe_config_root(s))
        {
            bases.push(PathBuf::from(xdg));
        }
        if let Ok(home) = std::env::var("HOME")
            && is_safe_config_root(&home)
        {
            bases.push(PathBuf::from(home).join(".config"));
        }
        let mut dirs = Vec::new();
        for base in bases {
            dirs.push(base.join("idle"));
            dirs.push(base.join("trance"));
        }
        dirs
    }

    /// Path used for **writes** and new installs (`~/.config/idle/config.yaml`).
    pub fn get_config_path() -> Option<PathBuf> {
        if let Some(dir) = Self::config_dir_override() {
            return Some(dir.join("config.yaml"));
        }
        Self::config_dir_candidates()
            .into_iter()
            .find(|d| d.ends_with("idle"))
            .map(|d| d.join("config.yaml"))
    }

    /// Resolve existing config for **reads**: prefer IdleScreen, fall back to legacy.
    pub fn resolve_config_path() -> Option<PathBuf> {
        if let Some(dir) = Self::config_dir_override() {
            return Some(dir.join("config.yaml"));
        }
        let candidates: Vec<PathBuf> = Self::config_dir_candidates()
            .into_iter()
            .map(|d| d.join("config.yaml"))
            .collect();
        candidates
            .iter()
            .find(|p| p.is_file())
            .cloned()
            .or_else(|| candidates.into_iter().next())
    }

    pub fn load() -> Self {
        let mut config = Self::default();
        let resolved = Self::resolve_config_path();
        if let Some(Ok(content)) = resolved.as_ref().map(fs::read_to_string) {
            let mut current_section = String::new();
            for line in content.lines() {
                apply_config_line(&mut config, &mut current_section, line);
            }
        }
        // Soft migrate: if we only had ~/.config/trance, copy to idle and write there next.
        if let (Some(src), Some(dst_path)) = (resolved, Self::get_config_path()) {
            let is_legacy = src.components().any(|c| c.as_os_str() == "trance");
            let idle_missing = !dst_path.is_file();
            if is_legacy && idle_missing {
                if let Some(parent) = dst_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::copy(&src, &dst_path).is_ok() {
                    idle_log::info!(
                        target: "idle_daemon::config",
                        "migrated config {} → {}",
                        src.display(),
                        dst_path.display()
                    );
                }
            }
        }
        config
    }

    /// Daemon-owned keys written to config.yaml. Everything else in the
    /// file (accent_color, theme_idx, user comments, unknown keys) belongs
    /// to other tools and is preserved verbatim by `save`.
    fn rendered_fields(&self) -> Vec<(&'static str, String)> {
        let active_str = self.active_saver.as_deref().unwrap_or("none");
        vec![
            ("idle_timeout_mins", self.idle_timeout_mins.to_string()),
            ("active_saver", format!("\"{active_str}\"")),
            ("idle_enabled", self.idle_enabled.to_string()),
            ("show_fps_overlay", self.show_fps_overlay.to_string()),
            (
                "render_scale",
                self.render_scale
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "null".to_string()),
            ),
            ("theme", format!("\"{}\"", self.theme)),
            ("strict_control", self.strict_control.to_string()),
        ]
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::get_config_path() else {
            return Ok(());
        };
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no parent dir"))?;
        fs::create_dir_all(parent)?;
        // Serialize read-modify-write against other writers (applet, TUI)
        // via a sidecar lock — unlocked by drop when the write finishes.
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(parent.join("config.yaml.lock"))?;
        lock.lock()?;
        // Read-modify-write: only daemon-owned keys are rewritten; foreign
        // keys and comments survive so co-writing tools cannot clobber them.
        let mut existing = fs::read_to_string(&path).unwrap_or_default();
        if existing.trim().is_empty() && path.is_file() {
            // A non-atomic co-writer's O_TRUNC window can yield a momentary
            // empty read — retry once before trusting it, otherwise the
            // empty-file template would stamp defaults over real settings.
            std::thread::sleep(std::time::Duration::from_millis(25));
            existing = fs::read_to_string(&path).unwrap_or_default();
        }
        if !existing.is_empty() {
            // Last-known-good snapshot so a bad write is recoverable.
            let _ = fs::write(parent.join("config.yaml.bak"), &existing);
        }
        let content = crate::config_parse::merge_config_body(
            &existing,
            &mut self.rendered_fields(),
            &self.saver_params,
        );
        static TMP_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let count = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp_path = parent.join(format!("config.tmp.{}.{}", std::process::id(), count));
        fs::write(&tmp_path, &content)?;
        match fs::rename(&tmp_path, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&tmp_path);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_5_minute_timeout() {
        let c = DaemonConfig::default();
        assert_eq!(c.idle_timeout_mins, 5);
    }

    #[test]
    fn default_saver_is_beams() {
        let c = DaemonConfig::default();
        assert_eq!(c.active_saver.as_deref(), Some("beams"));
    }

    #[test]
    fn safe_config_root_rejects_traversal() {
        assert!(is_safe_config_root("/home/user/.config"));
        assert!(!is_safe_config_root(""));
        assert!(!is_safe_config_root("relative"));
        assert!(!is_safe_config_root("/home/user/../etc"));
    }

    #[test]
    fn default_idle_enabled() {
        let c = DaemonConfig::default();
        assert!(c.idle_enabled);
    }

    #[test]
    fn default_render_scale_is_none() {
        let c = DaemonConfig::default();
        assert!(c.render_scale.is_none());
    }

    #[test]
    fn default_show_fps_overlay_false() {
        let c = DaemonConfig::default();
        assert!(!c.show_fps_overlay);
    }
}
