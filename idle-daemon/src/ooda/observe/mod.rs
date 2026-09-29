// SPDX-License-Identifier: MIT

//! openOODA Pillar 1: Observe (Sensors & Input Ingestion)

use std::sync::Arc;
use std::sync::atomic::Ordering;

use idle_api::IdleSource;

use crate::config::DaemonConfig;
use crate::controller::{DaemonCommand, DaemonController};
use crate::daemon::battery::is_on_battery;

/// Raw snapshot of environmental sensors collected during Phase 1.
#[derive(Debug, Clone)]
pub struct RawObservation {
    pub system_idle: bool,
    pub session_locked: bool,
    pub external_inhibited: bool,
    pub on_battery: bool,
    pub commands: Vec<DaemonCommand>,
    pub config: DaemonConfig,
}

#[derive(Default)]
pub struct OodaObserver;

impl OodaObserver {
    pub fn new() -> Self {
        Self
    }

    /// Sample all active environmental sensors into a `RawObservation`.
    pub fn observe(
        &mut self,
        controller: &Arc<DaemonController>,
        idle_monitor: &mut Box<dyn IdleSource>,
    ) -> RawObservation {
        let system_idle = idle_monitor.is_idle();
        let session_locked = controller.session_locked.load(Ordering::Relaxed);
        let external_inhibited = controller.inhibitors.is_inhibited();
        let on_battery = is_on_battery();
        let commands = controller.drain_commands();
        let config = controller
            .config
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .clone();

        RawObservation {
            system_idle,
            session_locked,
            external_inhibited,
            on_battery,
            commands,
            config,
        }
    }
}
