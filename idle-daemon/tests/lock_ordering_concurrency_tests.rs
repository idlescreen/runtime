// SPDX-License-Identifier: MIT
//! Empirical stress tests for multi-threaded lock ordering across config, inhibitors,
//! status, and teardown_requested.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use idle_daemon::config::DaemonConfig;
use idle_daemon::controller::{DaemonCommand, DaemonController};
use idle_daemon::dbus_server::service_helpers::{live_status, sync_config_status};
use zbus::names::UniqueName;

#[test]
fn stress_test_concurrent_lock_ordering_and_race_conditions() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let stop_signal = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();

    // Spawn 12 concurrent worker threads executing high-frequency operations
    // across config, status, inhibitors, and teardown_requested.

    // Worker 1 & 2: Mutate config and apply commands
    for _ in 0..2 {
        let c = controller.clone();
        let stop = stop_signal.clone();
        handles.push(thread::spawn(move || {
            let mut iter = 0u64;
            while !stop.load(Ordering::Relaxed) {
                iter += 1;
                let timeout = (iter % 10 + 1) as u32;
                let _ = c.apply_command(DaemonCommand::SetTimeout(timeout));
                let saver = if iter.is_multiple_of(2) {
                    Some("beams".to_string())
                } else {
                    None
                };
                let _ = c.apply_command(DaemonCommand::SetSaver(saver));
                let _ = c.apply_command(if iter.is_multiple_of(2) {
                    DaemonCommand::Enable
                } else {
                    DaemonCommand::Disable
                });
            }
            iter
        }));
    }

    // Worker 3 & 4: High-frequency live status updates
    for _ in 0..2 {
        let c = controller.clone();
        let stop = stop_signal.clone();
        handles.push(thread::spawn(move || {
            let mut iter = 0u64;
            while !stop.load(Ordering::Relaxed) {
                iter += 1;
                let sys_idle = iter.is_multiple_of(2);
                let pres_active = iter.is_multiple_of(3);
                let prev_active = iter.is_multiple_of(5);
                let inhibited = iter.is_multiple_of(7);
                let saver = if pres_active { "beams" } else { "" };
                c.update_live_state(sys_idle, pres_active, prev_active, saver, inhibited);
            }
            iter
        }));
    }

    // Worker 5 & 6: D-Bus live_status reads
    for _ in 0..2 {
        let c = controller.clone();
        let stop = stop_signal.clone();
        handles.push(thread::spawn(move || {
            let mut iter = 0u64;
            while !stop.load(Ordering::Relaxed) {
                iter += 1;
                let status = live_status(&c);
                assert!(status.running);
            }
            iter
        }));
    }

    // Worker 7 & 8: Inhibitors add, remove, and query
    for i in 0..2 {
        let c = controller.clone();
        let stop = stop_signal.clone();
        handles.push(thread::spawn(move || {
            let mut iter = 0u64;
            let client_str = format!(":1.{}", 100 + i);
            let client = UniqueName::try_from(client_str).unwrap();
            while !stop.load(Ordering::Relaxed) {
                iter += 1;
                if let Ok(cookie) =
                    c.inhibitors
                        .add("test_app".into(), "playing media".into(), client.clone())
                {
                    let _ = c.inhibitors.is_inhibited();
                    let _ = c.inhibitors.list_all();
                    c.inhibitors.remove_for_client(cookie, &client);
                }
            }
            iter
        }));
    }

    // Worker 9 & 10: Teardown and command queue drain
    for _ in 0..2 {
        let c = controller.clone();
        let stop = stop_signal.clone();
        handles.push(thread::spawn(move || {
            let mut iter = 0u64;
            while !stop.load(Ordering::Relaxed) {
                iter += 1;
                if iter.is_multiple_of(2) {
                    let _ = c.send_command(DaemonCommand::StopPresentation);
                } else {
                    let _ = c.send_command(DaemonCommand::Preview("bursts".into()));
                }
                let cmds = c.drain_commands();
                let _ = cmds.len();
            }
            iter
        }));
    }

    // Worker 11 & 12: Config sync and status publish
    for _ in 0..2 {
        let c = controller.clone();
        let stop = stop_signal.clone();
        handles.push(thread::spawn(move || {
            let mut iter = 0u64;
            while !stop.load(Ordering::Relaxed) {
                iter += 1;
                sync_config_status(&c);
                c.publish_status_if_dirty();
            }
            iter
        }));
    }

    // Run stress test for 3 seconds of sustained thread hammering
    thread::sleep(Duration::from_secs(3));
    stop_signal.store(true, Ordering::Relaxed);

    let mut total_ops = 0u64;
    for handle in handles {
        let ops = handle
            .join()
            .expect("Worker thread panicked or deadlocked!");
        total_ops += ops;
    }

    assert!(
        total_ops > 10_000,
        "Expected >10,000 operations, got {total_ops}"
    );
}
