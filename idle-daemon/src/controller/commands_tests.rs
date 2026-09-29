// SPDX-License-Identifier: MIT

use super::*;
use crate::config::DaemonConfig;

fn test_controller() -> (
    DaemonController,
    std::path::PathBuf,
    std::sync::MutexGuard<'static, ()>,
) {
    let guard = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
    let temp = std::env::temp_dir().join(format!(
        "idle-daemon-cmd-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&temp).expect("create temp config dir for command tests");
    // SAFETY: tests hold crate::TEST_ENV_LOCK; only this suite mutates XDG_CONFIG_HOME.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &temp);
    }
    let controller = DaemonController::new(DaemonConfig::default());
    (controller, temp, guard)
}

#[test]
fn enable_sets_idle_true() {
    let (c, _tmp, _guard) = test_controller();
    c.apply_command(DaemonCommand::Enable)
        .expect("Enable should succeed");
    assert!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .idle_enabled
    );
}

#[test]
fn disable_sets_idle_false() {
    let (c, _tmp, _guard) = test_controller();
    c.apply_command(DaemonCommand::Disable)
        .expect("Disable should succeed");
    assert!(
        !c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .idle_enabled
    );
}

#[test]
fn set_timeout_validates_range() {
    let (c, _tmp, _guard) = test_controller();
    assert!(c.apply_command(DaemonCommand::SetTimeout(0)).is_err());
    assert!(c.apply_command(DaemonCommand::SetTimeout(241)).is_err());
    assert!(c.apply_command(DaemonCommand::SetTimeout(10)).is_ok());
    assert_eq!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .idle_timeout_mins,
        10
    );
}

#[test]
fn set_timeout_accepts_boundaries() {
    let (c, _tmp, _guard) = test_controller();
    assert!(c.apply_command(DaemonCommand::SetTimeout(1)).is_ok());
    assert!(c.apply_command(DaemonCommand::SetTimeout(240)).is_ok());
    assert_eq!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .idle_timeout_mins,
        240
    );
}

#[test]
fn set_render_scale_zero_normalizes_to_none() {
    let (c, _tmp, _guard) = test_controller();
    c.apply_command(DaemonCommand::SetRenderScale(Some(0.0)))
        .expect("zero scale should normalize");
    assert!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .render_scale
            .is_none()
    );
}

#[test]
fn set_render_scale_rejects_out_of_range() {
    let (c, _tmp, _guard) = test_controller();
    assert!(
        c.apply_command(DaemonCommand::SetRenderScale(Some(2.0)))
            .is_err()
    );
    assert!(
        c.apply_command(DaemonCommand::SetRenderScale(Some(0.1)))
            .is_err()
    );
}

#[test]
fn set_render_scale_accepts_in_range() {
    let (c, _tmp, _guard) = test_controller();
    c.apply_command(DaemonCommand::SetRenderScale(Some(0.5)))
        .expect("0.5 scale in range");
    assert_eq!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .render_scale,
        Some(0.5)
    );
}

#[test]
fn set_render_scale_accepts_none() {
    let (c, _tmp, _guard) = test_controller();
    c.apply_command(DaemonCommand::SetRenderScale(None))
        .expect("None scale accepted");
    assert!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .render_scale
            .is_none()
    );
}

#[test]
fn set_show_fps_overlay_toggles() {
    let (c, _tmp, _guard) = test_controller();
    c.apply_command(DaemonCommand::SetShowFpsOverlay(true))
        .expect("enable fps overlay");
    assert!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .show_fps_overlay
    );
    c.apply_command(DaemonCommand::SetShowFpsOverlay(false))
        .expect("disable fps overlay");
    assert!(
        !c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .show_fps_overlay
    );
}

#[test]
fn preview_and_stop_are_no_ops() {
    // apply_command does not mutate config for Preview/Stop — tick loop owns that.
    let (c, _tmp, _guard) = test_controller();
    assert!(
        c.apply_command(DaemonCommand::Preview("beams".into()))
            .is_ok()
    );
    assert!(c.apply_command(DaemonCommand::StopPresentation).is_ok());
}

#[test]
fn command_queue_drains_preview_and_stop_in_order() {
    let (c, _tmp, _guard) = test_controller();
    c.command_tx
        .try_send(DaemonCommand::Preview("beams".into()))
        .expect("send preview");
    c.command_tx
        .try_send(DaemonCommand::Preview("ripple".into()))
        .expect("send preview2");
    c.command_tx
        .try_send(DaemonCommand::StopPresentation)
        .expect("send stop");
    let cmds = c.drain_commands();
    assert_eq!(cmds.len(), 3);
    assert!(matches!(&cmds[0], DaemonCommand::Preview(n) if n == "beams"));
    assert!(matches!(&cmds[1], DaemonCommand::Preview(n) if n == "ripple"));
    assert!(matches!(cmds[2], DaemonCommand::StopPresentation));
    assert!(c.drain_commands().is_empty());
}

#[test]
fn command_queue_enable_persists_and_is_drainable() {
    let (c, _tmp, _guard) = test_controller();
    c.command_tx
        .try_send(DaemonCommand::Disable)
        .expect("send disable");
    // Drain does not apply — only returns. apply_command is separate path.
    let cmds = c.drain_commands();
    assert_eq!(cmds.len(), 1);
    c.apply_command(DaemonCommand::Disable).expect("apply");
    assert!(
        !c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .idle_enabled
    );
}

#[test]
fn set_saver_none_is_random_mode() {
    let (c, _tmp, _guard) = test_controller();
    c.apply_command(DaemonCommand::SetSaver(None))
        .expect("random");
    assert!(
        c.config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .active_saver
            .is_none()
    );
}

#[test]
fn mark_dirty_sets_status_dirty_flag() {
    let (c, _tmp, _guard) = test_controller();
    let _ = c.take_dirty();
    c.mark_dirty();
    assert!(c.take_dirty());
}

#[test]
fn test_command_queue_backpressure() {
    let (c, _tmp, _guard) = test_controller();
    for i in 0..16 {
        assert!(c.command_tx.try_send(DaemonCommand::SetTimeout(i)).is_ok());
    }
    // 17th should fail because it's bounded to 16
    assert!(
        c.command_tx
            .try_send(DaemonCommand::SetTimeout(16))
            .is_err()
    );
}
