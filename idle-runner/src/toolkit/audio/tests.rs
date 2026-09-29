// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

#![allow(clippy::float_cmp)]

use super::bands_calc::{NUM_AUDIO_BANDS, compute_audio_bands};
use super::capture::{AudioCapture, is_audio_socket_available};
use super::{query_audio_bands, set_simulated_bands};

#[test]
fn test_silent_audio_produces_zeros() {
    let zeros = [0i16; 128];
    let bands = compute_audio_bands(&zeros);
    for b in bands {
        assert_eq!(b, 0.0);
    }
}

#[test]
fn test_empty_audio_produces_zeros() {
    let bands = compute_audio_bands(&[]);
    for b in bands {
        assert_eq!(b, 0.0);
    }
}

#[test]
fn test_synthetic_bass_tone() {
    // 100 Hz sine wave sampled at 8000 Hz: period is 80 samples
    let mut samples = [0i16; 64];
    for (i, sample) in samples.iter_mut().enumerate() {
        let t = (i as f32) / 8000.0;
        let val = (2.0 * std::f32::consts::PI * 100.0 * t).sin();
        *sample = (val * 20000.0) as i16;
    }

    let bands = compute_audio_bands(&samples);
    assert!(
        bands[0] > 0.05,
        "Bass band should detect 100 Hz tone, got {}",
        bands[0]
    );
    assert!(
        bands[0] > bands[3],
        "Bass energy should be greater than treble"
    );
}

#[test]
fn test_synthetic_treble_tone() {
    // 3000 Hz sine wave sampled at 8000 Hz
    let mut samples = [0i16; 64];
    for (i, sample) in samples.iter_mut().enumerate() {
        let t = (i as f32) / 8000.0;
        let val = (2.0 * std::f32::consts::PI * 3000.0 * t).sin();
        *sample = (val * 20000.0) as i16;
    }

    let bands = compute_audio_bands(&samples);
    assert!(
        bands[3] > 0.05,
        "Treble band should detect 3000 Hz tone, got {}",
        bands[3]
    );
    assert!(
        bands[3] > bands[0],
        "Treble energy should be greater than bass"
    );
}

#[test]
fn test_simulated_bands_roundtrip() {
    let test_bands: [f32; NUM_AUDIO_BANDS] = [0.25, 0.5, 0.75, 1.0];
    set_simulated_bands(Some(test_bands));

    let queried = query_audio_bands();
    assert_eq!(queried, test_bands);

    set_simulated_bands(None);
}

#[test]
fn test_audio_socket_detection_is_safe() {
    let _ = is_audio_socket_available();
}

#[test]
fn test_audio_capture_lifecycle() {
    let capture = AudioCapture::start();
    let bands = capture.current_bands();
    assert_eq!(bands.len(), NUM_AUDIO_BANDS);
    for b in bands {
        assert!(b >= 0.0 && b <= 1.0);
    }
}
