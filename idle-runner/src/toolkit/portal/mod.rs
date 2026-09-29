// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Desktop settings portal integration for accent color and palette synchronization.

pub mod accent_convert;
pub mod settings_client;

#[cfg(test)]
mod portal_tests;

pub use accent_convert::{portal_doubles_to_rgb, portal_scheme_to_dark_mode};
pub use settings_client::{PortalSettingsClient, query_portal_theme};
