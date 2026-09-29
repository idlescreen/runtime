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

#[test]
fn config_dir_candidates_includes_idlescreen_and_etc() {
    let candidates = DaemonConfig::config_dir_candidates();
    assert!(
        candidates.iter().any(|p| p.ends_with("idlescreen")),
        "candidates must include an idlescreen directory"
    );
    assert!(
        candidates
            .iter()
            .any(|p| p == std::path::Path::new("/etc/idlescreen")),
        "candidates must include /etc/idlescreen"
    );
}

#[test]
fn get_config_path_preserves_existing_idlescreen_file() {
    let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join(format!("idle-test-cfg-{}", std::process::id()));
    let idlescreen_dir = tmp.join("idlescreen");
    std::fs::create_dir_all(&idlescreen_dir).unwrap();
    let cfg_file = idlescreen_dir.join("config.yaml");
    std::fs::write(&cfg_file, "idle_timeout_mins: 42\n").unwrap();

    let prior_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
    }

    let resolved = DaemonConfig::resolve_config_path();
    assert_eq!(resolved.as_ref(), Some(&cfg_file));
    let write_path = DaemonConfig::get_config_path();
    assert_eq!(write_path.as_ref(), Some(&cfg_file));

    match prior_xdg {
        Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
