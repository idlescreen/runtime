// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Host-side helpers over a parsed [`Manifest`].
//!
//! Sprint 03 splits the previous "permit ambient capabilities" knob into one
//! predicate per capability, and adds profile-aware policy: the `minimal`
//! profile refuses network regardless of opt-in. Each remaining capability is
//! default-deny and requires an explicit operator env var to admit a plugin.

use super::Manifest;
use std::path::Path;

/// Operator opt-in for plugins declaring the `network` capability.
pub const PERMIT_NETWORK_ENV: &str = "IDLE_PERMIT_NETWORK_PLUGINS";

/// Operator opt-in for plugins declaring the `audio_capture` capability.
pub const PERMIT_AUDIO_CAPTURE_ENV: &str = "IDLE_PERMIT_AUDIO_CAPTURE";

/// Operator opt-in for plugins declaring the `audio_output` capability.
pub const PERMIT_AUDIO_OUTPUT_ENV: &str = "IDLE_PERMIT_AUDIO_OUTPUT";

/// Operator opt-in for loading a bare `.so` with no manifest.
pub const ALLOW_UNSIGNED_ENV: &str = "IDLE_ALLOW_UNSIGNED_PLUGINS";

/// Operator opt-in for the `experimental` sandbox profile.
pub const ALLOW_EXPERIMENTAL_ENV: &str = "IDLE_ALLOW_EXPERIMENTAL_PROFILES";

impl Manifest {
    /// Ambient capabilities that the host cannot mediate at the OS level.
    ///
    /// Returns the names of every such capability the plugin requested, so the
    /// caller can log precisely what was asked for before refusing.
    pub fn ambient_capabilities(&self) -> Vec<&'static str> {
        let c = &self.capabilities;
        [
            ("network", c.network),
            ("audio_capture", c.audio_capture),
            ("audio_output", c.audio_output),
        ]
        .into_iter()
        .filter_map(|(name, requested)| requested.then_some(name))
        .collect()
    }

    /// True when the plugin requests a capability the host cannot enforce.
    pub fn requests_ambient_capabilities(&self) -> bool {
        !self.ambient_capabilities().is_empty()
    }

    /// True when `entry.library` names the library actually resolved on disk.
    ///
    /// Compared by file name: the manifest declares a bare name, while the
    /// resolved path is absolute and may traverse a symlinked install root.
    pub fn library_matches(&self, resolved: &Path) -> bool {
        resolved
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name == self.entry.library)
    }

    /// True when this plugin targets the in-process native runtime.
    pub fn is_native(&self) -> bool {
        self.entry.runtime == "native"
    }
}

/// Result of running per-capability policy against a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDecision {
    /// Capabilities the operator admitted via env opt-in (still logged as a
    /// warning so the install-audit log records the widening).
    pub permitted: Vec<&'static str>,
    /// Capabilities the host refused to admit. Empty means the plugin is
    /// loadable; non-empty means the loader must fail closed.
    pub refused: Vec<&'static str>,
}

impl CapabilityDecision {
    fn empty() -> Self {
        Self {
            permitted: Vec::new(),
            refused: Vec::new(),
        }
    }
    fn permit(mut self, cap: &'static str) -> Self {
        self.permitted.push(cap);
        self
    }
    fn refuse(mut self, cap: &'static str) -> Self {
        self.refused.push(cap);
        self
    }
    /// True when the host must refuse to load the plugin.
    pub fn refused(&self) -> bool {
        !self.refused.is_empty()
    }
}

/// Run the Sprint 03 capability policy against `manifest`.
///
/// Rules:
/// - `network` is refused when `sandbox.profile == "minimal"` (no opt-in can
///   widen the minimal profile). In any other profile it requires
///   `IDLE_PERMIT_NETWORK_PLUGINS=1`.
/// - `audio_capture` / `audio_output` each require their own opt-in env var.
/// - Anything not in `ambient_capabilities()` is out of scope here (filesystem
///   scope-jail lives in the Landlock path rule stage, not here).
pub fn evaluate_capability_policy(manifest: &Manifest) -> CapabilityDecision {
    let mut d = CapabilityDecision::empty();
    let c = &manifest.capabilities;
    let profile = manifest.sandbox.profile.as_str();

    if c.network {
        if profile == "minimal" {
            d = d.refuse("network (refused under minimal profile; pick a wider profile)");
        } else if network_plugins_permitted() {
            d = d.permit("network");
        } else {
            d = d.refuse("network");
        }
    }
    if c.audio_capture {
        if audio_capture_plugins_permitted() {
            d = d.permit("audio_capture");
        } else {
            d = d.refuse("audio_capture");
        }
    }
    if c.audio_output {
        if audio_output_plugins_permitted() {
            d = d.permit("audio_output");
        } else {
            d = d.refuse("audio_output");
        }
    }
    d
}

/// Whether the operator has opted into network-capable plugins.
pub fn network_plugins_permitted() -> bool {
    crate::env_truthy(&[PERMIT_NETWORK_ENV])
}

/// Whether the operator has opted into audio-capture-capable plugins.
pub fn audio_capture_plugins_permitted() -> bool {
    crate::env_truthy(&[PERMIT_AUDIO_CAPTURE_ENV])
}

/// Whether the operator has opted into audio-output-capable plugins.
pub fn audio_output_plugins_permitted() -> bool {
    crate::env_truthy(&[PERMIT_AUDIO_OUTPUT_ENV])
}

/// Whether the operator has opted into manifest-less `.so` loading.
pub fn unsigned_plugins_allowed() -> bool {
    crate::env_truthy(&[ALLOW_UNSIGNED_ENV])
}

/// Whether the operator has opted into the `experimental` sandbox profile.
pub fn experimental_profiles_allowed() -> bool {
    crate::env_truthy(&[ALLOW_EXPERIMENTAL_ENV])
}
