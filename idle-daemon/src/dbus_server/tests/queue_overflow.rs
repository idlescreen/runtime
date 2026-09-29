// SPDX-License-Identifier: MIT

use crate::config::DaemonConfig;
use crate::controller::{DaemonCommand, DaemonController};
use crate::dbus_server::screensaver::ScreenSaverService;
use std::sync::Arc;

fn fill_command_queue(controller: &DaemonController) {
    for i in 0..16 {
        let res = controller.send_command(DaemonCommand::Preview(format!("saver_{i}")));
        assert!(res.is_ok(), "filling queue failed at index {i}");
    }
    let overflow = controller.send_command(DaemonCommand::Preview("overflow".into()));
    assert!(
        overflow.is_err(),
        "17th non-teardown command should fail when queue is full"
    );
}

#[tokio::test]
async fn test_send_command_teardown_never_dropped_when_queue_full() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    fill_command_queue(&controller);

    let res = controller.send_command(DaemonCommand::StopPresentation);
    assert!(
        res.is_ok(),
        "StopPresentation must succeed even when queue is full"
    );

    let commands = controller.drain_commands();
    assert_eq!(
        commands.len(),
        17,
        "expected 1 teardown + 16 queued preview commands"
    );
    assert_eq!(
        commands[0],
        DaemonCommand::StopPresentation,
        "StopPresentation must be prepended at index 0"
    );
}

#[tokio::test]
async fn test_screensaver_simulate_user_activity_queue_overflow() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let service = ScreenSaverService {
        controller: controller.clone(),
    };
    fill_command_queue(&controller);

    service.simulate_user_activity().await;

    let commands = controller.drain_commands();
    assert!(
        commands.contains(&DaemonCommand::StopPresentation),
        "simulate_user_activity must guarantee StopPresentation delivery"
    );
    assert_eq!(commands[0], DaemonCommand::StopPresentation);
}

#[tokio::test]
async fn test_screensaver_inhibit_queue_overflow() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let service = ScreenSaverService {
        controller: controller.clone(),
    };
    fill_command_queue(&controller);

    let msg = zbus::message::Message::method_call("/org/freedesktop/ScreenSaver", "Inhibit")
        .unwrap()
        .sender(":1.42")
        .unwrap()
        .build(&("test_app", "testing teardown"))
        .unwrap();
    let header = msg.header();

    let res = service
        .inhibit("test_app", "testing teardown", header)
        .await;
    assert!(res.is_ok(), "inhibit must succeed");

    let commands = controller.drain_commands();
    assert_eq!(commands[0], DaemonCommand::StopPresentation);
}

#[tokio::test]
async fn test_screensaver_lock_queue_overflow() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let service = ScreenSaverService {
        controller: controller.clone(),
    };
    fill_command_queue(&controller);

    service.lock().await;

    let commands = controller.drain_commands();
    assert_eq!(commands[0], DaemonCommand::StopPresentation);
}

#[tokio::test]
async fn test_screensaver_set_active_false_queue_overflow() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let service = ScreenSaverService {
        controller: controller.clone(),
    };
    fill_command_queue(&controller);

    let msg = zbus::message::Message::method_call("/org/freedesktop/ScreenSaver", "SetActive")
        .unwrap()
        .build(&(false,))
        .unwrap();
    let header = msg.header();

    let res = service.set_active(false, header).await;
    assert!(res.is_ok(), "set_active(false) must succeed");

    let commands = controller.drain_commands();
    assert_eq!(commands[0], DaemonCommand::StopPresentation);
}

#[tokio::test]
async fn test_trance_service_stop_preview_queue_overflow() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    fill_command_queue(&controller);

    let res = controller.send_command(DaemonCommand::StopPresentation);
    assert!(res.is_ok(), "stop_preview send_command must succeed");

    let commands = controller.drain_commands();
    assert_eq!(commands[0], DaemonCommand::StopPresentation);
}

#[tokio::test]
async fn test_multiple_teardown_requests_deduplicated() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    fill_command_queue(&controller);

    for _ in 0..5 {
        let _ = controller.send_command(DaemonCommand::StopPresentation);
    }

    let commands = controller.drain_commands();
    let teardown_count = commands
        .iter()
        .filter(|c| **c == DaemonCommand::StopPresentation)
        .count();
    assert_eq!(
        teardown_count, 1,
        "multiple teardown requests should be deduplicated to one"
    );
    assert_eq!(commands[0], DaemonCommand::StopPresentation);
}

#[tokio::test]
async fn test_concurrent_burst_100_threads_stop_presentation_never_dropped() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let mut handles = Vec::new();

    // Spawn 100 concurrent tasks flooding the queue
    for i in 0..100 {
        let ctrl = controller.clone();
        handles.push(tokio::spawn(async move {
            for j in 0..20 {
                let _ = ctrl.send_command(DaemonCommand::Preview(format!("saver_{i}_{j}")));
            }
            if i == 42 {
                let res = ctrl.send_command(DaemonCommand::StopPresentation);
                assert!(
                    res.is_ok(),
                    "StopPresentation send must succeed under high contention"
                );
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let commands = controller.drain_commands();
    assert!(
        commands.contains(&DaemonCommand::StopPresentation),
        "StopPresentation must be present in drained commands despite 100 concurrent burst tasks"
    );
    assert_eq!(
        commands[0],
        DaemonCommand::StopPresentation,
        "StopPresentation must be prioritized at index 0"
    );
}

#[tokio::test]
async fn test_concurrent_continuous_hammer_stress() {
    let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
    let stop_received_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut handles = Vec::new();

    // 50 producer tasks pounding queue with preview commands
    for i in 0..50 {
        let ctrl = controller.clone();
        handles.push(tokio::spawn(async move {
            for j in 0..100 {
                let _ = ctrl.send_command(DaemonCommand::Preview(format!("hammer_{i}_{j}")));
                tokio::task::yield_now().await;
            }
        }));
    }

    // 10 teardown tasks issuing StopPresentation
    for _ in 0..10 {
        let ctrl = controller.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..10 {
                let _ = ctrl.send_command(DaemonCommand::StopPresentation);
                tokio::task::yield_now().await;
            }
        }));
    }

    // 1 consumer task draining commands continuously
    let ctrl_consumer = controller.clone();
    let stop_counter = stop_received_count.clone();
    let consumer_handle = tokio::spawn(async move {
        for _ in 0..200 {
            let drained = ctrl_consumer.drain_commands();
            if drained.contains(&DaemonCommand::StopPresentation) {
                stop_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    });

    for h in handles {
        h.await.unwrap();
    }
    consumer_handle.await.unwrap();

    // Final drain to ensure any remaining teardown request is captured
    let final_drained = controller.drain_commands();
    if final_drained.contains(&DaemonCommand::StopPresentation) {
        stop_received_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    assert!(
        stop_received_count.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "At least one StopPresentation must be captured during heavy concurrent hammer"
    );
}
