// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for the render-loop watchdog primitive.

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[test]
fn fresh_watchdog_is_not_stalled() {
    let wd = Watchdog::new();
    assert!(!wd.stalled(60_000), "fresh watchdog must not be stalled");
}

#[test]
fn heartbeat_resets_age() {
    let wd = Watchdog::new();
    std::thread::sleep(Duration::from_millis(20));
    assert!(wd.age_ms() >= 10);
    wd.heartbeat();
    assert!(wd.age_ms() < 10);
}

#[test]
fn stalled_threshold_triggers() {
    let wd = Watchdog::new();
    // Fresh watchdog has near-zero age; advance with sleep then check.
    std::thread::sleep(Duration::from_millis(20));
    assert!(
        wd.stalled(10),
        "after 20 ms with 10 ms threshold, must stall"
    );
}

#[test]
fn configured_timeout_default_is_5s() {
    let _g = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_HEARTBEAT_TIMEOUT_MS") };
    assert_eq!(configured_timeout_ms(), DEFAULT_HEARTBEAT_TIMEOUT_MS);
}

#[test]
fn configured_timeout_env_override() {
    let _g = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_HEARTBEAT_TIMEOUT_MS", "1234") };
    let v = configured_timeout_ms();
    unsafe { std::env::remove_var("IDLE_HEARTBEAT_TIMEOUT_MS") };
    assert_eq!(v, 1234);
}

#[test]
fn clone_shares_state() {
    let wd1 = Watchdog::new();
    let wd2 = wd1.clone();
    wd2.heartbeat();
    assert!(!wd1.stalled(60_000));
}

#[test]
fn spawn_monitor_does_not_panic_then_drops() {
    let wd = Watchdog::new();
    let flag = Arc::new(AtomicBool::new(false));
    let handle = spawn_monitor(
        wd,
        60_000,
        flag.clone(),
        flag.clone(),
        std::thread::current(),
    );
    std::thread::sleep(Duration::from_millis(50));
    // The spawned thread runs forever; we don't join — just confirm the
    // handle is movable and the watchdog keeps the thread alive. A long
    // timeout (60s) means the flag stays false during the test window.
    assert!(!flag.load(Ordering::Relaxed));
    drop(handle);
}

#[test]
fn spawn_monitor_escalates_shutdown_on_stall() {
    let wd = Watchdog::new();
    // Force the watchdog into a stalled state with a tiny timeout.
    let flag = Arc::new(AtomicBool::new(false));
    let _handle = spawn_monitor(wd, 0, flag.clone(), flag.clone(), std::thread::current());
    // First monitor tick is the DEFAULT_MONITOR_INTERVAL sleep; the flag
    // must be raised within ~600ms (2 × default poll).
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if flag.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("watchdog did not raise shutdown flag within 2s");
}
