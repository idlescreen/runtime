// SPDX-License-Identifier: MIT

use super::manifest_gate;
use super::{PluginGuard, PluginSession};
use crate::launcher::PluginError;

use crate::dylib::Library;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl PluginSession {
    pub fn reload(&mut self) -> Result<(), PluginError> {
        idle_log::info!("Reloading plugin from {:?}", self.plugin_path);

        if !self.plugin_path.exists() {
            return Err(PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "plugin file not found",
            )));
        }

        let (was_active, was_focused) = if let Some(ref mut old_plugin) = self.plugin {
            let old_saver = old_plugin.saver_mut();
            (old_saver.active(), old_saver.focused())
        } else {
            (true, true)
        };

        self.plugin = None;

        // Re-read the manifest: the file on disk changed, so the capability
        // claims we admitted the old library under may no longer hold. Falling
        // back to the cached manifest would let a swapped .so inherit trust.
        let manifest = manifest_gate::load_manifest_for(&self.plugin_path)?;

        // Re-assert sandbox allow for this path before constructors run.
        match manifest.as_deref() {
            Some(m) => {
                crate::sandbox::enforce_sandbox_for_plugin_with_manifest(&self.plugin_path, m)
            }
            None => crate::sandbox::enforce_sandbox_for_plugin(&self.plugin_path),
        }
        .map_err(PluginError::Sandbox)?;

        let mut new_guard = unsafe {
            let lib = Library::new(&self.plugin_path)?;

            if let Some(m) = manifest.as_deref() {
                super::entry::check_entry(m, &self.plugin_path)?;
            }

            let (raw_ptr, destroy) = super::entry::resolve_entry(&lib)?;

            PluginGuard {
                ptr: raw_ptr,
                destroy,
                _lib: lib,
            }
        };

        {
            let new_saver = new_guard.saver_mut();
            new_saver.set_active(was_active);
            new_saver.set_focused(was_focused);
            if self.simulation_cols > 0 && self.simulation_rows > 0 {
                new_saver.init(self.simulation_cols, self.simulation_rows);
            }
        }

        self.plugin = Some(new_guard);
        self.manifest = manifest;
        idle_log::info!("Plugin successfully reloaded and state restored.");
        Ok(())
    }

    pub fn start_watcher(&mut self) -> Result<(), PluginError> {
        let needs_reload = self.needs_reload.clone();
        let target_filename = self
            .plugin_path
            .file_name()
            .ok_or_else(|| {
                PluginError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid plugin path",
                ))
            })?
            .to_os_string();

        // DirWatcher reports create/modify/move events by file name; a None
        // name means a self-event — treat it as a change too (fail-reload is
        // safer than silently missing a plugin swap).
        let callback = move |name: Option<&std::ffi::OsStr>| {
            let matches = name.is_none_or(|n| n == target_filename);
            if matches {
                idle_log::info!("Watcher detected modification for {:?}", target_filename);
                needs_reload.store(true, Ordering::Relaxed);
            }
        };

        let parent = self
            .plugin_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let watcher = crate::filewatch::DirWatcher::watch(&parent, callback)
            .map_err(|e| PluginError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        self.watcher = Some(watcher);
        idle_log::info!(
            "Started file watcher on {:?}",
            self.plugin_path.parent().unwrap_or(&self.plugin_path)
        );
        Ok(())
    }

    pub fn poll_reload(&mut self) -> Result<bool, PluginError> {
        if self.needs_reload.load(Ordering::Relaxed) {
            self.needs_reload.store(false, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(100));
            self.reload()?;
            return Ok(true);
        }
        Ok(false)
    }
}
