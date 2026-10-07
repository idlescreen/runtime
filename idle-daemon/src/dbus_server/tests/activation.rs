// SPDX-License-Identifier: MIT

//! D-Bus activation assets must describe the interfaces the daemon actually
//! exports. A stale activation file is worse than a missing one: the bus
//! activates a daemon that then serves a different name, and every caller
//! fails with a confusing error instead of a clear "no service files".

use super::super::{SCREENSAVER_NAME, SCREENSAVER_PATH};
use idle_dbus::SERVICE_NAME;

const SCREENSAVER_SERVICE: &str =
    include_str!("../../../assets/org.freedesktop.ScreenSaver.service");
const IDLE_SERVICE: &str = include_str!("../../../assets/io.github.idlescreen.Idle.service");
const MANIFEST: &str = include_str!("../../../Cargo.toml");

/// Parse the `Key=Value` lines of a `[D-BUS Service]` file.
fn field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key)
            && let Some(value) = rest.strip_prefix('=')
        {
            return Some(value.trim());
        }
    }
    None
}

#[test]
fn screensaver_activation_name_matches_the_claimed_bus_name() {
    assert_eq!(
        field(SCREENSAVER_SERVICE, "Name"),
        Some(SCREENSAVER_NAME),
        "activation Name= must equal the bus name the daemon claims"
    );
}

#[test]
fn screensaver_activation_object_path_is_served() {
    // The freedesktop convention turns the bus name's dots into slashes.
    let derived = format!("/{}", SCREENSAVER_NAME.replace('.', "/"));
    assert_eq!(
        derived, SCREENSAVER_PATH,
        "object path must follow the bus name"
    );
    assert_eq!(
        SCREENSAVER_PATH, "/org/freedesktop/ScreenSaver",
        "callers hard-code this path; changing it is a breaking change"
    );
}

#[test]
fn both_activation_files_launch_the_daemon() {
    for (label, body) in [("ScreenSaver", SCREENSAVER_SERVICE), ("Idle", IDLE_SERVICE)] {
        assert_eq!(
            field(body, "Exec"),
            Some("/usr/bin/idle-daemon daemon"),
            "{label} activation must exec the packaged daemon"
        );
        assert_eq!(
            field(body, "SystemdService"),
            Some("idle-daemon.service"),
            "{label} activation must name the user unit"
        );
    }
}

#[test]
fn idle_activation_name_matches_the_claimed_bus_name() {
    assert_eq!(
        field(IDLE_SERVICE, "Name"),
        Some(SERVICE_NAME),
        "activation Name= must equal the bus name the daemon claims"
    );
}

/// Both activation files ship to the same directory; a packaging change that
/// drops one silently breaks portal inhibition.
#[test]
fn packaging_ships_both_activation_files() {
    let manifest = MANIFEST;
    for asset in [
        "assets/org.freedesktop.ScreenSaver.service",
        "assets/io.github.idlescreen.Idle.service",
    ] {
        assert!(
            manifest.contains(asset),
            "Cargo.toml packaging must reference {asset}"
        );
    }
    // deb assets and rpm assets are separate tables; both need each file.
    assert!(
        manifest
            .matches("assets/org.freedesktop.ScreenSaver.service")
            .count()
            >= 2,
        "expected the ScreenSaver activation file in both deb and rpm asset tables"
    );
}
