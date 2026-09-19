// SPDX-License-Identifier: MIT

#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![warn(clippy::panic)]
#![warn(clippy::todo)]
#![warn(clippy::unimplemented)]

pub mod config;
pub mod config_parse;
pub mod futures_util;

pub mod cli_main;
#[cfg(test)]
mod config_fuzz_tests;
#[cfg(test)]
mod config_merge_tests;
pub mod config_watcher;
pub mod controller;
pub mod daemon;
pub mod dbus_server;
pub mod failsafe;
pub mod inhibit;
pub mod ipc_runner;
pub mod lock_monitor;
pub mod locks;
pub mod ooda;
pub mod presentation;
pub mod sleep_monitor;

/// Shared mutex for tests that mutate process env. Environment is
/// process-global and the whole lib test suite runs in one process —
/// file-local locks can't exclude siblings in other modules. Any test
/// that sets/removes an env var must hold this for its full body.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
