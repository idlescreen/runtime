// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Frequency band energy computation from raw PCM audio.

pub const NUM_AUDIO_BANDS: usize = 4;

/// Compute 4-band normalized energy from 16-bit mono PCM samples.
///
/// Bins are mapped for ~8000 Hz sampling:
/// - Band 0 (Bass): ~60 Hz – 250 Hz
/// - Band 1 (Low-Mid): ~250 Hz – 600 Hz
/// - Band 2 (Mid): ~600 Hz – 1800 Hz
/// - Band 3 (Treble): ~1800 Hz – 4000 Hz
pub fn compute_audio_bands(samples: &[i16]) -> [f32; NUM_AUDIO_BANDS] {
    if samples.is_empty() {
        return [0.0; NUM_AUDIO_BANDS];
    }

    // Downsample or window to max 64 samples for fast bounded transform
    let n = samples.len().min(64);
    if n < 8 {
        return [0.0; NUM_AUDIO_BANDS];
    }

    let n_f32 = n as f32;
    let mut band_energies = [0.0f32; NUM_AUDIO_BANDS];
    let mut band_counts = [0usize; NUM_AUDIO_BANDS];

    // Evaluate DFT bins 1 to N/2
    let half_n = n / 2;
    for k in 1..half_n {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        let omega = 2.0 * std::f32::consts::PI * (k as f32) / n_f32;

        for (idx, &sample) in samples.iter().take(n).enumerate() {
            let normalized = (sample as f32) / 32768.0;
            let angle = omega * (idx as f32);
            re += normalized * angle.cos();
            im -= normalized * angle.sin();
        }

        let mag_sq = (re * re + im * im) / (n_f32 * n_f32);
        let band_idx = if k <= 2 {
            0 // Bass
        } else if k <= 5 {
            1 // Low-mid
        } else if k <= 14 {
            2 // Mid
        } else {
            3 // Treble
        };

        band_energies[band_idx] += mag_sq;
        band_counts[band_idx] += 1;
    }

    let mut out = [0.0f32; NUM_AUDIO_BANDS];
    for i in 0..NUM_AUDIO_BANDS {
        if band_counts[i] > 0 {
            // Apply square root to get amplitude response and gain factor
            let amp = (band_energies[i] / band_counts[i] as f32).sqrt() * 6.0;
            out[i] = if amp.is_nan() || amp < 0.0 {
                0.0
            } else if amp > 1.0 {
                1.0
            } else {
                amp
            };
        }
    }
    out
}
