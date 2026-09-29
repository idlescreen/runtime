// SPDX-License-Identifier: MIT

//! openOODA Pillar 3: Decide (Pure Presentation Policy Engine)

use idle_api::OverlaySurface;

use super::orient::SituationAssessment;
use crate::daemon::idle_decision::{IdlePolicyInput, PresentationDecision, decide_presentation};
use crate::daemon::presentation::{ActivePresentation, current_time_micros, pick_saver_name};

#[derive(Default)]
pub struct OodaDecisionEngine;

impl OodaDecisionEngine {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate pure policy matrix for current situation.
    pub fn decide(
        &self,
        situation: &SituationAssessment,
        presentation: &ActivePresentation,
        overlay_presenter: &dyn OverlaySurface,
        preview_name: Option<&str>,
        current_saver: &str,
    ) -> PresentationDecision {
        let input = IdlePolicyInput {
            is_active: presentation.is_active(),
            surface_visible: overlay_presenter.is_visible(),
            current_saver,
            preview_name,
            idle_enabled: situation.config.idle_enabled,
            system_idle: situation.system_idle,
            session_locked: situation.session_locked,
            inhibited: situation.effective_inhibited,
        };

        let idle_name = pick_saver_name(&situation.config, current_time_micros());

        let mut decision = decide_presentation(input, &idle_name);
        // A user-forced activation uses Daemon launch mode (installed paths
        // only) — unlike preview, which may resolve dev-tree builds.
        if situation.manual_activate
            && let PresentationDecision::Start { reason, .. } = &mut decision
        {
            *reason = "manual";
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DaemonConfig;
    use std::sync::Arc;

    #[test]
    fn test_ooda_decision_engine_uses_live_current_saver() {
        let engine = OodaDecisionEngine::new();
        let config = DaemonConfig {
            active_saver: Some("random".to_string()),
            idle_enabled: true,
            ..DaemonConfig::default()
        };

        let situation = SituationAssessment {
            config,
            system_idle: true,
            session_locked: false,
            effective_inhibited: false,
            cooldown_active: false,
            manual_activate: false,
        };
        let presentation = ActivePresentation::None;
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };

        // When current_saver is "matrix" and previewing "beams", decide sees live "matrix" != "beams" -> Start "beams"
        let decision_start = engine.decide(
            &situation,
            &presentation,
            &*overlay_presenter,
            Some("beams"),
            "matrix",
        );

        assert_eq!(
            decision_start,
            PresentationDecision::Start {
                name: "beams".to_string(),
                reason: "preview"
            }
        );
    }

    #[test]
    fn test_ooda_decision_engine_lock_clears_preview_and_stops_presentation() {
        let engine = OodaDecisionEngine::new();
        let situation = SituationAssessment {
            config: DaemonConfig::default(),
            system_idle: true,
            session_locked: true,
            effective_inhibited: false,
            cooldown_active: false,
            manual_activate: false,
        };
        let presentation = ActivePresentation::None;
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };

        let decision = engine.decide(
            &situation,
            &presentation,
            &*overlay_presenter,
            Some("matrix"),
            "",
        );

        assert_eq!(
            decision,
            PresentationDecision::Stop {
                clear_preview: true
            }
        );
    }

    #[test]
    fn test_ooda_decision_engine_preview_overrides_inhibit() {
        let engine = OodaDecisionEngine::new();
        let situation = SituationAssessment {
            config: DaemonConfig::default(),
            system_idle: false,
            session_locked: false,
            effective_inhibited: true,
            cooldown_active: false,
            manual_activate: false,
        };
        let presentation = ActivePresentation::None;
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };

        let decision = engine.decide(
            &situation,
            &presentation,
            &*overlay_presenter,
            Some("matrix"),
            "",
        );

        assert_eq!(
            decision,
            PresentationDecision::Start {
                name: "matrix".to_string(),
                reason: "preview"
            }
        );
    }

    #[test]
    fn test_ooda_decision_engine_manual_activate_uses_daemon_mode() {
        let engine = OodaDecisionEngine::new();
        let situation = SituationAssessment {
            config: DaemonConfig {
                active_saver: Some("beams".to_string()),
                ..DaemonConfig::default()
            },
            system_idle: false,
            session_locked: false,
            effective_inhibited: false,
            cooldown_active: false,
            manual_activate: true,
        };
        let presentation = ActivePresentation::None;
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };

        // orient() queues the resolved saver into preview_name; decide must
        // tag the Start "manual" so act launches in Daemon mode.
        let decision = engine.decide(
            &situation,
            &presentation,
            &*overlay_presenter,
            Some("beams"),
            "",
        );
        assert_eq!(
            decision,
            PresentationDecision::Start {
                name: "beams".to_string(),
                reason: "manual"
            }
        );
    }

    #[test]
    fn test_ooda_decision_engine_inhibit_stops_idle_presentation() {
        let engine = OodaDecisionEngine::new();
        let situation = SituationAssessment {
            config: DaemonConfig::default(),
            system_idle: true,
            session_locked: false,
            effective_inhibited: true,
            cooldown_active: false,
            manual_activate: false,
        };
        let presentation = ActivePresentation::None;
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };

        let decision = engine.decide(&situation, &presentation, &*overlay_presenter, None, "");

        assert_eq!(decision, PresentationDecision::Hold);
    }
}
