// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use idle_runner::launcher::is_allowed_saver;

    use crate::config::DaemonConfig;
    use crate::daemon::idle_decision::PresentationDecision;
    use crate::daemon::presentation::{ActivePresentation, start_presentation};
    use crate::ooda::act::OodaActor;

    #[test]
    fn test_check_liveness_on_none_presentation_is_no_op() {
        let mut presentation = ActivePresentation::None;
        let mut preview_name = Some("beams".to_string());
        let mut current_saver = "beams".to_string();

        presentation.check_liveness(&mut preview_name, &mut current_saver);

        // When presentation is None, check_liveness is a no-op
        assert!(!presentation.is_active());
        assert_eq!(current_saver, "beams");
        assert_eq!(preview_name, Some("beams".to_string()));
    }

    #[test]
    fn test_start_presentation_preflight_rejects_invalid_savers() {
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };

        let mut presentation = ActivePresentation::None;
        let mut current_saver = String::new();
        let config = DaemonConfig::default();

        let invalid_names = [
            "../etc/passwd",
            "../../evil",
            "beams;rm",
            "nonexistent_saver_123",
            "",
            "random",
            "shuffle",
            "beams\0invalid",
        ];

        for name in invalid_names {
            let started = start_presentation(
                &overlay_presenter,
                &mut presentation,
                &mut current_saver,
                name.to_string(),
                "idle",
                &config,
            );

            assert!(
                !started,
                "Pre-flight validation failed to reject invalid saver '{name}'"
            );
            assert!(!presentation.is_active());
            assert_eq!(current_saver, "");
        }
    }

    #[test]
    fn test_rapid_saver_switching_liveness_and_validation() {
        let mut actor = OodaActor::new();
        let overlay_presenter: Arc<dyn idle_api::OverlaySurface> =
            match idle_api::WaylandOverlay::new() {
                Some(p) => Arc::new(p),
                None => return,
            };

        let mut presentation = ActivePresentation::None;
        let mut preview_name = None;
        let mut current_saver = String::new();
        let config = DaemonConfig::default();

        let test_cases = [
            ("beams", "idle"),
            ("invalid_saver_x", "preview"),
            ("matrix", "idle"),
            ("../traversal", "idle"),
            ("pipes", "preview"),
            ("beams", "preview"),
            ("nonexistent", "preview"),
        ];

        for _cycle in 0..5 {
            for (saver_name, reason) in test_cases {
                if reason == "preview" {
                    preview_name = Some(saver_name.to_string());
                }

                actor.execute(
                    PresentationDecision::Start {
                        name: saver_name.to_string(),
                        reason,
                    },
                    &overlay_presenter,
                    &mut presentation,
                    &mut preview_name,
                    &mut current_saver,
                    &config,
                    false,
                    false,
                    false,
                );

                if !is_allowed_saver(saver_name) {
                    assert!(
                        !presentation.is_active(),
                        "Invalid saver '{saver_name}' must not start presentation"
                    );
                    if reason == "preview" {
                        assert_eq!(
                            preview_name, None,
                            "Failed preview for '{saver_name}' must clear preview_name"
                        );
                    }
                }
            }
        }
    }
}
