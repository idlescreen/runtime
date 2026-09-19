// SPDX-License-Identifier: MIT

//! Entry-point resolution: manifest entry validation, ABI-version gating
//! symbol lookup, and the C-ABI vs legacy-Rust dispatch.

use std::path::Path;

use idle_api::ScreensaverInstance;

use crate::dylib::Library;
use idle_api::plugin_manifest::Manifest;

use crate::launcher::PluginError;

pub(crate) fn check_entry(manifest: &Manifest, resolved: &Path) -> Result<(), PluginError> {
    if !manifest.is_native() {
        return Err(PluginError::ManifestUnsupported(
            "wasm runtime not built in this build".to_string(),
        ));
    }
    if !manifest.library_matches(resolved) {
        return Err(PluginError::ManifestUnsupported(format!(
            "manifest entry.library '{}' does not match resolved library '{}'",
            manifest.entry.library,
            resolved.display()
        )));
    }
    Ok(())
}

/// Host-side destroy for C-ABI plugins: drops the boxed `CAbiSaver`, whose
/// `Drop` calls `ops.destroy(ctx)` to free plugin state.
unsafe extern "C" fn drop_c_abi_instance(ptr: *mut ScreensaverInstance) {
    drop(unsafe { Box::from_raw(ptr) });
}

/// Resolve the plugin's entry surface. Foreign-language plugins (C, Zig,
/// openOODA…) export `idle_saver_ops` returning a static [`IdleSaverOps`]
/// table; Rust plugins keep the legacy `create_screensaver`/`destroy_screensaver`
/// pair returning `Box<dyn Screensaver>`. The ops path wins when both exist.
///
/// # Safety
/// `lib` must remain loaded until the returned pointer is destroyed.
pub(crate) unsafe fn resolve_entry(
    lib: &Library,
) -> Result<
    (
        *mut ScreensaverInstance,
        unsafe extern "C" fn(*mut ScreensaverInstance),
    ),
    PluginError,
> {
    if let Ok(ops_fn) = unsafe {
        lib.get::<unsafe extern "C" fn() -> *const idle_api::IdleSaverOps>(idle_api::OPS_SYMBOL)
    } {
        let ops_ptr = unsafe { ops_fn() };
        if ops_ptr.is_null() {
            return Err(PluginError::SymbolMissing("idle_saver_ops (null)"));
        }
        let ops = unsafe { &*ops_ptr };
        if ops.abi_version != idle_api::API_VERSION {
            return Err(PluginError::ApiVersionMismatch {
                found: ops.abi_version,
                expected: idle_api::API_VERSION,
            });
        }
        let saver = unsafe { idle_api::CAbiSaver::new(ops) }
            .ok_or(PluginError::SymbolMissing("ops.create (null ctx)"))?;
        let instance = Box::into_raw(Box::new(ScreensaverInstance {
            inner: Box::new(saver),
        }));
        idle_log::info!("plugin uses C-ABI ops table (idle_saver_ops)");
        return Ok((instance, drop_c_abi_instance));
    }

    let create_fn: unsafe extern "C" fn() -> *mut ScreensaverInstance =
        unsafe { lib.get(b"create_screensaver") }
            .map_err(|_| PluginError::SymbolMissing("create_screensaver"))?;
    let destroy_fn: unsafe extern "C" fn(*mut ScreensaverInstance) =
        unsafe { lib.get(b"destroy_screensaver") }
            .map_err(|_| PluginError::SymbolMissing("destroy_screensaver"))?;

    let raw_ptr = unsafe { create_fn() };
    if raw_ptr.is_null() {
        return Err(PluginError::SymbolMissing("create_screensaver (null)"));
    }
    Ok((raw_ptr, destroy_fn))
}
