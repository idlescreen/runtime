// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for the signature verification seam.

use super::*;
use std::fs;
use std::path::{Path, PathBuf};

/// tempfile replacement: unique dir under the system temp dir, removed on Drop.
struct TmpDir(PathBuf);
impl TmpDir {
    fn new() -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "idle-sig-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn make_manifest(dir: &std::path::Path) -> PathBuf {
    let p = dir.join("libscreensaver_test.idleplugin.toml");
    fs::write(
        &p,
        b"schema_version = 1\nplugin_id = \"io.example.test\"\nplugin_version = \"0.0.1\"\napi_version = 1\n",
    )
    .unwrap();
    p
}

#[test]
fn signature_path_is_companion_with_sig_suffix() {
    let dir = TmpDir::new();
    let m = dir.path().join("libscreensaver_beams.idleplugin.toml");
    let s = signature_path(&m);
    assert!(s.ends_with("libscreensaver_beams.idleplugin.toml.sig"));
}

#[test]
fn missing_signature_is_ok_when_not_required() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_REQUIRE_MANIFEST_SIGNATURE") };
    let dir = TmpDir::new();
    let m = make_manifest(dir.path());
    // No .sig file exists. With the env unset, verification is opt-out — Ok.
    assert!(verify_signature(&m).is_ok());
}

#[test]
fn missing_signature_refuses_when_required() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_REQUIRE_MANIFEST_SIGNATURE", "1") };
    let dir = TmpDir::new();
    let m = make_manifest(dir.path());
    let err = verify_signature(&m).unwrap_err();
    unsafe { std::env::remove_var("IDLE_REQUIRE_MANIFEST_SIGNATURE") };
    assert!(
        matches!(err, ManifestError::SignatureMissing(_)),
        "missing signature must refuse when required, got {err:?}"
    );
}

#[test]
fn default_off_with_forged_signature_passes_permissively() {
    // Documents the rollout-safety default: with IDLE_REQUIRE_MANIFEST_SIGNATURE
    // unset, even a garbage sig is accepted. Operators who want fail-closed
    // enforcement MUST opt in. The test pins this contract so a future
    // refactor doesn't accidentally flip the default.
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_REQUIRE_MANIFEST_SIGNATURE") };
    let dir = TmpDir::new();
    let m = make_manifest(dir.path());
    let s = signature_path(&m);
    fs::write(
        &s,
        b"-----BEGIN PGP SIGNATURE-----\ndeadbeef\n-----END PGP SIGNATURE-----\n",
    )
    .unwrap();
    assert!(
        verify_signature(&m).is_ok(),
        "default-off must accept forged sig (rollout safety); \
         set IDLE_REQUIRE_MANIFEST_SIGNATURE=1 to enforce"
    );
    fs::remove_file(&s).ok();
}

#[test]
fn signature_present_without_requirement_warns_but_passes() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_REQUIRE_MANIFEST_SIGNATURE") };
    let dir = TmpDir::new();
    let m = make_manifest(dir.path());
    let s = signature_path(&m);
    // Create a fake sig file. With no requirement, we just log + pass.
    fs::write(&s, b"FAKE-SIG").unwrap();
    assert!(verify_signature(&m).is_ok());
    fs::remove_file(&s).ok();
}

#[test]
fn required_signature_with_missing_keyring_refuses() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_REQUIRE_MANIFEST_SIGNATURE", "1") };
    let dir = TmpDir::new();
    let m = make_manifest(dir.path());
    let s = signature_path(&m);
    fs::write(&s, b"FAKE-SIG").unwrap();
    // Point the keyring at a path that does not exist.
    let fake = dir.path().join("does-not-exist.gpg");
    unsafe { std::env::set_var("IDLE_TRUSTED_KEYS_DIR", &fake) };
    let err = verify_signature(&m).unwrap_err();
    unsafe { std::env::remove_var("IDLE_REQUIRE_MANIFEST_SIGNATURE") };
    unsafe { std::env::remove_var("IDLE_TRUSTED_KEYS_DIR") };
    assert!(
        matches!(err, ManifestError::SignatureInvalid(ref m) if m.contains("keyring")),
        "missing keyring must refuse when required, got {err:?}"
    );
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
