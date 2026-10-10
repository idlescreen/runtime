// SPDX-License-Identifier: MIT

//! Main idle-detection tick loop delegates to the openOODA coordinator.
//!
//! Replaces the legacy `thread::sleep(MAIN_LOOP_INTERVAL)` 250 ms polling
//! loop with an event-driven wait on the [`PowerWatcher`]. Wake sources:
//!
//!   * inotify event on `/sys/class/power_supply` — actual battery / AC
//!     change. We re-read and update the cached state before stepping OODA.
//!   * 1-second heartbeat — keeps the watchdog live when nothing else moves.
//!   * `controller.shutdown` — prompt exit.
//!
//! Idle CPU on the main tick drops from ~4 wakeups/sec + 8–16 fs syscalls/sec
//! to 1 wakeup/sec + 0 fs syscalls (inotify reads only fire on real events
//! on a background thread).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::controller::DaemonController;
use crate::daemon::power::upower::spawn_power_watcher;
use crate::daemon::power_watcher::{HEARTBEAT, PowerWatcher, WaitOutcome};
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

    super::notify::notify_ready();

    // PowerWatcher: spawned before the loop so the first OODA tick reads
    // the cached value populated by the watcher's startup sample. If
    // both UPower and inotify fail, fall back to the prior 250 ms sleep.
    let (power, _watcher_backend): (PowerWatcher, _) = match spawn_power_watcher() {
        Ok(backend) => (backend.handle(), Some(backend)),
        Err(err) => {
            idle_log::warn!(
                "power_watcher: falling back to polling battery state ({err}); \
                 idle CPU on the main tick will be higher"
            );
            return tick_loop_polling_fallback(
                controller,
                idle_monitor,
                overlay_presenter,
                ooda_loop,
                watchdog.clone(),
            );
        }
    };

    // Generation counter to detect spurious condvar wakes (very rare with
    // parking_lot, but possible if Notify_All races). Recheck on mismatch.
    let last_seen_generation = AtomicU64::new(power.notify_generation());

    let mut last_clock_sample = sample_clocks();

    while !controller.shutdown.load(Ordering::Relaxed) {
        // Wait up to HEARTBEAT for an event (Notified) or to time out
        // (Heartbeat). Stopped returns immediately if `controller.shutdown`
        // flipped before/while we were waiting.
        let outcome = power.wait_for_heartbeat(&controller.shutdown, HEARTBEAT);
        match outcome {
            WaitOutcome::Stopped => break,
            WaitOutcome::Notified => {
                // Just-resumed because the supply dir changed. The cached
                // battery snapshot inside `power` was updated by the
                // watcher thread; OODA tick reads it via
                // `power.cached_is_on_battery()` (used by
                // `ooda::observe::observe` via `is_on_battery()` — see
                // the cache gluing in `daemon/battery.rs`).
                let now = power.notify_generation();
                last_seen_generation.store(now, Ordering::Relaxed);
            }
            WaitOutcome::Heartbeat => {
                // Pure keepalive — the watchog heartbeat function is the
                // only thing that genuinely needs to fire on this beat.
            }
        }

        check_clock_divergence(&mut last_clock_sample, &watchdog);

        if let Err(err) =
            ooda_loop.step_tick(&controller, &mut idle_monitor, &mut overlay_presenter)
        {
            idle_log::error!("error in openOODA tick cycle: {err:#}");
        }

        // `watchdog.heartbeat()` runs on every OODA step (notified OR
        // heartbeat). Keeps the supervisor happy.
        watchdog.heartbeat();
        super::notify::notify_watchdog();
    }

    ooda_loop.shutdown(&overlay_presenter);
    super::notify::notify_stopping();
    if controller.watchdog_stalled.load(Ordering::Relaxed) {
        idle_err::bail!("render loop watchdog stall — exiting non-zero for systemd restart");
    }
    Ok(())
}

/// Pre-power_watcher fallback: 250 ms poll loop. Preserved for platforms
/// where `/sys/class/power_supply` isn't observable (sandbox / chroot).
/// Same logical flow; only the wake source changes.
fn tick_loop_polling_fallback(
    controller: Arc<DaemonController>,
    mut idle_monitor: Box<dyn idle_api::IdleSource>,
    mut overlay_presenter: Arc<dyn idle_api::OverlaySurface>,
    mut ooda_loop: OodaLoopController,
    watchdog: watchdog::Watchdog,
) -> idle_err::Result<()> {
    use crate::controller::MAIN_LOOP_INTERVAL;
    super::notify::notify_ready();
    let mut last_clock_sample = sample_clocks();
    while !controller
        .shutdown
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        std::thread::sleep(MAIN_LOOP_INTERVAL);
        check_clock_divergence(&mut last_clock_sample, &watchdog);

        if let Err(err) =
            ooda_loop.step_tick(&controller, &mut idle_monitor, &mut overlay_presenter)
        {
            idle_log::error!("error in openOODA tick cycle: {err:#}");
        }

        watchdog.heartbeat();
        super::notify::notify_watchdog();
    }
    ooda_loop.shutdown(&overlay_presenter);
    super::notify::notify_stopping();
    if controller
        .watchdog_stalled
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        idle_err::bail!("render loop watchdog stall — exiting non-zero for systemd restart");
    }
    Ok(())
}

fn check_clock_divergence(last_sample: &mut Option<ClockSample>, watchdog: &watchdog::Watchdog) {
    let current_sample = sample_clocks();
    if let (Some(prev), Some(curr)) = (*last_sample, current_sample) {
        let delta_boot = curr.boottime_ns.saturating_sub(prev.boottime_ns);
        let delta_mono = curr.monotonic_ns.saturating_sub(prev.monotonic_ns);
        let divergence_ns = delta_boot.saturating_sub(delta_mono);
        if divergence_ns >= 1_000_000_000 {
            let div_ms = divergence_ns / 1_000_000;
            idle_log::info!(
                divergence_ms = div_ms,
                "suspend/resume detected via clock divergence — resetting watchdog"
            );
            watchdog.heartbeat();
            super::notify::notify_watchdog();
        }
    }
    *last_sample = current_sample;
}

#[derive(Debug, Clone, Copy)]
struct ClockSample {
    boottime_ns: u64,
    monotonic_ns: u64,
}

fn sample_clocks() -> Option<ClockSample> {
    let mut ts_boot = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let mut ts_mono = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: pointers are to valid stack-allocated timespec structs.
    let res_boot = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts_boot) };
    let res_mono = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts_mono) };
    if res_boot == 0 && res_mono == 0 {
        let boottime_ns = (ts_boot.tv_sec as u64) * 1_000_000_000 + (ts_boot.tv_nsec as u64);
        let monotonic_ns = (ts_mono.tv_sec as u64) * 1_000_000_000 + (ts_mono.tv_nsec as u64);
        Some(ClockSample {
            boottime_ns,
            monotonic_ns,
        })
    } else {
        None
    }
}
