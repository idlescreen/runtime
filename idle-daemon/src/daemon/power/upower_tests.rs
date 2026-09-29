// SPDX-License-Identifier: MIT
// Copyright 2026 IdleScreen

//! Unit tests for UPower watcher and fallback paths.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::upower::{PowerWatcherBackend, spawn_power_watcher};
use super::watcher::{PowerWatcher, WaitOutcome};

#[test]
fn test_power_watcher_initial_and_notify() {
    let watcher = PowerWatcher::from_initial_state(false);
    assert!(!watcher.cached_is_on_battery());
    assert_eq!(watcher.notify_generation(), 0);

    watcher.notify_update(true);
    assert!(watcher.cached_is_on_battery());
    assert_eq!(watcher.notify_generation(), 1);

    let stop = Arc::new(AtomicBool::new(false));
    let outcome = watcher.wait_for_heartbeat(&stop, Duration::from_millis(10));
    assert!(matches!(
        outcome,
        WaitOutcome::Heartbeat | WaitOutcome::Notified
    ));
}

#[test]
fn test_spawn_power_watcher_fallback_or_upower() {
    // Should gracefully return either UPower or inotify backend without panic
    let backend = spawn_power_watcher();
    assert!(backend.is_ok());
    let watcher = backend.as_ref().map(|b| b.handle()).ok();
    assert!(watcher.is_some());
}

#[test]
fn test_backend_variants() {
    let watcher = PowerWatcher::from_initial_state(true);
    assert!(watcher.cached_is_on_battery());
    let backend = spawn_power_watcher().map(|b| {
        matches!(
            b,
            PowerWatcherBackend::UPower(_) | PowerWatcherBackend::Inotify(_)
        )
    });
    assert_eq!(backend.ok(), Some(true));
}
