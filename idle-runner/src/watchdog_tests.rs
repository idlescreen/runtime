// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for the per-call watchdog.

use super::*;

#[test]
fn fresh_guard_has_not_overflowed() {
    let g = CallGuard::new(Duration::from_mins(1));
    assert!(!g.overflowed());
}

#[test]
fn guard_overflows_after_sleep() {
    let g = CallGuard::new(Duration::from_millis(5));
    std::thread::sleep(Duration::from_millis(20));
    assert!(g.overflowed());
}

#[test]
fn timeout_default_is_250ms() {
    assert_eq!(DEFAULT_WATCHDOG_TIMEOUT, Duration::from_millis(250));
}

#[test]
fn env_var_overrides_timeout() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("IDLE_WATCHDOG_TIMEOUT_MS", "1234") };
    let t = watchdog_timeout();
    unsafe { std::env::remove_var("IDLE_WATCHDOG_TIMEOUT_MS") };
    assert_eq!(t, Duration::from_millis(1234));
}

#[test]
fn env_var_invalid_falls_back_to_default() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("IDLE_WATCHDOG_TIMEOUT_MS", "not-a-number") };
    let t = watchdog_timeout();
    unsafe { std::env::remove_var("IDLE_WATCHDOG_TIMEOUT_MS") };
    assert_eq!(t, DEFAULT_WATCHDOG_TIMEOUT);
}
