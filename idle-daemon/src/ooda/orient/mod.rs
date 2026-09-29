// SPDX-License-Identifier: MIT

//! openOODA Pillar 2: Orient (Situation Assessment & Fault Management)

use std::sync::Arc;
use std::time::{Duration, Instant};

use idle_api::IdleSource;
use idle_api::OverlaySurface;
#[cfg(target_os = "linux")]
use wayland_idle::IdleMonitor;

use super::observe::RawObservation;
use crate::config::DaemonConfig;
use crate::controller::{DaemonCommand, DaemonController};
use crate::daemon::presentation::{
    ActivePresentation, current_time_micros, pick_saver_name, stop_presentation,
};
use crate::daemon::preview_queue::{apply_fault_clear_preview, queue_preview, queue_stop};
use crate::daemon::runtime::{
    check_runtime_alive, present_cooldown_after_fault, recovery_plan, should_hold_idle_presentation,
};

/// Normalized situation assessment synthesized from raw observations.
#[derive(Debug, Clone)]
pub struct SituationAssessment {
    pub system_idle: bool,
    pub session_locked: bool,
    pub effective_inhibited: bool,
    pub cooldown_active: bool,
    /// `Activate` was drained this tick — the forced start should launch in
    /// `Daemon` mode (installed paths only), not preview/dev paths.
    pub manual_activate: bool,
    pub config: DaemonConfig,
}

#[derive(Default)]
pub struct OodaOrientator;

impl OodaOrientator {
    pub fn new() -> Self {
        Self
    }

    /// Process commands, verify Wayland health, and compute effective inhibition state.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::result_unit_err, clippy::collapsible_if)]
    pub fn orient(
        &mut self,
        raw: RawObservation,
        controller: &DaemonController,
        idle_monitor: &mut Box<dyn IdleSource>,
        overlay_presenter: &mut Arc<dyn OverlaySurface>,
        presentation: &mut ActivePresentation,
        preview_name: &mut Option<String>,
        current_saver: &mut String,
        consecutive_faults: &mut u32,
        present_cooldown_until: &mut Option<Instant>,
    ) -> Result<SituationAssessment, ()> {
        // 1. Process incoming commands that affect orient/cooldown state
        let mut manual_activate = false;
        for command in raw.commands {
            match command {
                DaemonCommand::Preview(name) => {
                    idle_log::info!(saver = %name, "queued preview command");
                    *present_cooldown_until = None;
                    *consecutive_faults = 0;
                    queue_preview(preview_name, name);
                }
                DaemonCommand::Activate => {
                    // `idlescreen start`: force the configured saver through
                    // the same user-initiated path as preview, but tagged so
                    // decide launches it in Daemon mode (installed only).
                    let name = pick_saver_name(&raw.config, current_time_micros());
                    idle_log::info!(saver = %name, "queued activate command");
                    *present_cooldown_until = None;
                    *consecutive_faults = 0;
                    queue_preview(preview_name, name);
                    manual_activate = true;
                }
                DaemonCommand::StopPresentation => {
                    idle_log::info!("queued stop-presentation command");
                    queue_stop(preview_name);
                    stop_presentation(Some(overlay_presenter), presentation);
                    current_saver.clear();
                }
                DaemonCommand::SetTimeout(minutes) => {
                    idle_monitor
                        .set_timeout(Duration::from_secs(minutes.saturating_mul(60) as u64));
                }
                other => {
                    let _ = controller.apply_command(other);
                }
            }
        }

        // 2. Runtime health evaluation: check if Wayland compositor connection broke
        if let Err(fault) = check_runtime_alive(&**idle_monitor, &**overlay_presenter) {
            *consecutive_faults = consecutive_faults.saturating_add(1);
            let cooldown = present_cooldown_after_fault(*consecutive_faults);
            *present_cooldown_until = Some(Instant::now() + cooldown);
            idle_log::warn!(
                consecutive_faults = *consecutive_faults,
                cooldown_secs = cooldown.as_secs(),
                "holding idle auto-start after Wayland fault"
            );

            // Execute non-terminating recovery plan
            let plan = recovery_plan(fault);
            if plan.stop_presentation {
                stop_presentation(Some(overlay_presenter), presentation);
                current_saver.clear();
            }
            apply_fault_clear_preview(preview_name, fault);
            if plan.recreate_presenter {
                if let Some(p) = idle_api::WaylandOverlay::new() {
                    *overlay_presenter = Arc::new(p);
                }
            }
            if plan.recreate_idle_monitor {
                let dur =
                    Duration::from_secs(raw.config.idle_timeout_mins.saturating_mul(60) as u64);
                if let Some(m) = IdleMonitor::new_timeout(dur) {
                    *idle_monitor = Box::new(m);
                }
            }

            return Err(());
        }

        // 3. User activity clears fault streak
        if !raw.system_idle {
            *consecutive_faults = 0;
        }

        // 4. Cooldown calculation & effective inhibition merger
        let cooldown_active = present_cooldown_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false);
        if !cooldown_active {
            *present_cooldown_until = None;
        }

        let mut effective_inhibited = raw.external_inhibited || raw.on_battery;
        if should_hold_idle_presentation(cooldown_active) {
            effective_inhibited = true;
        }

        Ok(SituationAssessment {
            system_idle: raw.system_idle,
            session_locked: raw.session_locked,
            effective_inhibited,
            cooldown_active,
            manual_activate,
            config: raw.config,
        })
    }
}
