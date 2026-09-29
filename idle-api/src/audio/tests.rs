// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Unit tests for lock-free atomic AudioBands.

use super::bands::{AudioBands, NUM_AUDIO_BANDS};
use super::{global_audio_bands, query_audio_bands};

#[test]
fn test_audio_bands_default_zero() {
    let bands = AudioBands::new();
    assert_eq!(bands.get_all(), [0.0; NUM_AUDIO_BANDS]);
    assert_eq!(bands.bass(), 0.0);
    assert_eq!(bands.mid(), 0.0);
    assert_eq!(bands.treble(), 0.0);
    assert_eq!(bands.total_energy(), 0.0);
}

#[test]
fn test_audio_bands_set_get() {
    let bands = AudioBands::new();
    bands.set_band(0, 0.8);
    bands.set_band(1, 0.5);
    bands.set_band(2, 0.3);
    bands.set_band(3, 0.1);

    assert!((bands.bass() - 0.8).abs() < 1e-4);
    assert!((bands.low_mid() - 0.5).abs() < 1e-4);
    assert!((bands.mid() - 0.3).abs() < 1e-4);
    assert!((bands.treble() - 0.1).abs() < 1e-4);
}

#[test]
fn test_audio_bands_clamping() {
    let bands = AudioBands::new();
    bands.set_band(0, -0.5);
    assert_eq!(bands.bass(), 0.0);

    bands.set_band(0, 1.5);
    assert_eq!(bands.bass(), 1.0);

    bands.set_band(0, f32::NAN);
    assert_eq!(bands.bass(), 0.0);
}

#[test]
fn test_global_audio_bands() {
    let _ = global_audio_bands();
    let snap = query_audio_bands();
    assert_eq!(snap.len(), NUM_AUDIO_BANDS);
}
