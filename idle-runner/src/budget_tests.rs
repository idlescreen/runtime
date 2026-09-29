// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for the per-saver CPU budget.
//!
//! Cgroup v2 attach requires write access to /sys/fs/cgroup; in a developer
//! shell that is usually denied, so most tests assert the safe fall-back
//! rather than the kernel-side throttle. The kernel-side path is exercised
//! when the test runner has cgroup write access (CI as root).

use super::*;

#[test]
fn attach_succeeds_when_cgroup_unwritable() {
    // The dev shell cannot create /sys/fs/cgroup children; this asserts the
    // module returns Unenforced without panicking or erroring.
    let b = CpuBudget::attach("budget-test-no-cgroup").expect("attach must not error");
    assert_eq!(b.status(), BudgetStatus::Unenforced);
    assert!(
        b.usage_micros() < 1_000_000,
        "fresh budget should be near zero"
    );
}

#[test]
fn hard_limit_not_exceeded_before_window() {
    let b = CpuBudget::attach("budget-test-no-cgroup").expect("attach");
    // Window is 5s — well under that, the limit is not enforced.
    assert!(
        !b.exceeded_hard_limit(),
        "must not trip before the window elapses"
    );
}

#[test]
fn hard_limit_window_constant() {
    let b = CpuBudget::attach("budget-test-no-cgroup").expect("attach");
    assert_eq!(b.hard_window_secs, DEFAULT_HARD_LIMIT_WINDOW_SECS);
    assert_eq!(
        b.hard_limit_us,
        b.quota_us * DEFAULT_HARD_LIMIT_MULTIPLIER as u64
    );
}

#[test]
fn quota_overrides_env_var() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("IDLE_CPU_QUOTA_PCT", "75") };
    let b = CpuBudget::attach("budget-test-env").expect("attach");
    unsafe { std::env::remove_var("IDLE_CPU_QUOTA_PCT") };
    let expected_period = 100_000_u64;
    let expected_quota = expected_period * 75 / 100;
    assert_eq!(b.quota_us(), expected_quota);
    assert_eq!(b.period_us(), expected_period);
}

#[test]
fn quota_clamps_out_of_range() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    // IDLE_CPU_QUOTA_PCT must clamp to [1, 1000].
    unsafe { std::env::set_var("IDLE_CPU_QUOTA_PCT", "0") };
    let b = CpuBudget::attach("budget-test-clamp-low").expect("attach");
    unsafe { std::env::remove_var("IDLE_CPU_QUOTA_PCT") };
    assert_eq!(b.quota_us(), 100_000 / 100);
}

#[test]
fn release_does_not_panic_when_unenforced() {
    let b = CpuBudget::attach("budget-test-release").expect("attach");
    assert_eq!(b.status(), BudgetStatus::Unenforced);
    b.release();
}
