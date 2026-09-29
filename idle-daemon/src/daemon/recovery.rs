// SPDX-License-Identifier: MIT

use std::time::Duration;

/// Which Wayland subsystems are unhealthy (never fatal to the process by itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum RuntimeFault {
    IdleMonitorDead,
    PresenterDead,
    BothDead,
}

/// Recovery plan when a Wayland subsystem dies mid-session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryPlan {
    pub stop_presentation: bool,
    pub clear_preview: bool,
    pub recreate_idle_monitor: bool,
    pub recreate_presenter: bool,
    /// Must always be false — presenter death must not kill idle-daemon.
    pub exit_process: bool,
}

/// Classify liveness of idle monitor + overlay presenter.
pub fn classify_runtime(idle_alive: bool, presenter_alive: bool) -> Result<(), RuntimeFault> {
    match (idle_alive, presenter_alive) {
        (true, true) => Ok(()),
        (false, true) => Err(RuntimeFault::IdleMonitorDead),
        (true, false) => Err(RuntimeFault::PresenterDead),
        (false, false) => Err(RuntimeFault::BothDead),
    }
}

/// Pure recovery policy (unit-tested).
pub fn recovery_plan(fault: RuntimeFault) -> RecoveryPlan {
    match fault {
        RuntimeFault::PresenterDead => RecoveryPlan {
            stop_presentation: true,
            clear_preview: true,
            recreate_idle_monitor: false,
            recreate_presenter: true,
            exit_process: false,
        },
        RuntimeFault::IdleMonitorDead => RecoveryPlan {
            stop_presentation: true,
            clear_preview: true,
            recreate_idle_monitor: true,
            recreate_presenter: false,
            exit_process: false,
        },
        RuntimeFault::BothDead => RecoveryPlan {
            stop_presentation: true,
            clear_preview: true,
            recreate_idle_monitor: true,
            recreate_presenter: true,
            exit_process: false,
        },
    }
}

/// Cooldown after a Wayland fault before auto-restarting idle presentation.
///
/// Without this, `system_idle` stays true and the tick loop immediately
/// restarts the saver → fault → recover thrash (~1 Hz).
///
/// `consecutive_faults` is 1-based (first fault → shortest wait).
pub fn present_cooldown_after_fault(consecutive_faults: u32) -> Duration {
    let n = consecutive_faults.max(1);
    // 5s, 10s, 20s, 40s, cap 60s
    let exp = n.saturating_sub(1).min(4);
    let secs = 5u64.saturating_mul(1u64 << exp).min(60);
    Duration::from_secs(secs)
}

/// Whether new idle presentations should be held (preview still allowed via
/// `decide_presentation` inhibit override).
pub fn should_hold_idle_presentation(cooldown_remaining: bool) -> bool {
    cooldown_remaining
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presenter_death_does_not_exit_process() {
        let plan = recovery_plan(RuntimeFault::PresenterDead);
        assert!(!plan.exit_process);
        assert!(plan.stop_presentation);
        assert!(plan.clear_preview);
        assert!(plan.recreate_presenter);
        assert!(!plan.recreate_idle_monitor);
    }

    #[test]
    fn idle_monitor_death_does_not_exit_process() {
        let plan = recovery_plan(RuntimeFault::IdleMonitorDead);
        assert!(!plan.exit_process);
        assert!(plan.recreate_idle_monitor);
    }

    #[test]
    fn both_dead_still_stays_alive() {
        let plan = recovery_plan(RuntimeFault::BothDead);
        assert!(!plan.exit_process);
        assert!(plan.recreate_idle_monitor);
        assert!(plan.recreate_presenter);
    }

    #[test]
    fn classify_ok_when_both_alive() {
        assert!(classify_runtime(true, true).is_ok());
    }

    #[test]
    fn classify_presenter_dead() {
        assert_eq!(
            classify_runtime(true, false),
            Err(RuntimeFault::PresenterDead)
        );
    }

    #[test]
    fn classify_idle_dead() {
        assert_eq!(
            classify_runtime(false, true),
            Err(RuntimeFault::IdleMonitorDead)
        );
    }

    #[test]
    fn classify_both_dead() {
        assert_eq!(classify_runtime(false, false), Err(RuntimeFault::BothDead));
    }

    #[test]
    fn every_fault_recovery_plan_never_exits_process() {
        let faults = [
            RuntimeFault::PresenterDead,
            RuntimeFault::IdleMonitorDead,
            RuntimeFault::BothDead,
        ];
        for fault in faults {
            let plan = recovery_plan(fault);
            assert!(!plan.exit_process);
            assert!(plan.stop_presentation);
            assert!(plan.clear_preview);
        }
    }

    #[test]
    fn presenter_death_recreates_only_presenter() {
        let plan = recovery_plan(RuntimeFault::PresenterDead);
        assert!(plan.recreate_presenter);
        assert!(!plan.recreate_idle_monitor);
    }

    #[test]
    fn idle_monitor_death_recreates_only_idle_monitor() {
        let plan = recovery_plan(RuntimeFault::IdleMonitorDead);
        assert!(plan.recreate_idle_monitor);
        assert!(!plan.recreate_presenter);
    }

    #[test]
    fn present_cooldown_grows_then_caps() {
        assert_eq!(present_cooldown_after_fault(1), Duration::from_secs(5));
        assert_eq!(present_cooldown_after_fault(2), Duration::from_secs(10));
        assert_eq!(present_cooldown_after_fault(3), Duration::from_secs(20));
        assert_eq!(present_cooldown_after_fault(4), Duration::from_secs(40));
        assert_eq!(present_cooldown_after_fault(5), Duration::from_mins(1));
        assert_eq!(present_cooldown_after_fault(99), Duration::from_mins(1));
        assert_eq!(present_cooldown_after_fault(0), Duration::from_secs(5));
    }

    #[test]
    fn hold_idle_only_while_cooldown_active() {
        assert!(should_hold_idle_presentation(true));
        assert!(!should_hold_idle_presentation(false));
    }

    #[test]
    fn thrash_guard_first_fault_is_at_least_five_seconds() {
        assert!(present_cooldown_after_fault(1).as_secs() >= 5);
    }

    #[test]
    fn recovery_plan_always_clears_preview_on_presenter_death() {
        let plan = recovery_plan(RuntimeFault::PresenterDead);
        assert!(plan.clear_preview);
        assert!(plan.stop_presentation);
        assert!(!plan.exit_process);
    }
}
