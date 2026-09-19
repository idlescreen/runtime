// SPDX-License-Identifier: MIT

//! Plugin manifest signature verification (Sprint 04 trust-surface).
//!
//! Operators can opt into a signed-manifest regime via
//! `IDLE_REQUIRE_MANIFEST_SIGNATURE=1`. When set, the loader refuses any
//! `.idleplugin.toml` that lacks an adjacent `.sig` detached signature or
//! whose signature fails to verify against a configured keyring.
//!
//! Default behaviour (env unset): no signature is required; existing
//! unsigned manifests continue to load. This is the fail-OPEN default for
//! rollout safety. Operators who enable the env var get the fail-CLOSED
//! regime: any unsigned manifest is `PluginError::SignatureMissing`.
//!
//! Verification is "best effort" via the system `gpg` CLI: the plugin keyring
//! lives in `~/.config/idle/trusted-keys.d/`. No GPG state is mutated.
//!
//! Privacy posture (DESIGN §"Privacy posture"): verification is local; no
//! network is contacted. The signature file is read once at load time.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::ManifestError;

/// Default location of the trusted-key ring.
pub const TRUSTED_KEYS_DIR: &str = ".config/idle/trusted-keys.d";

/// Detached signature path (companion to `.idleplugin.toml`).
pub fn signature_path(manifest_path: &Path) -> PathBuf {
    let mut p = manifest_path.to_path_buf();
    let name = manifest_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("manifest");
    p.set_file_name(format!("{name}.sig"));
    p
}

/// True when the operator has required signature verification.
pub fn signature_required() -> bool {
    std::env::var_os("IDLE_REQUIRE_MANIFEST_SIGNATURE").is_some()
}

/// Verify `manifest_path`'s companion `.sig` against the trusted keyring.
///
/// Returns `Ok(())` when the signature is valid (or when verification is not
/// required and no signature is present). Returns `Err(Missing)` when
/// verification is required and no signature file exists. Returns
/// `Err(Invalid)` when the signature file exists but `gpg --verify` fails
/// (bad sig, missing key, modified manifest).
pub fn verify_signature(manifest_path: &Path) -> Result<(), ManifestError> {
    let sig = signature_path(manifest_path);
    if !sig.exists() {
        if signature_required() {
            return Err(ManifestError::SignatureMissing(sig.display().to_string()));
        }
        return Ok(());
    }

    // Fail-closed: signature file exists but no opt-in means we are still
    // operating under the permissive default; log a warning so operators
    // notice they're shipping signed manifests without enforcement.
    if !signature_required() {
        idle_log::warn!(
            manifest = %manifest_path.display(),
            signature = %sig.display(),
            "manifest signature present but verification disabled (set IDLE_REQUIRE_MANIFEST_SIGNATURE=1)"
        );
        return Ok(());
    }

    let keyring = trusted_keyring();
    if !keyring.exists() {
        return Err(ManifestError::SignatureInvalid(
            "trusted keyring missing; create ~/.config/idle/trusted-keys.d/ with at least one public key".into(),
        ));
    }

    let status = Command::new("gpg")
        .arg("--no-default-keyring")
        .arg("--keyring")
        .arg(&keyring)
        .arg("--verify")
        .arg(&sig)
        .arg(manifest_path)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(ManifestError::SignatureInvalid(format!(
            "gpg --verify exit {} for {}",
            s.code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".into()),
            sig.display()
        ))),
        Err(e) => Err(ManifestError::SignatureInvalid(format!(
            "gpg not available: {e}"
        ))),
    }
}

/// Path to the trusted-key ring directory. Honours `IDLE_TRUSTED_KEYS_DIR`.
fn trusted_keyring() -> PathBuf {
    if let Some(p) = std::env::var_os("IDLE_TRUSTED_KEYS_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    home.join(TRUSTED_KEYS_DIR)
}

#[cfg(test)]
#[path = "signature_tests.rs"]
mod tests;
