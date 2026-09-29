// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Adversarial tests for F-005 (`publish_primary_bounds` must not call
//! `unsafe std::env::set_var`).
//!
//! Per AUDIT.md §7: tests must exercise the real `monitor::env` module and
//! would fail if the fix were reverted.

use crate::{
    MonitorCellBounds, clear_primary_bounds, get_primary_monitor_bounds, publish_primary_bounds,
};
use std::sync::Mutex;

/// Serialize tests that mutate process-global env vars so they don't race
/// with siblings.
static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn publish_does_not_set_process_env_vars() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: env-var mutation is the test's purpose.
    unsafe {
        std::env::remove_var("IDLE_PRIMARY_START_COL");
        std::env::remove_var("IDLE_PRIMARY_END_COL");
        std::env::remove_var("IDLE_PRIMARY_START_ROW");
        std::env::remove_var("IDLE_PRIMARY_END_ROW");
    }

    publish_primary_bounds(MonitorCellBounds {
        start_col: 2,
        end_col: 8,
        start_row: 1,
        end_row: 4,
        is_primary: true,
    });

    // After publishing, none of the env vars should be set.
    assert!(
        std::env::var("IDLE_PRIMARY_START_COL").is_err(),
        "publish_primary_bounds must NOT set IDLE_PRIMARY_START_COL"
    );
    assert!(
        std::env::var("IDLE_PRIMARY_END_COL").is_err(),
        "publish_primary_bounds must NOT set IDLE_PRIMARY_END_COL"
    );
    assert!(
        std::env::var("IDLE_PRIMARY_START_ROW").is_err(),
        "publish_primary_bounds must NOT set IDLE_PRIMARY_START_ROW"
    );
    assert!(
        std::env::var("IDLE_PRIMARY_END_ROW").is_err(),
        "publish_primary_bounds must NOT set IDLE_PRIMARY_END_ROW"
    );
}

#[test]
fn clear_does_not_remove_process_env_vars() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // Pre-set env vars so we can detect if clear_primary_bounds removes them.
    unsafe {
        std::env::set_var("IDLE_PRIMARY_START_COL", "10");
        std::env::set_var("IDLE_PRIMARY_END_COL", "20");
        std::env::set_var("IDLE_PRIMARY_START_ROW", "5");
        std::env::set_var("IDLE_PRIMARY_END_ROW", "15");
    }

    // clear_primary_bounds must not touch the process env (it only clears
    // the in-process cache).
    clear_primary_bounds();

    assert_eq!(
        std::env::var("IDLE_PRIMARY_START_COL").ok().as_deref(),
        Some("10"),
        "clear_primary_bounds must NOT remove IDLE_PRIMARY_START_COL"
    );
    assert_eq!(
        std::env::var("IDLE_PRIMARY_END_COL").ok().as_deref(),
        Some("20"),
        "clear_primary_bounds must NOT remove IDLE_PRIMARY_END_COL"
    );

    // Cleanup.
    unsafe {
        std::env::remove_var("IDLE_PRIMARY_START_COL");
        std::env::remove_var("IDLE_PRIMARY_END_COL");
        std::env::remove_var("IDLE_PRIMARY_START_ROW");
        std::env::remove_var("IDLE_PRIMARY_END_ROW");
    }
}

#[test]
fn cache_round_trip_within_process() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: env cleanup before/after.
    unsafe {
        std::env::remove_var("IDLE_PRIMARY_START_COL");
        std::env::remove_var("IDLE_PRIMARY_END_COL");
        std::env::remove_var("IDLE_PRIMARY_START_ROW");
        std::env::remove_var("IDLE_PRIMARY_END_ROW");
    }
    clear_primary_bounds();

    publish_primary_bounds(MonitorCellBounds {
        start_col: 2,
        end_col: 8,
        start_row: 1,
        end_row: 4,
        is_primary: true,
    });
    let b = get_primary_monitor_bounds(80, 24);
    assert_eq!(b.start_col, 2);
    assert_eq!(b.end_col, 8);
    assert_eq!(b.start_row, 1);
    assert_eq!(b.end_row, 4);

    clear_primary_bounds();
    // After clear, default returns to full grid (within this process).
    let b = get_primary_monitor_bounds(80, 24);
    assert_eq!(b.start_col, 0);
    assert_eq!(b.end_col, 80);
}
