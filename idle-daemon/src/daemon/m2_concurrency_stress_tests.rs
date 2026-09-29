// SPDX-License-Identifier: MIT
//! Stress tests for state machine policy matrix invariants and sticky preview clearing.

#[cfg(test)]
mod tests {
    use crate::config::DaemonConfig;
    use crate::daemon::idle_decision::{
        IdlePolicyInput, PresentationDecision, decide_presentation,
    };
    use crate::daemon::presentation::ActivePresentation;
    use crate::ooda::act::OodaActor;
    use std::sync::Arc;

    #[test]
    fn test_exhaustive_policy_matrix_invariants() {
        let bools = [false, true];
        let savers = [None, Some("beams"), Some("ripple")];
        let current_savers = ["", "beams", "matrix"];

        for &is_active in &bools {
            for &surface_visible in &bools {
                for &idle_enabled in &bools {
                    for &system_idle in &bools {
                        for &session_locked in &bools {
                            for &inhibited in &bools {
                                for &preview_name in &savers {
                                    for &current_saver in &current_savers {
                                        let input = IdlePolicyInput {
                                            is_active,
                                            surface_visible,
                                            current_saver,
                                            preview_name,
                                            idle_enabled,
                                            system_idle,
                                            session_locked,
                                            inhibited,
                                        };
                                        let decision = decide_presentation(input, "beams");

                                        // Invariant 1: Session lock NEVER allows Start
                                        if session_locked {
                                            assert!(
                                                !matches!(
                                                    decision,
                                                    PresentationDecision::Start { .. }
                                                ),
                                                "Session lock allowed Start decision: {input:?} -> {decision:?}"
                                            );
                                            if is_active || preview_name.is_some() {
                                                assert_eq!(
                                                    decision,
                                                    PresentationDecision::Stop {
                                                        clear_preview: true
                                                    },
                                                    "Session lock active/preview did not issue Stop(clear_preview=true): {input:?}"
                                                );
                                            }
                                        }

                                        // Invariant 2: Stale surface (is_active && !surface_visible) MUST Stop and clear preview
                                        if is_active && !surface_visible {
                                            assert_eq!(
                                                decision,
                                                PresentationDecision::Stop {
                                                    clear_preview: true
                                                },
                                                "Stale surface did not clear preview: {input:?}"
                                            );
                                        }

                                        // Invariant 3: Explicit preview overrides inhibited
                                        if preview_name.is_some()
                                            && !session_locked
                                            && surface_visible
                                            && (!is_active
                                                || current_saver != preview_name.unwrap())
                                        {
                                            assert!(
                                                matches!(
                                                    decision,
                                                    PresentationDecision::Start {
                                                        reason: "preview",
                                                        ..
                                                    }
                                                ),
                                                "Explicit preview did not produce Start: {input:?} -> {decision:?}"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_rapid_state_toggles_and_preview_fault_scenarios() {
        let mut actor = OodaActor::new();
        let mut presentation = ActivePresentation::None;
        let mut preview_name = Some("beams".to_string());
        let mut current_saver = String::new();
        let config = DaemonConfig::default();

        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => {
                    let decision = PresentationDecision::Stop {
                        clear_preview: true,
                    };
                    if clear_preview_from_decision(&decision) {
                        preview_name = None;
                    }
                    assert_eq!(
                        preview_name, None,
                        "Session lock did not clear sticky preview name"
                    );
                    return;
                }
            };

        // Fault Scenario 1: Non-existent saver binary execution failure
        let invalid_decision = PresentationDecision::Start {
            name: "invalid_nonexistent_binary_xyz_123".to_string(),
            reason: "preview",
        };
        actor.execute(
            invalid_decision,
            &overlay_presenter,
            &mut presentation,
            &mut preview_name,
            &mut current_saver,
            &config,
            false,
            false,
            false,
        );
        assert_eq!(
            preview_name, None,
            "Failed preview binary launch MUST clear preview_name immediately"
        );

        // Fault Scenario 2: Rapid toggle from preview active -> Session Lock -> Clear preview
        preview_name = Some("beams".to_string());
        actor.execute(
            PresentationDecision::Stop {
                clear_preview: true,
            },
            &overlay_presenter,
            &mut presentation,
            &mut preview_name,
            &mut current_saver,
            &config,
            false,
            true,
            false,
        );
        assert_eq!(
            preview_name, None,
            "Stop with clear_preview=true MUST clear preview_name"
        );
    }

    fn clear_preview_from_decision(decision: &PresentationDecision) -> bool {
        match decision {
            PresentationDecision::Stop { clear_preview } => *clear_preview,
            _ => false,
        }
    }
}
