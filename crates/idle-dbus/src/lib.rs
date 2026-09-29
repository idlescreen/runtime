// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! D-Bus API for the IdleScreen screensaver daemon.
//!
//! ## Well-known names (primary only)
//!
//! - Service: [`SERVICE_NAME`] (`io.github.idlescreen.Idle`)
//! - Path: [`OBJECT_PATH`] (`/io/github/idlescreen/Idle`)
//! - Interface: [`INTERFACE_NAME`] (`io.github.idlescreen.Idle`)

pub mod client;
pub mod locks;
pub mod service;
pub mod status;
pub mod status_contract;

pub use client::{TranceClient, daemon_available};
pub use status::DaemonStatus;
pub use status_contract::{
    STATUS_FIELD_COUNT, STATUS_FIELD_KEYS, sample_preview_status, status_map_has_contract_keys,
};

/// Well-known bus name.
pub const SERVICE_NAME: &str = "io.github.idlescreen.Idle";
/// Object path.
pub const OBJECT_PATH: &str = "/io/github/idlescreen/Idle";
/// D-Bus interface for control methods.
pub const INTERFACE_NAME: &str = "io.github.idlescreen.Idle";

/// Control methods clients may call (contract for CLI/TUI/applet).
pub const CONTROL_METHODS: &[&str] = &[
    "GetStatus",
    "Preview",
    "Stop",
    "Enable",
    "Disable",
    "SetTimeout",
    "SetSaver",
    "ListSavers",
    "ListInhibitors",
];

#[cfg(test)]
mod bus_contract_tests {
    use super::*;

    #[test]
    fn service_path_interface_are_idlescreen() {
        assert!(SERVICE_NAME.starts_with("io.github.idlescreen."));
        assert!(OBJECT_PATH.starts_with("/io/github/idlescreen/"));
        assert_eq!(INTERFACE_NAME, SERVICE_NAME);
    }

    #[test]
    fn no_legacy_trance_bus_names() {
        assert!(!SERVICE_NAME.contains("trance"));
        assert!(!OBJECT_PATH.contains("trance"));
        assert!(!INTERFACE_NAME.contains("trance"));
    }

    #[test]
    fn control_methods_include_preview_stop_status() {
        assert!(CONTROL_METHODS.contains(&"Preview"));
        assert!(CONTROL_METHODS.contains(&"Stop"));
        assert!(CONTROL_METHODS.contains(&"GetStatus"));
        assert!(CONTROL_METHODS.contains(&"ListInhibitors"));
    }

    #[test]
    fn control_methods_are_pascal_case() {
        for m in CONTROL_METHODS {
            assert!(m.chars().next().is_some_and(|c| c.is_ascii_uppercase()));
            assert!(m.chars().all(|c| c.is_ascii_alphanumeric()));
        }
    }
}
