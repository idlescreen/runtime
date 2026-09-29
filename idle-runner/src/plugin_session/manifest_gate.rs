// SPDX-License-Identifier: MIT

//! Manifest gate: read, validate, and capability-check the `.idleplugin.toml`
//! sibling of a plugin library. Fail-closed by design — every variant of
//! `ManifestError` or `PluginError::CapabilityMismatch` is a hard refusal.

use crate::launcher::PluginError;
use idle_api::plugin_manifest::{self, Manifest, host};
use std::path::Path;
use std::sync::Arc;

/// Read, validate and capability-check the manifest beside `path`.
///
/// Returns `Ok(None)` only for the operator-gated unsigned escape hatch; a
/// missing manifest is otherwise a hard refusal.
pub(crate) fn load_manifest_for(path: &Path) -> Result<Option<Arc<Manifest>>, PluginError> {
    let manifest = match plugin_manifest::load_for(path) {
        Ok(m) => m,
        Err(plugin_manifest::ManifestError::Missing(missing)) => {
            if host::unsigned_plugins_allowed() {
                idle_log::warn!(
                    plugin = %path.display(),
                    manifest = %missing.display(),
                    result = "unsigned_accepted",
                    "loading plugin WITHOUT a manifest ({}=1); capabilities unverified",
                    host::ALLOW_UNSIGNED_ENV
                );
                return Ok(None);
            }
            return Err(PluginError::ManifestMissing(missing.display().to_string()));
        }
        Err(other) => return Err(other.into()),
    };

    plugin_manifest::validate(&manifest)?;
    plugin_manifest::signature::verify_signature(path)?;
    check_capabilities(&manifest)?;
    Ok(Some(Arc::new(manifest)))
}

/// Refuse capabilities the host cannot mediate at the OS level yet.
///
/// Sprint 03 splits the policy per-capability: `network` is refused under
/// `sandbox.profile = "minimal"` regardless of opt-in; each remaining ambient
/// capability requires its own env knob. Anything the policy permits is logged
/// as a warning so the install-audit log records the widening.
pub(crate) fn check_capabilities(manifest: &Manifest) -> Result<(), PluginError> {
    let decision = host::evaluate_capability_policy(manifest);
    if decision.permitted.is_empty() && decision.refused.is_empty() {
        return Ok(());
    }
    if !decision.permitted.is_empty() {
        idle_log::warn!(
            plugin_id = %manifest.plugin_id,
            capabilities = %decision.permitted.join(", "),
            "plugin capabilities admitted via env opt-in"
        );
    }
    if !decision.refused.is_empty() {
        idle_log::error!(
            plugin_id = %manifest.plugin_id,
            refused = %decision.refused.join(", "),
            "refusing plugin: declares capabilities the host cannot enforce"
        );
        return Err(PluginError::CapabilityMismatch(format!(
            "plugin '{}' declares refused capabilities [{}]",
            manifest.plugin_id,
            decision.refused.join(", ")
        )));
    }
    Ok(())
}
