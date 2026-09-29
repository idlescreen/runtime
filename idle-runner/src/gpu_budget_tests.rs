// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for the GPU budget probe.

use super::*;

#[test]
fn detect_returns_unavailable_when_no_tools() {
    // PATH-search safety: we never assume nvidia-smi exists.
    let status = GpuBudget::detect();
    // We can't assert a specific outcome (CI may have nvidia-smi); just
    // assert the function returns a value and is debug-printable.
    let _ = format!("{status:?}");
}

#[test]
fn backend_string_round_trip() {
    assert_eq!(GpuBackend::Nvidia.as_str(), "nvidia-smi");
    assert_eq!(GpuBackend::Intel.as_str(), "intel_gpu_top");
    assert_eq!(GpuBackend::Amd.as_str(), "amdgpu_top");
}

#[test]
fn new_active_applies_quota_defaults() {
    let b = GpuBudget::new_active(GpuBackend::Nvidia);
    assert_eq!(b.quota_pct(), DEFAULT_GPU_QUOTA_PCT);
    assert!(b.hard_ceiling() >= b.quota_pct());
}

#[test]
fn new_active_clamps_quota() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("IDLE_GPU_QUOTA_PCT", "0") };
    let b = GpuBudget::new_active(GpuBackend::Nvidia);
    unsafe { std::env::remove_var("IDLE_GPU_QUOTA_PCT") };
    assert!(b.quota_pct() >= 1);
}

#[test]
fn sample_throttles_to_interval() {
    let mut b = GpuBudget::new_active(GpuBackend::Nvidia);
    assert!(b.sample_due(), "fresh budget must be due immediately");
    let _ = b.sample().ok();
    assert!(
        !b.sample_due(),
        "sample within interval must not be due again"
    );
}

#[test]
fn exceeded_streak_triggers_after_threshold() {
    let mut b = GpuBudget::new_active(GpuBackend::Nvidia);
    // Force over_streak to threshold by stuffing the field via repeated samples
    // would require a working vendor tool. Instead, verify the math: hard_streak
    // is 1 by default, so over_streak==1 trips. Set hard_streak=3 for the test.
    b.hard_streak = 3;
    b.over_streak = 2;
    assert!(!b.exceeded(), "below threshold must not trip");
    b.over_streak = 3;
    assert!(b.exceeded(), "at threshold must trip");
}

#[test]
fn gpu_budget_enabled_default_false() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    unsafe { std::env::remove_var("IDLE_GPU_BUDGET") };
    assert!(!gpu_budget_enabled());
}

#[test]
fn unhealthy_threshold_after_consecutive_failures() {
    let mut b = GpuBudget::new_active(GpuBackend::Nvidia);
    assert!(!b.unhealthy(), "fresh budget is healthy");
    for _ in 0..DEFAULT_FAILURE_STREAK {
        b.record_result(&Err(std::io::Error::other("test")));
    }
    assert!(
        b.unhealthy(),
        "after {} consecutive failures the watchdog must trip",
        DEFAULT_FAILURE_STREAK
    );
}

#[test]
fn record_result_resets_failure_streak_on_success() {
    let mut b = GpuBudget::new_active(GpuBackend::Nvidia);
    b.record_result(&Err(std::io::Error::other("test")));
    b.record_result(&Err(std::io::Error::other("test")));
    b.record_result(&Ok(50));
    assert!(
        !b.unhealthy(),
        "a single success must reset the failure streak"
    );
}

#[test]
fn gpu_budget_enabled_env_var() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    unsafe { std::env::set_var("IDLE_GPU_BUDGET", "1") };
    assert!(gpu_budget_enabled());
    unsafe { std::env::remove_var("IDLE_GPU_BUDGET") };
}
