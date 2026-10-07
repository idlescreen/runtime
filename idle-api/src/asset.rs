// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Host-supplied text assets (ASCII art, logos).
//!
//! # Why the environment and not a callback
//!
//! `crate::callbacks` registers host state through `OnceLock<fn()>`, which
//! works for the daemon's own use of this crate. It does **not** reach a
//! plugin: a plugin is a `cdylib` with its own statically linked copy of
//! `idle-api`, so `idle_api::callbacks::SYSTEM_INFO_CALLBACK` inside the
//! plugin is a distinct symbol from the host's. `nm` on a built saver shows
//! both as local `d` (data) symbols — the plugin defines its own copy and the
//! host's `set` never reaches it.
//!
//! The environment, by contrast, is genuinely shared across the spawn: the
//! daemon reads the asset in its own trusted context and hands the contents
//! to the runner process at spawn time, so the sandboxed child and the
//! plugin never open the file themselves. That keeps the Landlock policy
//! unchanged and needs no capability grant.
//!
//! This mirrors how per-saver parameters already travel
//! (`IDLE_SAVER_PARAM_*`, see [`crate::param`]).

/// The well-known ASCII-art logo asset.
pub const ASSET_LOGO: &str = "logo";

/// Environment prefix for asset delivery.
pub const ASSET_ENV_PREFIX: &str = "IDLE_ASSET_";

/// Upper bound on a single asset.
///
/// Linux caps one environment string at 128 KiB (`MAX_ARG_STRLEN`), and the
/// whole environment is bounded by `ARG_MAX` alongside every other variable.
/// A logo is small; anything near this cap is a misconfiguration, not art.
pub const MAX_ASSET_BYTES: usize = 64 * 1024;

// Compile-time guard: Linux caps one environment string at 128 KiB
// (`MAX_ARG_STRLEN`). An asset above that would fail to reach the child
// process with no error anywhere, so this must fail the build instead.
const _: () = assert!(MAX_ASSET_BYTES <= 128 * 1024);

/// Environment key carrying `name`'s contents.
pub fn env_key(name: &str) -> String {
    format!("{ASSET_ENV_PREFIX}{}", name.to_ascii_uppercase())
}

/// Process-lifetime cache of resolved assets.
///
/// `std::env::var` allocates on every call, and assets are read from the
/// render path — which is required to be allocation-free. Each distinct name
/// is resolved once and then handed out as a `&'static str`.
///
/// The leak is deliberate and bounded: a process loads a handful of assets
/// once, and they live until the runner exits.
fn cache() -> &'static std::sync::Mutex<Vec<(String, &'static str)>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Vec<(String, &'static str)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Returns the host-supplied asset named `name`, or `None` when the host did
/// not supply it.
///
/// Allocation-free after the first call for a given name.
///
/// A saver should treat `None` as "draw the default" rather than an error: the
/// common case by far is a machine with no art file configured at all.
pub fn asset(name: &str) -> Option<&'static str> {
    let mut guard = cache().lock().ok()?;
    if let Some((_, value)) = guard.iter().find(|(key, _)| key == name) {
        return Some(*value);
    }
    // Nothing to sanitise: the daemon already bounded the bytes it wrote, and
    // this never touches the filesystem.
    let raw = std::env::var(env_key(name)).ok()?;
    let leaked: &'static str = Box::leak(raw.into_boxed_str());
    guard.push((name.to_string(), leaked));
    Some(leaked)
}

/// Convenience for [`ASSET_LOGO`].
pub fn logo() -> Option<&'static str> {
    asset(ASSET_LOGO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_key_is_uppercased_and_prefixed() {
        assert_eq!(env_key("logo"), "IDLE_ASSET_LOGO");
        assert_eq!(env_key("LOGO"), "IDLE_ASSET_LOGO");
        assert_eq!(env_key("banner"), "IDLE_ASSET_BANNER");
    }

    #[test]
    fn missing_asset_is_none_not_empty() {
        // The common case is no art file configured. Returning "" here would
        // make a saver render a blank block and look like a rendering bug.
        assert_eq!(asset("definitely_not_set_9f3a"), None);
    }

    #[test]
    fn asset_round_trips_multiline_art() {
        let art = "  _  _\n | || |\n |__   _|\n    |_|";
        // Safety: single-threaded test, and the key is unique to this test.
        unsafe { std::env::set_var(env_key("logo"), art) };
        assert_eq!(asset("logo"), Some(art));
    }

    #[test]
    fn empty_asset_is_distinct_from_absent() {
        unsafe { std::env::set_var(env_key("banner"), "") };
        assert_eq!(asset("banner"), Some(""));
        assert_eq!(asset("never_configured_banner"), None);
    }

    #[test]
    fn a_resolved_asset_is_pinned_for_the_process_lifetime() {
        // The cache exists so the render path never allocates. That makes the
        // value a snapshot taken at first use, not a live view of the
        // environment — an art file edited mid-session is picked up on the
        // next presentation, not mid-frame.
        let first = "first";
        unsafe { std::env::set_var(env_key("pinned"), first) };
        assert_eq!(asset("pinned"), Some(first));

        unsafe { std::env::set_var(env_key("pinned"), "second") };
        assert_eq!(
            asset("pinned"),
            Some(first),
            "asset must be cached, not re-read from the environment"
        );
    }
}
