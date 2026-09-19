// SPDX-License-Identifier: MIT

//! openOODA Pillar 4: Act (Presentation Side-Effects & Surface Execution)

use std::sync::Arc;

use idle_api::OverlaySurface;

use crate::config::DaemonConfig;
use crate::daemon::idle_decision::PresentationDecision;
use crate::daemon::presentation::{ActivePresentation, start_presentation, stop_presentation};

#[derive(Default)]
pub struct OodaActor;

impl OodaActor {
    pub fn new() -> Self {
        Self
    }

    /// Execute target presentation decision on Wayland overlay presenter.
    #[allow(clippy::too_many_arguments)]
    pub fn execute(
        &mut self,
        decision: PresentationDecision,
        overlay_presenter: &Arc<dyn OverlaySurface>,
        presentation: &mut ActivePresentation,
        preview_name: &mut Option<String>,
        current_saver: &mut String,
        config: &DaemonConfig,
        system_idle: bool,
        _session_locked: bool,
        _inhibited: bool,
    ) {
        presentation.check_liveness(preview_name, current_saver);

        match decision {
            PresentationDecision::Hold => {}
            PresentationDecision::Stop { clear_preview } => {
                if presentation.is_active() {
                    stop_presentation(Some(overlay_presenter), presentation);
                    current_saver.clear();
                    if !system_idle && preview_name.is_none() {
                        idle_log::info!("system activity detected. presentation stopped.");
                    }
                }
                if clear_preview {
                    *preview_name = None;
                }
            }
            PresentationDecision::Start { name, reason } => {
                if presentation.is_active() && current_saver.as_str() != name.as_str() {
                    stop_presentation(Some(overlay_presenter), presentation);
                    current_saver.clear();
                }
                if !presentation.is_active() {
                    let started = start_presentation(
                        overlay_presenter,
                        presentation,
                        current_saver,
                        name,
                        reason,
                        config,
                    );
                    if !started && reason != "idle" {
                        idle_log::warn!("forced presentation launch failed; clearing queued state");
                        *preview_name = None;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ooda_actor_executes_stop_decision_directly() {
        let mut actor = OodaActor::new();
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };
        let mut presentation = ActivePresentation::None;
        let mut preview_name = Some("beams".to_string());
        let mut current_saver = String::new();
        let config = DaemonConfig::default();

        let decision = PresentationDecision::Stop {
            clear_preview: true,
        };

        actor.execute(
            decision,
            &overlay_presenter,
            &mut presentation,
            &mut preview_name,
            &mut current_saver,
            &config,
            false,
            false,
            false,
        );

        assert_eq!(preview_name, None);
    }

    #[test]
    fn test_ooda_actor_hold_does_not_mutate_state() {
        let mut actor = OodaActor::new();
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };
        let mut presentation = ActivePresentation::None;
        let mut preview_name = Some("matrix".to_string());
        let mut current_saver = "matrix".to_string();
        let config = DaemonConfig::default();

        actor.execute(
            PresentationDecision::Hold,
            &overlay_presenter,
            &mut presentation,
            &mut preview_name,
            &mut current_saver,
            &config,
            true,
            false,
            false,
        );

        assert_eq!(preview_name, Some("matrix".to_string()));
        assert_eq!(current_saver, "matrix");
    }

    #[test]
    fn test_ooda_actor_stop_without_clear_preview() {
        let mut actor = OodaActor::new();
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };
        let mut presentation = ActivePresentation::None;
        let mut preview_name = Some("matrix".to_string());
        let mut current_saver = "matrix".to_string();
        let config = DaemonConfig::default();

        actor.execute(
            PresentationDecision::Stop {
                clear_preview: false,
            },
            &overlay_presenter,
            &mut presentation,
            &mut preview_name,
            &mut current_saver,
            &config,
            false,
            false,
            true,
        );

        // preview_name should NOT be cleared because clear_preview is false
        assert_eq!(preview_name, Some("matrix".to_string()));
        assert_eq!(current_saver, "matrix"); // presentation was None, so current_saver wasn't cleared by stop_presentation
    }

    #[test]
    fn test_ooda_actor_failed_preview_clears_preview_state() {
        let mut actor = OodaActor::new();
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };
        let mut presentation = ActivePresentation::None;
        let mut preview_name = Some("nonexistent_invalid_saver_123".to_string());
        let mut current_saver = String::new();
        let config = DaemonConfig::default();

        let decision = PresentationDecision::Start {
            name: "nonexistent_invalid_saver_123".to_string(),
            reason: "preview",
        };

        actor.execute(
            decision,
            &overlay_presenter,
            &mut presentation,
            &mut preview_name,
            &mut current_saver,
            &config,
            false,
            false,
            false,
        );

        // When launching invalid preview saver fails, preview_name MUST be cleared to prevent sticky state
        assert_eq!(preview_name, None);
    }
}
