// SPDX-License-Identifier: MIT

//! openOODA loop coordinator for `idle-daemon`.
//!
//! Orchestrates the 5 phases of the openOODA control cycle on every tick:
//! Observe -> Orient -> Decide -> Act -> State sync.

pub mod act;
pub mod decide;
pub mod observe;
pub mod orient;
pub mod state;

use std::sync::Arc;
use std::time::{Duration, Instant};

use idle_api::{IdleSource, OverlaySurface};

use self::act::OodaActor;
use self::decide::OodaDecisionEngine;
use self::observe::OodaObserver;
use self::orient::OodaOrientator;
use self::state::OodaStateManager;
use crate::controller::DaemonController;
use crate::daemon::presentation::{ActivePresentation, stop_presentation};

/// State persistent across tick iterations of the openOODA loop.
pub struct OodaLoopController {
    pub observer: OodaObserver,
    pub orientator: OodaOrientator,
    pub decision_engine: OodaDecisionEngine,
    pub actor: OodaActor,
    pub state_manager: OodaStateManager,
    pub presentation: ActivePresentation,
    pub preview_name: Option<String>,
    pub current_saver: String,
    pub tick_counter: u32,
    pub consecutive_faults: u32,
    pub present_cooldown_until: Option<Instant>,
}

impl Default for OodaLoopController {
    fn default() -> Self {
        Self::new()
    }
}

impl OodaLoopController {
    pub fn new() -> Self {
        Self {
            observer: OodaObserver::new(),
            orientator: OodaOrientator::new(),
            decision_engine: OodaDecisionEngine::new(),
            actor: OodaActor::new(),
            state_manager: OodaStateManager::new(),
            presentation: ActivePresentation::None,
            preview_name: None,
            current_saver: String::new(),
            tick_counter: 0,
            consecutive_faults: 0,
            present_cooldown_until: None,
        }
    }

    /// Execute one complete openOODA cycle (Observe -> Orient -> Decide -> Act -> State).
    pub fn step_tick(
        &mut self,
        controller: &Arc<DaemonController>,
        idle_monitor: &mut Box<dyn IdleSource>,
        overlay_presenter: &mut Arc<dyn OverlaySurface>,
    ) -> idle_err::Result<()> {
        self.tick_counter = self.tick_counter.saturating_add(1);
        self.presentation
            .check_liveness(&mut self.preview_name, &mut self.current_saver);

        // 1. OBSERVE: Sample raw environmental sensors & command channels
        let raw_obs = self.observer.observe(controller, idle_monitor);

        // 2. ORIENT: Assess situation, check Wayland runtime health & fault backoff
        let situation = match self.orientator.orient(
            raw_obs,
            controller,
            idle_monitor,
            overlay_presenter,
            &mut self.presentation,
            &mut self.preview_name,
            &mut self.current_saver,
            &mut self.consecutive_faults,
            &mut self.present_cooldown_until,
        ) {
            Ok(s) => s,
            Err(_fault_handled) => {
                // Subsystem fault recovered; skip decision/action phases for this tick.
                return Ok(());
            }
        };

        // Handle dynamic config reload interval if due
        if let Some(timeout) = controller.reload_config_if_due(self.tick_counter) {
            idle_monitor
                .as_mut()
                .set_timeout(Duration::from_secs(timeout.saturating_mul(60) as u64));
        }

        // 3. DECIDE: Evaluate pure policy matrix to determine presentation target
        let decision = self.decision_engine.decide(
            &situation,
            &self.presentation,
            overlay_presenter.as_ref(),
            self.preview_name.as_deref(),
            &self.current_saver,
        );

        // 4. ACT: Execute side-effect presentation actions & process pending commands
        if overlay_presenter.is_alive() {
            self.actor.execute(
                decision,
                overlay_presenter,
                &mut self.presentation,
                &mut self.preview_name,
                &mut self.current_saver,
                &situation.config,
                situation.system_idle,
                situation.session_locked,
                situation.effective_inhibited,
            );
        }

        // 5. STATE: Synchronize canonical DaemonStatus & publish D-Bus contract
        self.state_manager.sync_state(
            controller,
            situation.system_idle,
            self.presentation.is_active(),
            self.preview_name.is_some(),
            &self.current_saver,
            situation.effective_inhibited,
        );

        Ok(())
    }

    /// Shutdown cleanup.
    pub fn shutdown(&mut self, overlay_presenter: &Arc<dyn OverlaySurface>) {
        stop_presentation(Some(overlay_presenter), &mut self.presentation);
    }
}
