// SPDX-License-Identifier: MIT

//! Merge/save tests for config.yaml: read-modify-write must preserve foreign
//! keys, comments, `[saver]` extras, and accept `=` separators — never clobber.

use crate::config::DaemonConfig;
use crate::config_parse::{apply_config_line, merge_config_body};
use std::collections::BTreeMap;

fn fields() -> Vec<(&'static str, String)> {
    vec![
        ("idle_timeout_mins", "10".into()),
        ("active_saver", "\"beams\"".into()),
        ("idle_enabled", "true".into()),
        ("show_fps_overlay", "false".into()),
        ("render_scale", "null".into()),
        ("theme", "\"synthwave\"".into()),
        ("strict_control", "false".into()),
    ]
}

#[test]
fn merge_preserves_foreign_keys_and_comments() {
    let existing = "# my header\naccent_color: \"#FF0000\"\ntheme_idx: 3\nidle_timeout_mins: 5\nmy_note: keep me\n";
    let out = merge_config_body(existing, &mut fields(), &BTreeMap::new());
    assert!(out.contains("accent_color: \"#FF0000\""));
    assert!(out.contains("theme_idx: 3"));
    assert!(out.contains("# my header"));
    assert!(out.contains("my_note: keep me"));
    assert!(out.contains("idle_timeout_mins: 10"));
    assert!(!out.contains("idle_timeout_mins: 5"));
    // Every daemon-owned field lands even when absent from the old file.
    for k in ["active_saver", "theme", "strict_control", "render_scale"] {
        assert!(out.contains(&format!("{k}:")), "missing {k}");
    }
}

#[test]
fn merge_rewrites_saver_sections() {
    let existing = "idle_enabled: false\n[saver]\nbeams.speed: 9\n[saver.storm]\ndensity: 4\n";
    let mut params = BTreeMap::new();
    params.insert("beams.speed".to_string(), "2".to_string());
    let out = merge_config_body(existing, &mut fields(), &params);
    assert!(out.contains("[saver]\nbeams.speed: 2"));
    assert!(!out.contains("density"), "removed params must not linger");
    assert!(out.contains("idle_enabled: true"));
}

#[test]
fn merge_preserves_comments_inside_saver_section() {
    // Comments / blank lines / unparseable text inside [saver] must survive —
    // dropping them is silent user-data loss.
    let existing = "[saver]\n# hearth tuning — do not edit\n\nhearth_density: 0.8\n";
    let mut params = BTreeMap::new();
    params.insert("hearth_density".to_string(), "0.8".to_string());
    let out = merge_config_body(existing, &mut fields(), &params);
    assert!(out.contains("# hearth tuning — do not edit"));
    assert!(out.contains("hearth_density: 0.8"));
}

#[test]
fn merge_keeps_saver_section_with_only_comments() {
    // A [saver] section holding only comments still round-trips.
    let existing = "idle_enabled: true\n[saver]\n# empty for now\n";
    let out = merge_config_body(existing, &mut fields(), &BTreeMap::new());
    assert!(out.contains("[saver]"));
    assert!(out.contains("# empty for now"));
}

#[test]
fn equals_separator_parses_and_normalizes() {
    // Hand-edited `key = value` must apply like `key: value` — silently
    // ignoring it is a config-loss trap.
    let mut c = DaemonConfig::default();
    let mut section = String::new();
    apply_config_line(&mut c, &mut section, "idle_timeout_mins = 15");
    assert_eq!(c.idle_timeout_mins, 15);
    section.clear();
    apply_config_line(&mut c, &mut section, "[saver]");
    apply_config_line(&mut c, &mut section, "hearth_density = 0.7");
    assert_eq!(
        c.saver_params.get("hearth_density").map(String::as_str),
        Some("0.7")
    );
}

#[test]
fn merge_rewrites_owned_equals_line_in_place() {
    // An owned key written with `=` is rewritten as `key: value` — not left
    // stale alongside an appended duplicate.
    let existing = "idle_timeout_mins = 5\n";
    let out = merge_config_body(existing, &mut fields(), &BTreeMap::new());
    assert!(out.contains("idle_timeout_mins: 10"));
    assert_eq!(out.matches("idle_timeout_mins").count(), 1);
}

#[test]
fn merge_leaves_keys_inside_other_sections_alone() {
    let existing = "idle_enabled: false\n[plugin]\ntheme: \"other\"\n";
    let out = merge_config_body(existing, &mut fields(), &BTreeMap::new());
    assert!(out.contains("[plugin]\ntheme: \"other\""));
    // top-level theme still emitted (it was absent at top level)
    assert!(out.contains("theme: \"synthwave\""));
}

#[test]
fn merge_empty_file_emits_template() {
    let out = merge_config_body("", &mut fields(), &BTreeMap::new());
    assert!(out.contains("accent_color"));
    assert!(out.contains("idle_timeout_mins: 10"));
    assert!(out.contains("theme: \"synthwave\""));
}

#[test]
fn save_roundtrip_preserves_foreign_keys() {
    let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!("idle-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("config.yaml");
    std::fs::write(
        &file,
        "# user header\naccent_color: \"#AABBCC\"\ntheme_idx: 7\nidle_timeout_mins: 5\n\
         active_saver: \"beams\"\nstrict_control: true\ncustom_key: 42\n[saver]\nbeams.speed: 1\n",
    )
    .unwrap();
    unsafe { std::env::set_var("IDLE_CONFIG_DIR", &dir) };
    let mut cfg = DaemonConfig::load();
    cfg.idle_timeout_mins = 20;
    cfg.saver_params.clear();
    cfg.save().unwrap();
    let out = std::fs::read_to_string(&file).unwrap();
    unsafe { std::env::remove_var("IDLE_CONFIG_DIR") };
    assert!(out.contains("accent_color: \"#AABBCC\""));
    assert!(out.contains("theme_idx: 7"));
    assert!(out.contains("custom_key: 42"));
    assert!(out.contains("# user header"));
    assert!(out.contains("idle_timeout_mins: 20"));
    assert!(out.contains("strict_control: true"));
    assert!(!out.contains("beams.speed"), "cleared params must drop");
}

#[test]
fn save_writes_backup_of_prior_content() {
    let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!("idle-cfg-bak-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("config.yaml");
    let prior = "# user header\naccent_color: \"#112233\"\nidle_timeout_mins: 7\n";
    std::fs::write(&file, prior).unwrap();
    unsafe { std::env::set_var("IDLE_CONFIG_DIR", &dir) };
    let mut cfg = DaemonConfig::load();
    cfg.idle_timeout_mins = 30;
    cfg.save().unwrap();
    let bak = std::fs::read_to_string(dir.join("config.yaml.bak")).unwrap();
    unsafe { std::env::remove_var("IDLE_CONFIG_DIR") };
    assert_eq!(bak, prior, "backup must hold the pre-save file verbatim");
}
