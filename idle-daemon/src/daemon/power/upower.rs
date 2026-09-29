// SPDX-License-Identifier: MIT
// Copyright 2026 IdleScreen

//! UPower system D-Bus client for dynamic battery and AC power state detection.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::thread::PowerWatcherThread;
use super::watcher::PowerWatcher;

const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const UPOWER_INTERFACE: &str = "org.freedesktop.UPower";

/// Background worker thread querying UPower on system D-Bus.
pub struct UPowerWatcherThread {
    handle: PowerWatcher,
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl UPowerWatcherThread {
    /// Attempt to spawn the UPower watcher over system D-Bus.
    pub fn spawn() -> Result<Self, String> {
        let conn = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
        let proxy =
            zbus::blocking::Proxy::new(&conn, UPOWER_SERVICE, UPOWER_PATH, UPOWER_INTERFACE)
                .map_err(|e| e.to_string())?;

        let on_battery: bool = proxy.get_property("OnBattery").map_err(|e| e.to_string())?;
        let handle = PowerWatcher::from_initial_state(on_battery);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = stop_flag.clone();
        let thread_handle = handle.clone();

        let thread = std::thread::Builder::new()
            .name("idle-upower-watch".into())
            .spawn(move || run_upower_loop(proxy, thread_handle, thread_stop, on_battery))
            .map_err(|e| e.to_string())?;

        Ok(Self {
            handle,
            stop_flag,
            thread: Some(thread),
        })
    }

    /// Clone the consumer-side watcher handle.
    pub fn handle(&self) -> PowerWatcher {
        self.handle.clone()
    }
}

impl Drop for UPowerWatcherThread {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run_upower_loop(
    proxy: zbus::blocking::Proxy<'static>,
    handle: PowerWatcher,
    stop: Arc<AtomicBool>,
    mut last_state: bool,
) {
    while !stop.load(Ordering::Relaxed) {
        for _ in 0..10 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        match proxy.get_property::<bool>("OnBattery") {
            Ok(current) => {
                if current != last_state {
                    idle_log::info!(
                        from = last_state,
                        to = current,
                        "upower: battery status change detected"
                    );
                    last_state = current;
                    handle.notify_update(current);
                }
            }
            Err(err) => {
                idle_log::warn!(err = %err, "upower: failed to read OnBattery property");
            }
        }
    }
}

/// Unified power watcher backend with graceful fallback.
pub enum PowerWatcherBackend {
    UPower(UPowerWatcherThread),
    Inotify(PowerWatcherThread),
}

impl PowerWatcherBackend {
    pub fn handle(&self) -> PowerWatcher {
        match self {
            Self::UPower(u) => u.handle(),
            Self::Inotify(i) => i.handle(),
        }
    }
}

/// Spawn the power watcher: try UPower system D-Bus first, falling back to inotify.
pub fn spawn_power_watcher() -> Result<PowerWatcherBackend, std::io::Error> {
    match UPowerWatcherThread::spawn() {
        Ok(upower) => {
            idle_log::info!("power_watcher: connected to org.freedesktop.UPower on system D-Bus");
            Ok(PowerWatcherBackend::UPower(upower))
        }
        Err(err) => {
            idle_log::info!(
                err = %err,
                "power_watcher: UPower unavailable; falling back to /sys inotify watcher"
            );
            let inotify = PowerWatcherThread::spawn()?;
            Ok(PowerWatcherBackend::Inotify(inotify))
        }
    }
}
