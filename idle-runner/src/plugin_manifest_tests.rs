// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for the `.idleplugin.toml` manifest gate.
//!
//! Every rejection path must fail *closed*: a plugin whose manifest is
//! missing, stale, mismatched or over-reaching must not load.

use crate::launcher::PluginError;
use crate::plugin_session::entry::check_entry;
use crate::plugin_session::manifest_gate::{check_capabilities, load_manifest_for};
use idle_api::plugin_manifest::{self, Manifest};
use std::fs;
use std::path::PathBuf;

use crate::ENV_LOCK;

const BASE: &str = r#"schema_version = 1
plugin_id      = "io.github.idlescreen.beams"
plugin_version = "2.0.3"
api_version    = 1

[entry]
runtime = "native"
library = "libscreensaver_beams.so"

[capabilities]
network          = false
audio_capture    = false
audio_output     = false
filesystem_read  = []
filesystem_write = []

[sandbox]
profile = "minimal"

[dependencies]
native = ["libc6"]
wasm   = []

[headless_render]
default_fps        = 60
deterministic_seed = true
gpu_optional       = true
"#;

/// Stage `libscreensaver_beams.so` plus an optional sibling manifest.
fn staged(manifest: Option<&str>) -> (crate::test_util::TmpDir, PathBuf) {
    let dir = crate::test_util::tempdir().unwrap();
    let so = dir.path().join("libscreensaver_beams.so");
    fs::write(&so, b"not-an-elf").unwrap();
    if let Some(text) = manifest {
        fs::write(plugin_manifest::sibling_path(&so), text).unwrap();
    }
    (dir, so)
}

fn parse(text: &str) -> Manifest {
    plugin_manifest::parse_str(text, &PathBuf::from("test.toml")).unwrap()
}

#[test]
fn manifest_missing_fails_closed() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_ALLOW_UNSIGNED_PLUGINS") };
    let (_d, so) = staged(None);
    assert!(matches!(
        load_manifest_for(&so),
        Err(PluginError::ManifestMissing(_))
    ));
}

#[test]
fn manifest_wrong_schema_version_fails_closed() {
    let text = BASE.replace("schema_version = 1", "schema_version = 99");
    let (_d, so) = staged(Some(&text));
    let err = load_manifest_for(&so).unwrap_err();
    assert!(
        matches!(err, PluginError::ManifestUnsupported(ref m) if m.contains("99")),
        "expected schema refusal, got {err:?}"
    );
}

#[test]
fn manifest_unsupported_runtime_fails_closed() {
    let m = parse(&BASE.replace(r#"runtime = "native""#, r#"runtime = "wasm""#));
    let err = check_entry(&m, &PathBuf::from("libscreensaver_beams.so")).unwrap_err();
    assert!(
        matches!(err, PluginError::ManifestUnsupported(ref s) if s.contains("not built")),
        "wasm must be refused, got {err:?}"
    );
}

#[test]
fn manifest_library_mismatch_fails_closed() {
    let m = parse(&BASE.replace("libscreensaver_beams.so", "wrong.so"));
    let err = check_entry(&m, &PathBuf::from("/x/libscreensaver_beams.so")).unwrap_err();
    assert!(matches!(err, PluginError::ManifestUnsupported(_)));
}

#[test]
fn manifest_invalid_plugin_id_fails() {
    let m = parse(&BASE.replace(r#""io.github.idlescreen.beams""#, r#""beams""#));
    assert!(plugin_manifest::validate(&m).is_err(), "bare id must fail");
}

#[test]
fn manifest_round_trip_toml() {
    let first = parse(BASE);
    let emitted = first.to_toml_string();
    assert_eq!(
        first,
        parse(&emitted),
        "parse -> emit -> parse must be lossless"
    );
}

#[test]
fn manifest_load_for_returns_manifest() {
    let (_d, so) = staged(Some(BASE));
    let m = load_manifest_for(&so).unwrap().unwrap();
    assert_eq!(m.plugin_id, "io.github.idlescreen.beams");
    assert_eq!(m.entry.library, "libscreensaver_beams.so");
    assert_eq!(m.sandbox.profile, "minimal");
    assert!(m.is_native() && m.library_matches(&so));
}

#[test]
fn capability_mismatch_rejects_network() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_PERMIT_NETWORK_PLUGINS") };
    let m = parse(&BASE.replace("network          = false", "network          = true"));
    let err = check_capabilities(&m).unwrap_err();
    assert!(
        matches!(err, PluginError::CapabilityMismatch(ref s) if s.contains("network")),
        "network must be refused without the opt-in, got {err:?}"
    );
}

#[test]
fn unsigned_legacy_so_loaded_under_flag() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_d, so) = staged(None);
    unsafe { std::env::set_var("IDLE_ALLOW_UNSIGNED_PLUGINS", "1") };
    let got = load_manifest_for(&so);
    unsafe { std::env::remove_var("IDLE_ALLOW_UNSIGNED_PLUGINS") };
    assert!(
        matches!(got, Ok(None)),
        "flagged bare .so should load with no manifest, got {got:?}"
    );
}

/// Regression test for the wave-3 reviewer finding: `run_plugin_fullscreen`
/// (the entry point used by `idle-daemon run-plugin <saver>`, the TUI preview
/// fallback, and the COSMIC preview fallback) used to call `libloading::Library::new`
/// directly, bypassing the manifest gate that the IPC child path already had.
///
/// After the fix, `run_plugin_fullscreen` is routed through
/// `PluginSession::load_path_with_options`, so the gate is on every code path
/// that resolves a saver binary.
#[test]
fn run_plugin_fullscreen_refuses_bare_so_without_flag() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_ALLOW_UNSIGNED_PLUGINS") };
    let (_d, so) = staged(None);
    let err = crate::idle_runner::run_plugin_fullscreen(so.to_string_lossy().as_ref())
        .expect_err("bare .so must be refused by the manifest gate, not loaded");
    let msg = err.to_string();
    assert!(
        msg.contains("idleplugin.toml") || msg.contains("ManifestMissing"),
        "expected ManifestMissing error from the gate, got: {msg}"
    );
}

/// Companion to `run_plugin_fullscreen_refuses_bare_so_without_flag`: when
/// `IDLE_ALLOW_UNSIGNED_PLUGINS=1` is set, the manifest gate must *pass* (i.e.
/// the error must not be `ManifestMissing`). The staged `.so` is `not-an-elf`
/// so a downstream gate (the `Library::new` step) is expected to reject it,
/// which proves the manifest step let it through.
#[test]
fn run_plugin_fullscreen_passes_gate_under_flag() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_d, so) = staged(None);
    unsafe { std::env::set_var("IDLE_ALLOW_UNSIGNED_PLUGINS", "1") };
    let result = crate::idle_runner::run_plugin_fullscreen(so.to_string_lossy().as_ref());
    unsafe { std::env::remove_var("IDLE_ALLOW_UNSIGNED_PLUGINS") };
    let err = result.expect_err("fake .so must fail somewhere; the point is *where*");
    let msg = err.to_string();
    // B4 — the gate passed; the dlopen() must fail closed at
    // the Library::new step. A regression that flipped the order (e.g.
    // dlopen before the manifest check) would silently accept a bogus
    // binary; a regression that swallowed the dlopen error entirely
    // would crash or hang. We assert the error is *not* a manifest
    // error (gate passed) and *not* a cap-mismatch (caps were checked
    // only after gate).
    assert!(
        !msg.contains("idleplugin.toml"),
        "manifest gate must pass under IDLE_ALLOW_UNSIGNED_PLUGINS=1, got: {msg}"
    );
    assert!(
        !msg.to_lowercase().contains("capability"),
        "capability check must happen after dlopen (which fails first), got: {msg}"
    );
    // The dlopen of a not-an-elf payload yields a libloading error
    // surfaced via `?` as a Box<dyn Error>. libloading's message
    // contains "failed to load" or "is not a valid" depending on
    // platform; the contract is just that an error is returned.
    assert!(
        !msg.is_empty(),
        "B4: dlopen failure must produce a non-empty error message"
    );
}
