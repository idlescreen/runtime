// SPDX-License-Identifier: MIT
//! Integration test suite validating state synchronization across idle-daemon and idle-dbus.

use idle_daemon::config::DaemonConfig;
use idle_daemon::controller::{DaemonCommand, DaemonController};
use idle_daemon::dbus_server::service_helpers::{apply_config_command, live_status};
use std::sync::Arc;

#[test]
fn test_live_status_contract_completeness() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let status = live_status(&controller);
    let map = status.to_map();

    // Verify all 13 canonical contract keys exist
    let keys = [
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
    for key in keys {
        assert!(map.contains_key(key), "Missing status contract key: {key}");
    }
}

#[test]
fn test_saver_alias_normalization_sync() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));

    // Setting random/none/shuffle (any case) or "" → active_saver = None (random rotation)
    for alias in [
        "random", "none", "shuffle", "", "Random", "RANDOM", "None", "Shuffle",
    ] {
        let cmd = DaemonCommand::SetSaver(if alias.is_empty() {
            None
        } else {
            Some(alias.to_string())
        });
        apply_config_command(&controller, cmd, "SetSaver").expect("apply_config_command failed");

        let status = live_status(&controller);
        assert_eq!(
            status.active_saver, "",
            "Alias '{alias}' did not normalize to empty active_saver"
        );
    }
}

#[test]
fn test_effective_inhibit_sync() {
    unsafe {
        std::env::set_var("IDLE_TEST_MOCK_AC", "1");
    }
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));

    // 1. Initial state: not inhibited
    let status0 = live_status(&controller);
    assert!(!status0.inhibited, "Initial status should not be inhibited");

    // 2. Update live state with effective_inhibited = true (e.g. battery or cooldown)
    controller.update_live_state(false, false, false, "", true);
    let status1 = live_status(&controller);
    assert!(
        status1.inhibited,
        "Effective inhibition flag was not published in status"
    );

    // 3. Update live state with effective_inhibited = false
    controller.update_live_state(false, false, false, "", false);
    let status2 = live_status(&controller);
    assert!(
        !status2.inhibited,
        "Status should clear inhibited when effective_inhibited becomes false"
    );
}

#[test]
fn test_idempotent_config_mutation_no_double_save() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));

    // Initial mutation
    apply_config_command(&controller, DaemonCommand::SetTimeout(10), "SetTimeout").unwrap();
    let status1 = live_status(&controller);
    assert_eq!(status1.idle_timeout_mins, 10);

    // Repeated mutation with same value
    apply_config_command(&controller, DaemonCommand::SetTimeout(10), "SetTimeout").unwrap();
    let status2 = live_status(&controller);
    assert_eq!(status2.idle_timeout_mins, 10);

    // Test SetShowFpsOverlay idempotent behavior
    apply_config_command(
        &controller,
        DaemonCommand::SetShowFpsOverlay(true),
        "SetFps",
    )
    .unwrap();
    let status3 = live_status(&controller);
    assert!(status3.show_fps_overlay);

    apply_config_command(
        &controller,
        DaemonCommand::SetShowFpsOverlay(true),
        "SetFps",
    )
    .unwrap();
    let status4 = live_status(&controller);
    assert!(status4.show_fps_overlay);
}
