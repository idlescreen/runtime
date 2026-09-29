// SPDX-License-Identifier: Apache-2.0

//! Unit tests for pure idle presentation policy.

use super::{IdlePolicyInput, PresentationDecision, decide_presentation};

fn base() -> IdlePolicyInput<'static> {
    IdlePolicyInput {
        is_active: false,
        surface_visible: true,
        current_saver: "",
        preview_name: None,
        idle_enabled: true,
        system_idle: false,
        session_locked: false,
        inhibited: false,
    }
}

#[test]
fn preview_survives_activity() {
    let mut i = base();
    i.is_active = true;
    i.current_saver = "storm";
    i.preview_name = Some("storm");
    i.system_idle = false;
    assert_eq!(decide_presentation(i, "beams"), PresentationDecision::Hold);
}

#[test]
fn lock_stops_and_clears_preview() {
    let mut i = base();
    i.is_active = true;
    i.session_locked = true;
    assert_eq!(
        decide_presentation(i, "beams"),
        PresentationDecision::Stop {
            clear_preview: true,
        }
    );
}

#[test]
fn inhibit_stops_idle_presentation_not_preview_flag() {
    // Idle presentation without preview_name is stopped by inhibit.
    let mut i = base();
    i.is_active = true;
    i.current_saver = "beams";
    i.inhibited = true;
    assert_eq!(
        decide_presentation(i, "beams"),
        PresentationDecision::Stop {
            clear_preview: false,
        }
    );
}

#[test]
fn preview_starts_even_when_inhibited() {
    // Regression: Grok logind idle / MPRIS used to clear TUI `p` before start.
    let mut i = base();
    i.is_active = false;
    i.preview_name = Some("beams");
    i.inhibited = true;
    assert_eq!(
        decide_presentation(i, "ripple"),
        PresentationDecision::Start {
            name: "beams".into(),
            reason: "preview",
        }
    );
}

#[test]
fn preview_holds_when_active_and_inhibited() {
    let mut i = base();
    i.is_active = true;
    i.current_saver = "beams";
    i.preview_name = Some("beams");
    i.inhibited = true;
    assert_eq!(decide_presentation(i, "ripple"), PresentationDecision::Hold);
}

#[test]
fn preview_overrides_idle() {
    let mut i = base();
    i.system_idle = true;
    i.preview_name = Some("storm");
    assert_eq!(
        decide_presentation(i, "beams"),
        PresentationDecision::Start {
            name: "storm".into(),
            reason: "preview",
        }
    );
}

#[test]
fn preview_switch_restarts() {
    let mut i = base();
    i.is_active = true;
    i.current_saver = "beams";
    i.preview_name = Some("storm");
    assert_eq!(
        decide_presentation(i, "beams"),
        PresentationDecision::Start {
            name: "storm".into(),
            reason: "preview",
        }
    );
}

#[test]
fn lock_clears_preview_even_when_inactive() {
    let mut i = base();
    i.preview_name = Some("storm");
    i.session_locked = true;
    assert_eq!(
        decide_presentation(i, "beams"),
        PresentationDecision::Stop {
            clear_preview: true,
        }
    );
}

#[test]
fn lock_and_inhibit_hold_when_inactive_no_preview() {
    let mut i = base();
    i.session_locked = true;
    assert_eq!(decide_presentation(i, "beams"), PresentationDecision::Hold);
    i.session_locked = false;
    i.inhibited = true;
    assert_eq!(decide_presentation(i, "beams"), PresentationDecision::Hold);
}

#[test]
fn after_preview_cleared_by_recovery_must_requeue() {
    // Recovery plan clear_preview → preview_name=None. Without a new D-Bus
    // Preview command, policy must Hold (not auto-restart idle/preview).
    let mut i = base();
    i.is_active = false;
    i.preview_name = None;
    i.inhibited = true;
    i.system_idle = true;
    assert_eq!(
        decide_presentation(i, "beams"),
        PresentationDecision::Hold,
        "after fault recovery, preview is sticky only if re-queued"
    );
}

#[test]
fn requeued_preview_starts_after_recovery_clear() {
    // Second TUI `p` after recovery: preview_name set again → Start.
    let mut i = base();
    i.is_active = false;
    i.preview_name = Some("bursts");
    i.inhibited = true;
    assert_eq!(
        decide_presentation(i, "beams"),
        PresentationDecision::Start {
            name: "bursts".into(),
            reason: "preview",
        }
    );
}

#[test]
fn preview_stop_clears_without_starting_idle() {
    // idlescreen stop: preview_name gone, not idle → Hold.
    let mut i = base();
    i.is_active = false;
    i.preview_name = None;
    i.system_idle = false;
    assert_eq!(decide_presentation(i, "beams"), PresentationDecision::Hold);
}

#[test]
fn cooldown_as_inhibit_blocks_idle_not_forced_preview() {
    // Tick loop maps present_cooldown → inhibited=true for idle path only.
    // Forced preview must still Start (same as MPRIS/logind inhibit).
    let mut idle_blocked = base();
    idle_blocked.system_idle = true;
    idle_blocked.inhibited = true;
    assert_eq!(
        decide_presentation(idle_blocked, "ripple"),
        PresentationDecision::Hold,
        "post-fault cooldown must not thrash idle auto-start"
    );

    let mut force = base();
    force.preview_name = Some("beams");
    force.inhibited = true;
    assert_eq!(
        decide_presentation(force, "ripple"),
        PresentationDecision::Start {
            name: "beams".into(),
            reason: "preview",
        }
    );
}

#[test]
fn activity_does_not_stop_forced_preview_while_inhibited() {
    let mut i = base();
    i.is_active = true;
    i.current_saver = "beams";
    i.preview_name = Some("beams");
    i.system_idle = false;
    i.inhibited = true;
    assert_eq!(decide_presentation(i, "ripple"), PresentationDecision::Hold);
}
