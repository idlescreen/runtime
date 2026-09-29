// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! XDG Desktop Portal Settings client over Session D-Bus.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::accent_convert::{portal_doubles_to_rgb, portal_scheme_to_dark_mode};

const PORTAL_DEST: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_INTERFACE: &str = "org.freedesktop.portal.Settings";
const APPEARANCE_NAMESPACE: &str = "org.freedesktop.appearance";

/// Query the desktop portal for accent color and color scheme.
pub struct PortalSettingsClient;

impl PortalSettingsClient {
    /// Read accent color from portal settings.
    pub fn read_accent_color() -> Result<(u8, u8, u8), String> {
        let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
        let proxy = zbus::blocking::Proxy::new(&conn, PORTAL_DEST, PORTAL_PATH, PORTAL_INTERFACE)
            .map_err(|e| e.to_string())?;

        let reply: (zbus::zvariant::OwnedValue,) = proxy
            .call("Read", &(APPEARANCE_NAMESPACE, "accent-color"))
            .map_err(|e| e.to_string())?;

        let val: zbus::zvariant::Value = reply.0.into();
        if let Ok(doubles) = val.downcast::<(f64, f64, f64)>() {
            return Ok(portal_doubles_to_rgb(doubles.0, doubles.1, doubles.2));
        }

        Err("unexpected accent-color variant format".into())
    }

    /// Read color-scheme (dark mode preference) from portal settings.
    pub fn read_color_scheme() -> Result<Option<bool>, String> {
        let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
        let proxy = zbus::blocking::Proxy::new(&conn, PORTAL_DEST, PORTAL_PATH, PORTAL_INTERFACE)
            .map_err(|e| e.to_string())?;

        let reply: (zbus::zvariant::OwnedValue,) = proxy
            .call("Read", &(APPEARANCE_NAMESPACE, "color-scheme"))
            .map_err(|e| e.to_string())?;

        let val: zbus::zvariant::Value = reply.0.into();
        if let Ok(code) = val.downcast::<u32>() {
            return Ok(portal_scheme_to_dark_mode(code));
        }

        Err("unexpected color-scheme variant format".into())
    }
}

type CachedTheme = (Option<(u8, u8, u8)>, Option<bool>);
static PORTAL_CACHE: OnceLock<Mutex<(Option<CachedTheme>, Instant)>> = OnceLock::new();

/// Cached query for portal theme settings with 2-second TTL.
pub fn query_portal_theme() -> (Option<(u8, u8, u8)>, Option<bool>) {
    let cache_mutex = PORTAL_CACHE.get_or_init(|| Mutex::new((None, Instant::now())));
    let mut cache = match cache_mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Some(val) = cache.0
        && cache.1.elapsed() < Duration::from_secs(2)
    {
        return val;
    }

    let accent = PortalSettingsClient::read_accent_color().ok();
    let dark = PortalSettingsClient::read_color_scheme().ok().flatten();
    let result = (accent, dark);

    cache.0 = Some(result);
    cache.1 = Instant::now();
    result
}
