// SPDX-License-Identifier: MIT

use super::feedback::VrrFeedbackState;
use super::pacing_stats::{RefreshMode, VrrPacingStats};

#[test]
fn record_presentation_computes_hz() {
    let mut stats = VrrPacingStats::new();
    // 16_666_666 ns = 60 Hz
    stats.record_presentation(16_666_666);
    assert_eq!(stats.reported_refresh_hz, Some(60));

    // 6_944_444 ns = 144 Hz
    stats.record_presentation(6_944_444);
    assert_eq!(stats.reported_refresh_hz, Some(144));
}

#[test]
fn detect_mode_fixed_vsync() {
    let mut stats = VrrPacingStats::new();
    for _ in 0..10 {
        stats.record_presentation(16_666_666);
    }
    assert_eq!(stats.detect_mode(), RefreshMode::FixedVsync(60));
}

#[test]
fn detect_mode_adaptive_sync() {
    let mut stats = VrrPacingStats::new();
    // Vary between 144 Hz (6.9ms) and 48 Hz (20.8ms)
    let intervals = [
        6_944_444, 10_000_000, 16_666_666, 20_833_333, 8_000_000, 15_000_000,
    ];
    for &int in &intervals {
        stats.record_presentation(int);
    }
    let mode = stats.detect_mode();
    if let RefreshMode::AdaptiveSync { min_hz, max_hz } = mode {
        assert!(min_hz <= 60);
        assert!(max_hz >= 120);
    } else {
        assert!(
            matches!(mode, RefreshMode::AdaptiveSync { .. }),
            "expected AdaptiveSync"
        );
    }
}

#[test]
fn feedback_state_tracks_zero_copy_and_discards() {
    let mut state = VrrFeedbackState::new();
    assert_eq!(state.presentation_count, 0);
    assert_eq!(state.discarded_count, 0);

    // flags with ZERO_COPY (bit 3 / 8)
    state.record_presented(100, 500_000, 16_666_666, 8 | 1);
    assert_eq!(state.presentation_count, 1);
    assert_eq!(state.zero_copy_count, 1);
    assert_eq!(state.last_tv_sec, 100);
    assert_eq!(state.last_tv_nsec, 500_000);

    state.record_discarded();
    assert_eq!(state.discarded_count, 1);
}
