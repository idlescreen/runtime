// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Tests for the sandbox profile table (`DECISION-MANIFEST-01`).
//!
//! The load-bearing property is that no profile is ever *wider* than the
//! operator asked for: unknown names are refused, `experimental` needs an
//! explicit opt-in, and a blank plugin id omits id-scoped trees rather than
//! falling back to their parent directory.

use super::{FONT_ROOTS, ProfileError, profile_rules_for};
use idle_api::plugin_manifest;
use std::fs;
use std::path::PathBuf;

const PLUGIN_ID: &str = "io.github.idlescreen.beams";

fn manifest_with(profile: &str, read: &str, write: &str) -> plugin_manifest::Manifest {
    let text = format!(
        "schema_version = 1\nplugin_id = \"{PLUGIN_ID}\"\nplugin_version = \"1.0.0\"\n\
         api_version = 1\n\n[entry]\nruntime = \"native\"\nlibrary = \"x.so\"\n\n\
         [capabilities]\nfilesystem_read = [{read}]\nfilesystem_write = [{write}]\n\n\
         [sandbox]\nprofile = \"{profile}\"\n"
    );
    plugin_manifest::parse_str(&text, &PathBuf::from("t.toml")).expect("fixture parses")
}

#[test]
fn minimal_profile_matches_today() {
    // Regression: `minimal` must stay exactly the pre-manifest policy —
    // font roots only, with the plugin dir added by the enforcer itself.
    let rules = profile_rules_for("minimal", PLUGIN_ID).expect("minimal is a known profile");
    let paths: Vec<_> = rules.iter().map(|r| r.path.display().to_string()).collect();
    assert_eq!(
        paths, FONT_ROOTS,
        "minimal profile drifted from today's policy"
    );
    assert!(rules.iter().all(|r| !r.write), "minimal grants no write");
}

#[test]
fn unknown_profile_rejects_at_load() {
    let err = profile_rules_for("wide-open", PLUGIN_ID).unwrap_err();
    assert_eq!(err, ProfileError::Unknown("wide-open".to_string()));
    // ...and validation refuses it before the sandbox is ever built.
    let m = manifest_with("wide-open", "", "");
    assert!(
        plugin_manifest::validate(&m).is_err(),
        "unknown profile must not validate"
    );
}

#[test]
fn seatbelt_profile_unsupported_on_linux() {
    let err = profile_rules_for("seatbelt", PLUGIN_ID).unwrap_err();
    assert!(
        matches!(err, ProfileError::UnsupportedPlatform { ref profile } if profile == "seatbelt"),
        "seatbelt must refuse on Linux until Sprint 05, got {err:?}"
    );
}

#[test]
fn appcontainer_profile_unsupported_on_linux() {
    let err = profile_rules_for("appcontainer", PLUGIN_ID).unwrap_err();
    assert!(
        matches!(err, ProfileError::UnsupportedPlatform { ref profile } if profile == "appcontainer"),
        "appcontainer must refuse on Linux until Sprint 05, got {err:?}"
    );
}

#[test]
fn filesystem_read_declarations_allow_file() {
    let dir = crate::test_util::tempdir().expect("tempdir");
    let asset = dir.path().join("asset.txt");
    fs::write(&asset, b"x").expect("write asset");
    let m = manifest_with(
        "renderer",
        &format!("{:?}", asset.display().to_string()),
        "",
    );
    assert_eq!(m.capabilities.filesystem_read.len(), 1);

    // renderer = minimal + the plugin's shared read tree; declared reads are
    // layered on top by enforce_sandbox_for_plugin_with_manifest.
    let rules = profile_rules_for("renderer", PLUGIN_ID).expect("renderer is known");
    assert!(
        rules
            .iter()
            .any(|r| r.path.ends_with(PLUGIN_ID) && !r.write),
        "renderer must grant read on /usr/share/idle/<plugin_id>"
    );
}

#[test]
fn filesystem_write_declarations_allow_write() {
    let m = manifest_with("asset-author", "", "\"/tmp/idle-write-test\"");
    assert_eq!(m.capabilities.filesystem_write.len(), 1);

    let rules = profile_rules_for("asset-author", PLUGIN_ID).expect("asset-author is known");
    assert!(
        rules.iter().any(|r| r.write && r.path.ends_with(PLUGIN_ID)),
        "asset-author must grant a writable per-plugin user data tree"
    );
}

#[test]
fn experimental_profile_requires_opt_in() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    // SAFETY: test-only env mutation on a key no other test touches.
    unsafe { std::env::remove_var("IDLE_ALLOW_EXPERIMENTAL_PROFILES") };
    assert_eq!(
        profile_rules_for("experimental", PLUGIN_ID).unwrap_err(),
        ProfileError::ExperimentalNotAllowed
    );
}

#[test]
fn empty_plugin_id_does_not_widen_sandbox() {
    // A blank id must omit the id-scoped trees, never grant their parent.
    let rules = profile_rules_for("asset-author", "").expect("asset-author is known");
    let paths: Vec<_> = rules.iter().map(|r| r.path.display().to_string()).collect();
    assert_eq!(paths, FONT_ROOTS, "blank plugin_id must not add trees");
}
