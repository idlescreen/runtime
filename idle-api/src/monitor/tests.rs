use super::*;
use std::sync::Mutex;

/// Serialize tests that touch process-global env or the primary-bounds cache.
static SERIAL: Mutex<()> = Mutex::new(());

fn bounds(start_col: usize, end_col: usize, start_row: usize, end_row: usize) -> MonitorCellBounds {
    MonitorCellBounds {
        start_col,
        end_col,
        start_row,
        end_row,
        is_primary: true,
    }
}

#[test]
fn bounds_contains_inside() {
    let b = bounds(0, 10, 0, 5);
    assert!(b.contains(5, 3));
}

#[test]
fn bounds_excludes_outside() {
    let b = bounds(0, 10, 0, 5);
    assert!(!b.contains(11, 3));
    assert!(!b.contains(5, 6));
}

#[test]
fn bounds_excludes_end_exclusive() {
    let b = bounds(0, 10, 0, 5);
    assert!(!b.contains(10, 0));
    assert!(!b.contains(0, 5));
}

#[test]
fn bounds_width_height() {
    let b = bounds(0, 10, 0, 5);
    assert_eq!(b.width(), 10);
    assert_eq!(b.height(), 5);
}

#[test]
fn bounds_width_height_saturate_when_inverted() {
    let b = bounds(8, 4, 6, 2);
    assert_eq!(b.width(), 0);
    assert_eq!(b.height(), 0);
}

#[test]
fn bounds_centers() {
    let b = bounds(0, 10, 0, 6);
    assert_eq!(b.center_col(), 5);
    assert_eq!(b.center_row(), 3);
}

#[test]
fn get_primary_monitor_bounds_default_is_full_grid() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // No callback set and no env vars in test by default
    // SAFETY: test isolation — clear env so default full-grid path is exercised.
    unsafe {
        std::env::remove_var("IDLE_PRIMARY_START_COL");
        std::env::remove_var("IDLE_PRIMARY_END_COL");
        std::env::remove_var("IDLE_PRIMARY_START_ROW");
        std::env::remove_var("IDLE_PRIMARY_END_ROW");
    }
    clear_primary_bounds();
    let b = get_primary_monitor_bounds(80, 24);
    assert_eq!(b.start_col, 0);
    assert_eq!(b.end_col, 80);
    assert_eq!(b.start_row, 0);
    assert_eq!(b.end_row, 24);
    assert!(b.is_primary);
}

#[test]
fn publish_then_get_primary_bounds_round_trip() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    clear_primary_bounds();
    publish_primary_bounds(bounds(2, 8, 1, 4));
    let b = get_primary_monitor_bounds(80, 24);
    assert_eq!(b.start_col, 2);
    assert_eq!(b.end_col, 8);
    assert_eq!(b.start_row, 1);
    assert_eq!(b.end_row, 4);
    clear_primary_bounds();
}

#[test]
fn is_secondary_monitor_default_false() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: test isolation for secondary-monitor env flag.
    unsafe {
        std::env::remove_var("IDLE_SECONDARY_MONITOR");
    }
    assert!(!is_secondary_monitor());
}
