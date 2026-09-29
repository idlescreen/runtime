// SPDX-License-Identifier: MIT

//! Sandbox profile table (`DECISION-MANIFEST-01`).
//!
//! A profile expands to the set of filesystem trees a plugin may reach, on
//! top of which the loader adds the plugin's own directory and any paths the
//! manifest declares under `[capabilities]`.
//!
//! Profiles are additive, tightest first:
//! - `minimal`      — nothing beyond the plugin dir + font roots.
//! - `renderer`     — `minimal` + read `/usr/share/idle/<plugin_id>`.
//! - `asset-author` — `renderer` + write `~/.local/share/idle/<plugin_id>`.
//! - `experimental` — gated behind `IDLE_ALLOW_EXPERIMENTAL_PROFILES=1`.

use std::path::PathBuf;

/// One allowed filesystem tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessRule {
    pub path: PathBuf,
    /// `false` = read+execute, `true` = read+execute+write.
    pub write: bool,
}

impl AccessRule {
    pub fn read(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write: false,
        }
    }

    pub fn write(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write: true,
        }
    }
}

/// Why a sandbox profile could not be expanded.
#[derive(Debug, PartialEq, Eq)]
pub enum ProfileError {
    Unknown(String),
    ExperimentalNotAllowed,
    UnsupportedPlatform { profile: String },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(p) => write!(f, "unknown sandbox profile '{p}'"),
            Self::ExperimentalNotAllowed => write!(
                f,
                "sandbox profile 'experimental' requires \
                 IDLE_ALLOW_EXPERIMENTAL_PROFILES=1 (refusing to widen the \
                 sandbox implicitly)"
            ),
            Self::UnsupportedPlatform { profile } => write!(
                f,
                "sandbox profile '{profile}' is not built on this platform; \
                 see Sprint 05 (macOS shim / Windows shim) for the seatbelt / \
                 appcontainer enforcers"
            ),
        }
    }
}

impl std::error::Error for ProfileError {}

/// Font roots every profile may read; caption rendering needs them.
pub const FONT_ROOTS: &[&str] = &["/usr/share/fonts", "/usr/share/fontconfig"];

/// Validate a profile name and return its plugin-id-independent rules.
///
/// `minimal` is fully id-independent, so this is the whole story for it. The
/// wider profiles additionally scope trees by plugin id — use
/// [`profile_rules_for`] when an id is available.
pub fn profile_rules(name: &str) -> Result<Vec<AccessRule>, ProfileError> {
    profile_rules_for(name, "")
}

/// Expand `name` into the filesystem trees allowed for `plugin_id`.
///
/// An empty `plugin_id` omits the id-scoped trees rather than granting the
/// parent directory, so a missing id can never widen the sandbox.
pub fn profile_rules_for(name: &str, plugin_id: &str) -> Result<Vec<AccessRule>, ProfileError> {
    let mut rules: Vec<AccessRule> = FONT_ROOTS.iter().map(AccessRule::read).collect();

    match name {
        "minimal" => {}
        "renderer" => push_shared_read(&mut rules, plugin_id),
        "asset-author" => {
            push_shared_read(&mut rules, plugin_id);
            push_user_write(&mut rules, plugin_id);
        }
        "experimental" => {
            if !idle_api::plugin_manifest::host::experimental_profiles_allowed() {
                return Err(ProfileError::ExperimentalNotAllowed);
            }
            push_shared_read(&mut rules, plugin_id);
            push_user_write(&mut rules, plugin_id);
        }
        // Cross-platform stubs (Sprint 05). Names are accepted so manifests
        // can declare them; the Landlock enforcement (this file) only knows
        // Linux, so the runner short-circuits to a clear refusal until the
        // Seatbelt / AppContainer enforcers land.
        "seatbelt" | "appcontainer" => {
            return Err(ProfileError::UnsupportedPlatform {
                profile: name.to_string(),
            });
        }
        other => return Err(ProfileError::Unknown(other.to_string())),
    }

    Ok(rules)
}

fn push_shared_read(rules: &mut Vec<AccessRule>, plugin_id: &str) {
    if !plugin_id.is_empty() {
        rules.push(AccessRule::read(
            PathBuf::from("/usr/share/idle").join(plugin_id),
        ));
    }
}

fn push_user_write(rules: &mut Vec<AccessRule>, plugin_id: &str) {
    if plugin_id.is_empty() {
        return;
    }
    if let Some(dir) = user_data_root() {
        rules.push(AccessRule::write(dir.join(plugin_id)));
    }
}

/// `$XDG_DATA_HOME/idle` or `$HOME/.local/share/idle`. Relative paths are
/// rejected — they would resolve against an attacker-influenced cwd.
fn user_data_root() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/share"),
    };
    base.is_absolute().then(|| base.join("idle"))
}

#[cfg(test)]
#[path = "sandbox_profile_tests.rs"]
mod sandbox_profile_tests;
