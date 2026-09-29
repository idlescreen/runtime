// SPDX-License-Identifier: MIT

//! Landlock filesystem sandbox for plugin host processes.
//!
//! Policy: handle all FS rights, then allow **only** explicit path trees
//! (plugin directory + optional font dirs). Empty allowlists are rejected —
//! that would deny-all and break `dlopen`, or force operators into escape hatches.

use crate::sandbox_profiles::{AccessRule, profile_rules_for};
use idle_api::plugin_manifest::Manifest;
use std::path::Path;

// ---- Landlock ABI v1 over raw syscalls (was the `landlock` crate) ----

/// Landlock v1 access rights (linux/landlock.h).
mod ll;
use ll::{FdGuard, ll_add_rule, ll_create_ruleset, ll_path_fd, ll_restrict_self};
pub fn sandbox_skip_allowed() -> bool {
    if !idle_api::env_truthy(&["IDLE_DISABLE_SANDBOX"]) {
        return false;
    }
    if cfg!(debug_assertions) {
        return true;
    }
    idle_api::env_truthy(&["IDLE_RENDER_PIPELINE"])
}

/// Strip ambient sandbox escape env from the process (call at `run-ipc-runner`).
pub fn clear_sandbox_escape_env() {
    // SAFETY: single-threaded startup of the IPC child before other threads.
    unsafe {
        std::env::remove_var("IDLE_DISABLE_SANDBOX");
        std::env::remove_var("IDLE_DEV_PLUGINS");
        // Do not clear IDLE_RENDER_PIPELINE here — render host may set it intentionally
        // only on the offline binary, never on daemon IPC children (those should not set it).
    }
}

/// Enforce Landlock with read access to `plugin_path`'s directory (and parents as needed).
///
/// This is the manifest-less entry point: it applies the `minimal` profile,
/// the tightest policy there is. Callers that *have* a manifest must use
/// [`enforce_sandbox_for_plugin_with_manifest`] so declared capabilities are
/// honoured; absent a manifest we fail closed to `minimal` rather than guess.
pub fn enforce_sandbox_for_plugin(plugin_path: &Path) -> Result<(), String> {
    let rules =
        profile_rules_for("minimal", "").map_err(|e| format!("sandbox profile 'minimal': {e}"))?;
    enforce_with_rules(plugin_path, "minimal", &rules)
}

/// Enforce Landlock using the profile and capability declarations in `manifest`.
pub fn enforce_sandbox_for_plugin_with_manifest(
    plugin_path: &Path,
    manifest: &Manifest,
) -> Result<(), String> {
    let profile = manifest.sandbox.profile.as_str();
    let mut rules = profile_rules_for(profile, &manifest.plugin_id)
        .map_err(|e| format!("sandbox profile '{profile}': {e}"))?;

    // Declared capability paths widen the profile, never narrow it.
    for p in &manifest.capabilities.filesystem_read {
        rules.push(AccessRule::read(p));
    }
    for p in &manifest.capabilities.filesystem_write {
        rules.push(AccessRule::write(p));
    }

    enforce_with_rules(plugin_path, profile, &rules)
}

/// Shared core: handle all FS rights, then allow the plugin dir plus `rules`.
fn enforce_with_rules(
    plugin_path: &Path,
    profile: &str,
    rules: &[AccessRule],
) -> Result<(), String> {
    if sandbox_skip_allowed() {
        idle_log::warn!(
            "Landlock sandbox DISABLED (IDLE_DISABLE_SANDBOX) — offline/render or debug only"
        );
        return Ok(());
    }

    let plugin_path = plugin_path
        .canonicalize()
        .map_err(|e| format!("canonicalize plugin path: {e}"))?;
    let parent = plugin_path
        .parent()
        .ok_or_else(|| "plugin path has no parent directory".to_string())?;

    let ruleset_fd = FdGuard(ll_create_ruleset()?);

    // Plugin dir: ReadFile|ReadDir|Execute so `dlopen` of the .so works.
    let plugin_dir_fd = ll_path_fd(parent)?;
    let add_result = ll_add_rule(ruleset_fd.0, ll::READ_EXEC, plugin_dir_fd);
    unsafe { libc::close(plugin_dir_fd) };
    add_result.map_err(|e| format!("add_rule plugin dir: {e}"))?;

    // Profile + capability trees. Absent paths are skipped: Landlock cannot
    // pin a path that does not exist, and refusing here would make an
    // optional font/asset dir a hard load failure.
    for rule in rules {
        if !rule.path.exists() {
            idle_log::debug!("sandbox: skip absent path {}", rule.path.display());
            continue;
        }
        let access = if rule.write {
            ll::READ_WRITE
        } else {
            ll::READ_EXEC
        };
        match ll_path_fd(&rule.path) {
            Ok(fd) => {
                let r = ll_add_rule(ruleset_fd.0, access, fd);
                unsafe { libc::close(fd) };
                r.map_err(|e| format!("add_rule {}: {e}", rule.path.display()))?;
            }
            Err(e) => idle_log::debug!("skip {}: {e}", rule.path.display()),
        }
    }

    let fully_enforced = ll_restrict_self(ruleset_fd.0)?;

    idle_log::info!(
        plugin = %plugin_path.display(),
        profile,
        rules = rules.len(),
        "Landlock filesystem sandbox enforced: {}",
        if fully_enforced {
            "fully enforced"
        } else {
            "partially enforced"
        }
    );
    Ok(())
}

/// Backward-compatible entry used by tests / call sites without a path.
/// Prefer [`enforce_sandbox_for_plugin`].
pub fn enforce_sandbox_or_skip_for_render() -> Result<(), String> {
    // Without a plugin path we cannot build a safe allowlist. Only allow skip
    // when the render/debug escape is set; otherwise fail closed.
    if sandbox_skip_allowed() {
        idle_log::warn!("Landlock sandbox DISABLED via escape hatch (no path)");
        return Ok(());
    }
    Err(
        "enforce_sandbox_or_skip_for_render requires a plugin path; use enforce_sandbox_for_plugin"
            .into(),
    )
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod sandbox_tests;
