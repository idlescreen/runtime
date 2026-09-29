// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Key/value application for daemon `config.yaml` lines.

use idle_runner::launcher::{is_allowed_saver, sanitize_saver_name};

use crate::config::DaemonConfig;

/// Apply a single `key: value` pair from config.yaml into `config`.
pub(crate) fn apply_config_key(config: &mut DaemonConfig, key: &str, val: &str) {
    match key {
        "idle_timeout_mins" => apply_idle_timeout(config, val),
        "active_saver" => apply_active_saver(config, val),
        "idle_enabled" => {
            if let Ok(b) = val.parse::<bool>() {
                config.idle_enabled = b;
            }
        }
        "show_fps_overlay" => {
            if let Ok(b) = val.parse::<bool>() {
                config.show_fps_overlay = b;
            }
        }
        "render_scale" => apply_render_scale(config, val),
        "theme" => {
            if let Ok(theme) = val.parse::<idle_api::Theme>() {
                config.theme = theme;
            }
        }
        "strict_control" => {
            if let Ok(b) = val.parse::<bool>() {
                config.strict_control = b;
            }
        }
        _ => {}
    }
}

fn apply_idle_timeout(config: &mut DaemonConfig, val: &str) {
    if let Some(n) = val.parse::<u32>().ok().filter(|&n| (1..=240).contains(&n)) {
        config.idle_timeout_mins = n;
    }
}

fn apply_active_saver(config: &mut DaemonConfig, val: &str) {
    // Empty / none / random / shuffle (any case) → random rotation (None on wire).
    if val.is_empty()
        || val.eq_ignore_ascii_case("none")
        || val.eq_ignore_ascii_case("random")
        || val.eq_ignore_ascii_case("shuffle")
    {
        config.active_saver = None;
    } else if is_allowed_saver(val) {
        config.active_saver = sanitize_saver_name(val).map(|s| s.to_string());
    }
}

fn apply_render_scale(config: &mut DaemonConfig, val: &str) {
    if val.is_empty() || val.eq_ignore_ascii_case("null") {
        config.render_scale = None;
    } else if let Some(scale) = val.parse::<f32>().ok().filter(|s| s.is_finite()) {
        config.render_scale = Some(scale.clamp(0.25, 1.0));
    }
}

/// Parse one non-comment config line (`key: value` or `[section]`) into the config.
pub(crate) fn apply_config_line(
    config: &mut DaemonConfig,
    current_section: &mut String,
    line: &str,
) {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return;
    }

    if let Some(stripped) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        *current_section = stripped.to_string();
        return;
    }

    // `key: value` is canonical, but accept `key = value` too — hand-edited
    // files commonly use `=`, and silently ignoring (or worse, dropping on
    // merge) a line the user meant as a setting is a config-loss bug.
    let Some(idx) = line.find([':', '=']) else {
        return;
    };
    let key = line[..idx].trim();
    let val = line[idx + 1..].trim().trim_matches('"').trim_matches('\'');

    if current_section == "saver" {
        config.saver_params.insert(key.to_string(), val.to_string());
    } else if let Some(stripped) = current_section.strip_prefix("saver.") {
        let full_key = format!("{}.{}", stripped, key);
        config.saver_params.insert(full_key, val.to_string());
    } else {
        apply_config_key(config, key, val);
    }
}

/// Merge `fields` (daemon-owned `key: value` pairs) into existing config
/// text, preserving every line it does not own — comments, applet keys like
/// `accent_color`/`theme_idx`, unknown keys. `[saver]`/`[saver.*]` sections
/// are rewritten from `saver_params` so removed params disappear; comment,
/// blank, and unparseable lines inside them are kept verbatim.
/// `fields` is drained: consumed entries are replaced in place, leftovers
/// append at the end.
pub(crate) fn merge_config_body(
    existing: &str,
    fields: &mut Vec<(&'static str, String)>,
    saver_params: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut body = String::new();
    // Non-kv lines collected from `saver*` sections — re-emitted verbatim.
    let mut saver_extras: Vec<&str> = Vec::new();
    if existing.trim().is_empty() {
        body.push_str(
            "# IdleScreen themes and settings\n\
             accent_color: \"#00BFFF\"\n\
             # dark_mode is auto-detected from system\n\
             theme_idx: 0\n\
             # strict_control: deny D-Bus control when peer exe unreadable (no comm fallback)\n",
        );
    } else {
        // `saver*` sections are rewritten from saver_params; keys inside any
        // other section are preserved verbatim even if names collide. Inside
        // saver sections, non-key lines (comments, blanks, unparseable text)
        // are kept and re-emitted — silently dropping them loses user data.
        let mut in_saver = false;
        let mut in_section = false;
        for line in existing.lines() {
            let t = line.trim();
            if let Some(sec) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                in_saver = sec == "saver" || sec.starts_with("saver.");
                in_section = true;
                if !in_saver {
                    body.push_str(line);
                    body.push('\n');
                }
                continue;
            }
            if in_saver {
                if t.find([':', '=']).is_none() {
                    saver_extras.push(line);
                }
                continue;
            }
            let owned = !in_section
                && !t.is_empty()
                && !t.starts_with('#')
                && t.find([':', '=']).is_some_and(|idx| {
                    let key = t[..idx].trim();
                    if let Some(pos) = fields.iter().position(|(k, _)| *k == key) {
                        let (k, v) = fields.remove(pos);
                        body.push_str(&format!("{k}: {v}\n"));
                        true
                    } else {
                        false
                    }
                });
            if !owned {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    for (k, v) in fields.drain(..) {
        body.push_str(&format!("{k}: {v}\n"));
    }
    if !saver_params.is_empty() || !saver_extras.is_empty() {
        body.push_str("\n[saver]\n");
        for line in &saver_extras {
            body.push_str(line);
            body.push('\n');
        }
        for (k, v) in saver_params {
            body.push_str(&format!("{k}: {v}\n"));
        }
    }
    body
}
