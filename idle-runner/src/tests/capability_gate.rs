// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Sprint 03 capability-policy adversarial tests.
//!
//! Each test mutates process-global env vars under `ENV_LOCK` so they cannot
//! race. Every variant asserts the loader fails *closed* on the capability the
//! manifest claims, or admits it only under the documented opt-in.

use crate::launcher::PluginError;
use crate::plugin_session::manifest_gate::check_capabilities;
use idle_api::plugin_manifest::Manifest;
use std::path::PathBuf;

/// Serialises tests that mutate process-global env vars.
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

fn parse(text: &str) -> Manifest {
    idle_api::plugin_manifest::parse_str(text, &PathBuf::from("test.toml")).unwrap()
}

#[test]
fn audio_capture_refused_without_opt_in() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_PERMIT_AUDIO_CAPTURE") };
    let m = parse(&BASE.replace("audio_capture    = false", "audio_capture    = true"));
    let err = check_capabilities(&m).unwrap_err();
    assert!(
        matches!(err, PluginError::CapabilityMismatch(ref s) if s.contains("audio_capture")),
        "audio_capture must be refused without the opt-in, got {err:?}"
    );
}

#[test]
fn audio_capture_admitted_with_opt_in() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_PERMIT_AUDIO_CAPTURE", "1") };
    let m = parse(&BASE.replace("audio_capture    = false", "audio_capture    = true"));
    let result = check_capabilities(&m);
    unsafe { std::env::remove_var("IDLE_PERMIT_AUDIO_CAPTURE") };
    assert!(
        result.is_ok(),
        "audio_capture must load under IDLE_PERMIT_AUDIO_CAPTURE=1, got {result:?}"
    );
}

#[test]
fn audio_output_refused_without_opt_in() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_PERMIT_AUDIO_OUTPUT") };
    let m = parse(&BASE.replace("audio_output     = false", "audio_output     = true"));
    let err = check_capabilities(&m).unwrap_err();
    assert!(
        matches!(err, PluginError::CapabilityMismatch(ref s) if s.contains("audio_output")),
        "audio_output must be refused without the opt-in, got {err:?}"
    );
}

#[test]
fn network_refused_under_minimal_even_with_opt_in() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_PERMIT_NETWORK_PLUGINS", "1") };
    // BASE declares sandbox.profile = "minimal"
    let m = parse(&BASE.replace("network          = false", "network          = true"));
    let err = check_capabilities(&m).unwrap_err();
    unsafe { std::env::remove_var("IDLE_PERMIT_NETWORK_PLUGINS") };
    assert!(
        matches!(err, PluginError::CapabilityMismatch(ref s) if s.contains("minimal")),
        "minimal profile must refuse network even with PERMIT_NETWORK=1, got {err:?}"
    );
}

#[test]
fn network_admitted_under_renderer_with_opt_in() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_PERMIT_NETWORK_PLUGINS", "1") };
    let text = BASE
        .replace("network          = false", "network          = true")
        .replace(r#"profile = "minimal""#, r#"profile = "renderer""#);
    let m = parse(&text);
    let result = check_capabilities(&m);
    unsafe { std::env::remove_var("IDLE_PERMIT_NETWORK_PLUGINS") };
    assert!(
        result.is_ok(),
        "renderer profile must admit network under PERMIT_NETWORK=1, got {result:?}"
    );
}

#[test]
fn filesystem_read_rejects_relative_path() {
    let text = BASE.replace(
        "filesystem_read  = []",
        r#"filesystem_read  = ["relative/path"]"#,
    );
    let m = parse(&text);
    let err = idle_api::plugin_manifest::validate(&m).unwrap_err();
    assert!(
        matches!(err, idle_api::plugin_manifest::ManifestError::Invalid(ref m, _) if m.contains("absolute")),
        "relative path must fail validation, got {err:?}"
    );
}

#[test]
fn filesystem_read_rejects_parent_traversal() {
    let text = BASE.replace(
        "filesystem_read  = []",
        r#"filesystem_read  = ["/var/data/../etc/passwd"]"#,
    );
    let m = parse(&text);
    let err = idle_api::plugin_manifest::validate(&m).unwrap_err();
    assert!(
        matches!(err, idle_api::plugin_manifest::ManifestError::Invalid(ref m, _) if m.contains("..")),
        "parent-dir traversal must fail validation, got {err:?}"
    );
}

#[test]
fn filesystem_read_rejects_empty_path() {
    let text = BASE.replace("filesystem_read  = []", r#"filesystem_read  = [""]"#);
    let m = parse(&text);
    let err = idle_api::plugin_manifest::validate(&m).unwrap_err();
    assert!(
        matches!(err, idle_api::plugin_manifest::ManifestError::Invalid(ref m, _) if m.contains("empty")),
        "empty path must fail validation, got {err:?}"
    );
}

#[test]
fn filesystem_read_accepts_absolute_path() {
    let text = BASE.replace(
        "filesystem_read  = []",
        r#"filesystem_read  = ["/var/lib/idle/data"]"#,
    );
    let m = parse(&text);
    assert!(
        idle_api::plugin_manifest::validate(&m).is_ok(),
        "absolute path must pass validation"
    );
}

#[test]
fn filesystem_write_rejects_relative_path() {
    let text = BASE.replace("filesystem_write = []", r#"filesystem_write = ["./out"]"#);
    let m = parse(&text);
    let err = idle_api::plugin_manifest::validate(&m).unwrap_err();
    assert!(
        matches!(err, idle_api::plugin_manifest::ManifestError::Invalid(ref m, _) if m.contains("absolute")),
        "write-side relative path must fail validation, got {err:?}"
    );
}
