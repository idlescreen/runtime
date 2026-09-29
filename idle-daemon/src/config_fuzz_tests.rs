// SPDX-License-Identifier: MIT

//! Fuzz tests for the config parser: every byte pattern that can appear in a
//! user-editable config.yaml must parse without panic and leave the config in
//! a valid state (bounded timeout, finite in-range scale, charset-clean saver).

use crate::config::DaemonConfig;
use crate::config_parse::apply_config_line;
use idle_api::LcgRng;

/// Post-parse invariants the parser must preserve regardless of input.
fn assert_config_valid(c: &DaemonConfig) {
    assert!((1..=240).contains(&c.idle_timeout_mins));
    if let Some(s) = c.render_scale {
        assert!(
            s.is_finite() && (0.25..=1.0).contains(&s),
            "scale {s} out of range"
        );
    }
    if let Some(name) = &c.active_saver {
        assert!(!name.is_empty());
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'),
            "active_saver not charset-clean: {name:?}"
        );
    }
}

#[test]
fn hostile_key_value_lines() {
    // Targeted hostile lines: extreme numerics, injection, section tricks.
    let lines = [
        "idle_timeout_mins: 0",
        "idle_timeout_mins: 240",
        "idle_timeout_mins: 241",
        "idle_timeout_mins: 4294967295",
        "idle_timeout_mins: -5",
        "idle_timeout_mins: abc",
        "idle_timeout_mins: 5extra",
        "idle_timeout_mins:  5  ",
        "idle_timeout_mins: 005",
        "idle_timeout_mins:",
        "render_scale: NaN",
        "render_scale: nan",
        "render_scale: inf",
        "render_scale: -inf",
        "render_scale: 1e999",
        "render_scale: -0.5",
        "render_scale: 0.25",
        "render_scale: 1.0",
        "render_scale: 0.0",
        "active_saver: ../../etc/passwd",
        "active_saver: evil;sudo reboot",
        "active_saver: beams\u{0}x",
        "active_saver: ",
        "active_saver: RANDOM",
        "active_saver: Shuffle",
        "idle_enabled: TRUE",
        "idle_enabled: 1",
        "idle_enabled: notabool",
        "key_without_colon",
        ":",
        "::",
        "[",
        "]",
        "[]",
        "[saver",
        "saver]",
        "[saver]",
        "[saver.params]",
        "theme: notatheme",
    ];
    for line in lines {
        let mut c = DaemonConfig::default();
        let mut section = String::new();
        apply_config_line(&mut c, &mut section, line);
        assert_config_valid(&c);
    }
}

#[test]
fn seeded_line_soup() {
    let mut rng = LcgRng::new(0xBEEF);
    for _ in 0..20_000 {
        let mut c = DaemonConfig::default();
        let mut section = String::new();
        // Hostile alphabet incl. unicode + control + structural chars.
        const ALPHABET: &[u8] = b"abz019-./\\\0\x7f~:[]\"' \t".as_slice();
        let len = rng.next_usize(80);
        let mut line = String::with_capacity(len);
        for _ in 0..len {
            line.push(ALPHABET[rng.next_usize(ALPHABET.len())] as char);
        }
        apply_config_line(&mut c, &mut section, &line);
        assert_config_valid(&c);
    }
}

#[test]
fn oversized_and_edge_lines() {
    let huge_val = "x".repeat(1_000_000);
    let huge_key = "k".repeat(100_000);
    for line in [
        format!("render_scale: {huge_val}"),
        format!("{huge_key}: v"),
        format!("active_saver: {huge_val}"),
        format!("idle_timeout_mins: {}", "9".repeat(500)),
        "a".repeat(1_000_000),
    ] {
        let mut c = DaemonConfig::default();
        let mut section = String::new();
        apply_config_line(&mut c, &mut section, &line);
        assert_config_valid(&c);
    }
}
