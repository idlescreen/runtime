// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

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
