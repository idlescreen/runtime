// SPDX-License-Identifier: MIT

//! Hot-reload `~/.config/idle/config.yaml` (or legacy `trance`) without Tokio.
//!
//! The daemon main path is not Tokio-driven; only the D-Bus thread owns a
//! runtime. The inotify watcher runs on its own OS thread so startup cannot
//! panic with "there is no reactor running".

use std::sync::Arc;
use std::time::Duration;

use crate::config::DaemonConfig;
use crate::controller::DaemonController;

pub fn start_config_watcher(controller: Arc<DaemonController>) {
    let Some(path) = DaemonConfig::resolve_config_path() else {
        return;
    };
    let Some(parent_dir) = path.parent() else {
        return;
    };

    if !parent_dir.exists() {
        let _ = std::fs::create_dir_all(parent_dir);
    }

    let controller_clone = controller.clone();
    let target_name = path.file_name().map(|n| n.to_os_string());

    // Debounce: atomic write is tmp→rename; inotify may fire several events.
    let last_reload = std::sync::Arc::new(std::sync::Mutex::new(
        std::time::Instant::now()
            .checked_sub(Duration::from_secs(10))
            .unwrap_or_else(std::time::Instant::now),
    ));
    let last_reload_cb = last_reload.clone();

    let watcher = match idle_runner::filewatch::DirWatcher::watch(
        parent_dir,
        move |name: Option<&std::ffi::OsStr>| {
            let hits = match (name, &target_name) {
                (Some(n), Some(t)) => n == t,
                (None, _) => true, // self-event/overflow — reload is the safe reaction
                _ => false,
            };
            if !hits {
                return;
            }
            // Ignore rename/write storms within 400ms.
            if let Ok(mut last) = last_reload_cb.lock() {
                if last.elapsed() < Duration::from_millis(400) {
                    return;
                }
                *last = std::time::Instant::now();
            }
            idle_log::info!("Config file modified on disk; hot-reloading settings...");
            // Disk is source of truth: apply under lock and **never** save back
            // (avoids lost-update races with D-Bus mutate_config + self-echo loops).
            if let Err(e) = controller_clone.reload_config_from_disk() {
                idle_log::warn!("config hot-reload failed: {e:#}");
            }
        },
    ) {
        Ok(w) => w,
        Err(e) => {
            idle_log::warn!("Failed to initialize config file watcher: {e}");
            return;
        }
    };

    // The watcher thread dies when the struct is dropped; keep it alive for
    // the process lifetime, same contract the notify watcher had.
    std::mem::forget(watcher);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_config_watcher_does_not_panic_without_tokio() {
        // No Tokio Handle in this thread — must not abort.
        let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
        start_config_watcher(controller);
    }
}
