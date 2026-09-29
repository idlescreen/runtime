// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! D-Bus / CLI status field contract — package-gate stable surface.

use super::DaemonStatus;

/// Canonical status map keys (order not guaranteed in HashMap).
pub const STATUS_FIELD_KEYS: &[&str] = &[
    "running",
    "idle_enabled",
    "idle_timeout_mins",
    "active_saver",
    "presentation_active",
    "preview_active",
    "system_idle",
    "session_locked",
    "inhibited",
    "current_saver",
    "show_fps_overlay",
    "render_scale",
];

/// Number of status fields exposed over D-Bus.
pub const STATUS_FIELD_COUNT: usize = 12;

/// True when `map` contains every contract key.
pub fn status_map_has_contract_keys(keys: impl Iterator<Item = impl AsRef<str>>) -> bool {
    let set: std::collections::HashSet<String> = keys.map(|k| k.as_ref().to_string()).collect();
    STATUS_FIELD_KEYS
        .iter()
        .all(|required| set.contains(*required))
}

/// Build a status used in contract tests.
pub fn sample_preview_status() -> DaemonStatus {
    DaemonStatus {
        running: true,
        idle_enabled: true,
        idle_timeout_mins: 5,
        active_saver: "ripple".into(),
        presentation_active: true,
        preview_active: true,
        system_idle: false,
        session_locked: false,
        inhibited: true,
        current_saver: "beams".into(),
        show_fps_overlay: false,
        render_scale: "0.5".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_key_count_matches() {
        assert_eq!(STATUS_FIELD_KEYS.len(), STATUS_FIELD_COUNT);
    }

    #[test]
    fn default_map_has_all_contract_keys() {
        let map = DaemonStatus::default().to_map();
        assert_eq!(map.len(), STATUS_FIELD_COUNT);
        assert!(status_map_has_contract_keys(map.keys().map(|s| s.as_str())));
    }

    #[test]
    fn preview_status_map_has_all_contract_keys() {
        let map = sample_preview_status().to_map();
        assert_eq!(map.len(), STATUS_FIELD_COUNT);
        for key in STATUS_FIELD_KEYS {
            assert!(map.contains_key(*key), "missing status key {key}");
        }
    }

    #[test]
    fn contract_includes_preview_and_inhibited() {
        assert!(STATUS_FIELD_KEYS.contains(&"preview_active"));
        assert!(STATUS_FIELD_KEYS.contains(&"presentation_active"));
        assert!(STATUS_FIELD_KEYS.contains(&"inhibited"));
        assert!(STATUS_FIELD_KEYS.contains(&"session_locked"));
    }

    #[test]
    fn missing_key_fails_contract() {
        let keys = ["running", "idle_enabled"];
        assert!(!status_map_has_contract_keys(keys.iter().copied()));
    }

    #[test]
    fn no_unknown_required_key_typos() {
        // Guard renames that would break CLI/applet without a coordinated bump.
        for key in STATUS_FIELD_KEYS {
            assert!(!key.is_empty());
            assert!(key.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }

    #[test]
    fn sample_preview_flags_are_independent() {
        // inhibited can be true while preview_active is true (forced preview).
        let s = sample_preview_status();
        assert!(s.inhibited);
        assert!(s.preview_active);
        assert!(s.presentation_active);
    }
}
