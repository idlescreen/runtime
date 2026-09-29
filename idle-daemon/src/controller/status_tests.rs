// SPDX-License-Identifier: MIT

use super::DaemonController;
use super::status::render_scale_matches;
use crate::config::DaemonConfig;
use idle_dbus::DaemonStatus;

#[test]
fn render_scale_matches_accepts_same_value() {
    assert!(render_scale_matches("0.5", 0.5));
    assert!(render_scale_matches("1", 1.0));
    assert!(!render_scale_matches("0.5", 1.0));
    assert!(!render_scale_matches("", 0.5));
}

#[test]
fn live_fields_can_show_preview_while_inhibited() {
    let mut status = DaemonStatus::default();
    let config = DaemonConfig::default();
    let changed = DaemonController::apply_live_fields(
        &mut status,
        &config,
        false,
        true,
        true,
        "beams",
        false,
        true,
    );
    assert!(changed);
    assert!(status.preview_active);
    assert!(status.presentation_active);
    assert!(status.inhibited);
    assert_eq!(status.current_saver, "beams");
    assert!(status.running);
}

#[test]
fn live_fields_idempotent_when_unchanged() {
    let mut status = DaemonStatus::default();
    let config = DaemonConfig::default();
    let _ = DaemonController::apply_live_fields(
        &mut status,
        &config,
        true,
        false,
        false,
        "",
        false,
        false,
    );
    let changed = DaemonController::apply_live_fields(
        &mut status,
        &config,
        true,
        false,
        false,
        "",
        false,
        false,
    );
    assert!(!changed);
}

#[test]
fn live_fields_reflect_session_locked() {
    let mut status = DaemonStatus::default();
    let config = DaemonConfig::default();
    DaemonController::apply_live_fields(&mut status, &config, false, false, false, "", true, false);
    assert!(status.session_locked);
    assert!(!status.preview_active);
}

#[test]
fn live_fields_sync_idle_enabled_from_config() {
    let mut status = DaemonStatus::default();
    let config = DaemonConfig {
        idle_enabled: false,
        ..DaemonConfig::default()
    };
    DaemonController::apply_live_fields(
        &mut status,
        &config,
        false,
        false,
        false,
        "",
        false,
        false,
    );
    assert!(!status.idle_enabled);
}
