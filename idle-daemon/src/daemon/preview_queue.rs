// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Pure preview command queue effects (package-gate integration without Wayland).

use super::runtime::{RuntimeFault, recovery_plan};

/// Apply a Preview D-Bus command to sticky preview state.
pub fn queue_preview(preview_name: &mut Option<String>, name: impl Into<String>) {
    *preview_name = Some(name.into());
}

/// Apply StopPresentation — clears sticky preview.
pub fn queue_stop(preview_name: &mut Option<String>) {
    *preview_name = None;
}

/// After a Wayland fault recovery plan that clears preview.
pub fn apply_fault_clear_preview(preview_name: &mut Option<String>, fault: RuntimeFault) {
    let plan = recovery_plan(fault);
    if plan.clear_preview {
        *preview_name = None;
    }
}

#[cfg(test)]
mod tests {
    use super::super::idle_decision::{IdlePolicyInput, PresentationDecision, decide_presentation};
    use super::*;

    /// Decide presentation after applying optional preview queue + inhibit flag.
    pub fn decide_after_queue(
        preview_name: Option<&str>,
        inhibited: bool,
        system_idle: bool,
        idle_saver: &str,
    ) -> PresentationDecision {
        let input = IdlePolicyInput {
            is_active: false,
            surface_visible: true,
            current_saver: "",
            preview_name,
            idle_enabled: true,
            system_idle,
            session_locked: false,
            inhibited,
        };
        decide_presentation(input, idle_saver)
    }

    #[test]
    fn queue_preview_sets_name() {
        let mut p = None;
        queue_preview(&mut p, "beams");
        assert_eq!(p.as_deref(), Some("beams"));
    }

    #[test]
    fn queue_preview_overwrites() {
        let mut p = Some("ripple".into());
        queue_preview(&mut p, "cosmos");
        assert_eq!(p.as_deref(), Some("cosmos"));
    }

    #[test]
    fn queue_stop_clears() {
        let mut p = Some("beams".into());
        queue_stop(&mut p);
        assert!(p.is_none());
    }

    #[test]
    fn double_stop_is_idempotent() {
        let mut p = Some("beams".into());
        queue_stop(&mut p);
        queue_stop(&mut p);
        assert!(p.is_none());
    }

    #[test]
    fn presenter_fault_clears_sticky_preview() {
        let mut p = Some("beams".into());
        apply_fault_clear_preview(&mut p, RuntimeFault::PresenterDead);
        assert!(p.is_none());
    }

    #[test]
    fn after_fault_idle_does_not_auto_restart_when_inhibited() {
        let mut p = Some("beams".into());
        apply_fault_clear_preview(&mut p, RuntimeFault::PresenterDead);
        let d = decide_after_queue(p.as_deref(), true, true, "ripple");
        assert_eq!(d, PresentationDecision::Hold);
    }

    #[test]
    fn requeue_after_fault_starts_preview_even_if_inhibited() {
        let mut p = Some("beams".into());
        apply_fault_clear_preview(&mut p, RuntimeFault::BothDead);
        queue_preview(&mut p, "storm");
        let d = decide_after_queue(p.as_deref(), true, false, "ripple");
        assert_eq!(
            d,
            PresentationDecision::Start {
                name: "storm".into(),
                reason: "preview",
            }
        );
    }

    #[test]
    fn forced_preview_beats_system_idle_saver() {
        let d = decide_after_queue(Some("beams"), false, true, "ripple");
        assert_eq!(
            d,
            PresentationDecision::Start {
                name: "beams".into(),
                reason: "preview",
            }
        );
    }

    #[test]
    fn idle_starts_when_uninhibited_no_preview() {
        let d = decide_after_queue(None, false, true, "ripple");
        assert_eq!(
            d,
            PresentationDecision::Start {
                name: "ripple".into(),
                reason: "idle",
            }
        );
    }

    #[test]
    fn stop_then_idle_can_start() {
        let mut p = Some("beams".into());
        queue_stop(&mut p);
        let d = decide_after_queue(p.as_deref(), false, true, "cosmos");
        assert_eq!(
            d,
            PresentationDecision::Start {
                name: "cosmos".into(),
                reason: "idle",
            }
        );
    }

    #[test]
    fn multi_preview_switch_last_wins() {
        let mut p = None;
        queue_preview(&mut p, "beams");
        queue_preview(&mut p, "glyphs");
        queue_preview(&mut p, "radar");
        assert_eq!(p.as_deref(), Some("radar"));
        let d = decide_after_queue(p.as_deref(), true, true, "ripple");
        assert_eq!(
            d,
            PresentationDecision::Start {
                name: "radar".into(),
                reason: "preview",
            }
        );
    }

    #[test]
    fn all_faults_clear_preview_via_plan() {
        for fault in [
            RuntimeFault::PresenterDead,
            RuntimeFault::IdleMonitorDead,
            RuntimeFault::BothDead,
        ] {
            let mut p = Some("beams".into());
            apply_fault_clear_preview(&mut p, fault);
            assert!(p.is_none(), "{fault:?} should clear sticky preview");
        }
    }

    /// Closed-loop simulation: control → fault → requeue (no Wayland).
    #[test]
    fn closed_loop_preview_fault_requeue_status_story() {
        let mut preview = None;
        // User presses p
        queue_preview(&mut preview, "beams");
        assert_eq!(
            decide_after_queue(preview.as_deref(), true, false, "ripple"),
            PresentationDecision::Start {
                name: "beams".into(),
                reason: "preview",
            }
        );
        // Presenter dies; recovery clears sticky preview
        apply_fault_clear_preview(&mut preview, RuntimeFault::PresenterDead);
        assert!(preview.is_none());
        // Cooldown path = inhibited for idle; must not thrash
        assert_eq!(
            decide_after_queue(None, true, true, "ripple"),
            PresentationDecision::Hold
        );
        // User presses p again
        queue_preview(&mut preview, "beams");
        assert_eq!(
            decide_after_queue(preview.as_deref(), true, true, "ripple"),
            PresentationDecision::Start {
                name: "beams".into(),
                reason: "preview",
            }
        );
        // Stop
        queue_stop(&mut preview);
        assert_eq!(
            decide_after_queue(None, false, false, "ripple"),
            PresentationDecision::Hold
        );
    }

    #[test]
    fn closed_loop_stop_during_idle_request() {
        // Idle would start, but stop clears any sticky preview first.
        let mut preview = Some("beams".into());
        queue_stop(&mut preview);
        let d = decide_after_queue(preview.as_deref(), false, true, "cosmos");
        assert_eq!(
            d,
            PresentationDecision::Start {
                name: "cosmos".into(),
                reason: "idle",
            }
        );
    }
}
