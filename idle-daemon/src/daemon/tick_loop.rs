// SPDX-License-Identifier: MIT

//! Main idle-detection tick loop delegates to the openOODA coordinator.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::controller::{DaemonController, MAIN_LOOP_INTERVAL};
use crate::daemon::watchdog;
use crate::ooda::OodaLoopController;

pub fn tick_loop_until_shutdown(controller: Arc<DaemonController>) -> idle_err::Result<()> {
    let (mut idle_monitor, mut overlay_presenter) =
        super::runtime::initialize_runtime(&controller)?;

    let mut ooda_loop = OodaLoopController::new();
    let watchdog = controller.watchdog.clone();
    // The controller builds the watchdog at startup; re-baseline here so
    // runtime init time doesn't read as a stall on the first check.
    watchdog.heartbeat();
    // Watchdog raises `controller.shutdown` on stall — the loop exits and
    // the process supervisor (systemd) restarts us.
    let _monitor = watchdog::spawn_monitor(
        watchdog.clone(),
        watchdog::configured_timeout_ms(),
        controller.shutdown.clone(),
        controller.watchdog_stalled.clone(),
        std::thread::current(),
    );

    while !controller.shutdown.load(Ordering::Relaxed) {
        std::thread::sleep(MAIN_LOOP_INTERVAL);

        if let Err(err) =
            ooda_loop.step_tick(&controller, &mut idle_monitor, &mut overlay_presenter)
        {
            idle_log::error!("error in openOODA tick cycle: {err:#}");
        }

        watchdog.heartbeat();
    }

    ooda_loop.shutdown(&overlay_presenter);
    if controller.watchdog_stalled.load(Ordering::Relaxed) {
        idle_err::bail!("render loop watchdog stall — exiting non-zero for systemd restart");
    }
    Ok(())
}
