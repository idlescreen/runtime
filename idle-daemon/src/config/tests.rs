// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

use super::*;

#[test]
fn default_config_has_2_minute_timeout() {
    let c = DaemonConfig::default();
    assert_eq!(c.idle_timeout_mins, 2);
}

#[test]
fn default_saver_is_ascii() {
    let c = DaemonConfig::default();
    assert_eq!(c.active_saver.as_deref(), Some("ascii"));
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

#[test]
fn resolve_config_path_prefers_newer_modified_file_when_both_exist() {
    let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join(format!("idle-test-cfg-mtime-{}", std::process::id()));
    let idlescreen_dir = tmp.join("idlescreen");
    let idle_dir = tmp.join("idle");
    std::fs::create_dir_all(&idlescreen_dir).unwrap();
    std::fs::create_dir_all(&idle_dir).unwrap();
    let idlescreen_file = idlescreen_dir.join("config.yaml");
    let idle_file = idle_dir.join("config.yaml");

    std::fs::write(&idlescreen_file, "idle_timeout_mins: 10\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(&idle_file, "idle_timeout_mins: 20\n").unwrap();

    let prior_xdg = std::env::var("XDG_CONFIG_HOME").ok();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
    }

    let resolved = DaemonConfig::resolve_config_path();
    assert_eq!(resolved.as_ref(), Some(&idle_file));

    match prior_xdg {
        Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn default_inhibit_on_media_is_true() {
    let c = DaemonConfig::default();
    assert!(c.inhibit_on_media);
}

#[test]
fn inhibit_on_media_round_trip_persistence() {
    let mut config = DaemonConfig::default();
    assert!(config.inhibit_on_media);

    let mut section = String::new();
    apply_config_line(&mut config, &mut section, "inhibit_on_media: false");
    assert!(!config.inhibit_on_media);

    let rendered = config.rendered_fields();
    assert!(
        rendered
            .iter()
            .any(|(k, v)| *k == "inhibit_on_media" && v == "false")
    );

    let merged = parse::merge_config_body("", &mut config.rendered_fields(), &config.saver_params);
    assert!(merged.contains("inhibit_on_media: false"));

    let mut restored = DaemonConfig::default();
    let mut current_sec = String::new();
    for line in merged.lines() {
        apply_config_line(&mut restored, &mut current_sec, line);
    }
    assert_eq!(restored.inhibit_on_media, config.inhibit_on_media);
}

// ---- logo_file -----------------------------------------------------------

#[test]
fn logo_file_accepts_an_absolute_path() {
    let mut config = DaemonConfig::default();
    let mut section = String::new();
    apply_config_line(
        &mut config,
        &mut section,
        "logo_file: /home/u/.config/idlescreen/logo.txt",
    );
    assert_eq!(
        config.logo_file.as_deref(),
        Some("/home/u/.config/idlescreen/logo.txt")
    );
}

#[test]
fn logo_file_rejects_a_relative_path() {
    // A relative path would resolve against the daemon's cwd, which is not
    // something a user can reason about from a config file.
    let mut config = DaemonConfig::default();
    let mut section = String::new();
    apply_config_line(&mut config, &mut section, "logo_file: ../../etc/passwd");
    assert_eq!(config.logo_file, None);
}

#[test]
fn logo_file_empty_clears_the_setting() {
    let mut config = DaemonConfig::default();
    let mut section = String::new();
    apply_config_line(&mut config, &mut section, "logo_file: /tmp/a.txt");
    assert!(config.logo_file.is_some());
    apply_config_line(&mut config, &mut section, "logo_file:");
    assert_eq!(config.logo_file, None);
}
