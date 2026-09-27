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

/// ABI negotiation: REQUIRED on every path that hands plugin code to the host.
///
/// Every idle-saver-* crate ships an `idle_api_version` symbol; a plugin
/// without it is refused as `MissingVersion` so a stale or hostile plugin
/// cannot slip past. A plugin rebuilt against a different `idle-api` exports a
/// different value and is refused as `ApiVersionMismatch` rather than being
/// driven as a foreign struct layout.
///
/// This lives here rather than inline in `load_path_with_options` because the
/// hot-reload path must run it too: `reload` used to go
/// `Library::new` → `check_entry` → `resolve_entry`, so a `.so` swapped on
/// disk against a different `idle-api` bypassed the gate entirely.
///
/// # Safety
///
/// `lib` must be a live, fully-initialised `Library` for a plugin whose
/// constructors have already run inside the sandbox, and the returned
/// `idle_api_version` symbol must be a valid `extern "C" fn() -> u32` as
/// declared by the plugin. That is the same contract the caller already
/// upholds before `resolve_entry` calls into the plugin.
pub(crate) unsafe fn check_api_version(lib: &Library) -> Result<(), PluginError> {
    let ver_sym = unsafe { lib.get::<unsafe extern "C" fn() -> u32>(b"idle_api_version") };
    let ver_fn = match ver_sym {
        Ok(f) => *f,
        Err(_) => {
            return Err(PluginError::MissingVersion);
        }
    };
    let found = unsafe { ver_fn() };
    let expected = idle_api::API_VERSION;
    if found != expected {
        return Err(PluginError::ApiVersionMismatch { found, expected });
    }
    idle_log::info!(found, expected, "plugin API version ok");
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
        let ops_ptr = unsafe { (*ops_fn)() };
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

    let create_fn = unsafe {
        lib.get::<unsafe extern "C" fn() -> *mut ScreensaverInstance>(b"create_screensaver")
    }
    .map_err(|_| PluginError::SymbolMissing("create_screensaver"))?;
    let destroy_fn = unsafe {
        lib.get::<unsafe extern "C" fn(*mut ScreensaverInstance)>(b"destroy_screensaver")
    }
    .map_err(|_| PluginError::SymbolMissing("destroy_screensaver"))?;

    let raw_ptr = unsafe { (*create_fn)() };
    if raw_ptr.is_null() {
        return Err(PluginError::SymbolMissing("create_screensaver (null)"));
    }
    // `*destroy_fn` copies the fn ptr out of its `Symbol` — the caller's
    // `PluginGuard._lib` keeps the library loaded until after it fires.
    Ok((raw_ptr, *destroy_fn))
}
