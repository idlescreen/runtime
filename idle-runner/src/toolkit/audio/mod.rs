// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Host audio capture and frequency band telemetry.

pub mod bands_calc;
pub mod capture;
#[cfg(test)]
mod tests;

pub use bands_calc::{NUM_AUDIO_BANDS, compute_audio_bands};
pub use capture::{AudioCapture, is_audio_socket_available};

use std::sync::{Mutex, OnceLock};

static GLOBAL_CAPTURE: OnceLock<AudioCapture> = OnceLock::new();
static SIMULATED_BANDS: Mutex<Option<[f32; NUM_AUDIO_BANDS]>> = Mutex::new(None);

/// Query the host audio frequency bands (`[bass, low_mid, mid, treble]`).
///
/// Returns live captured bands if audio subsystem is active, or simulated bands
/// if set, or silent zeroes `[0.0, 0.0, 0.0, 0.0]` if absent.
pub fn query_audio_bands() -> [f32; NUM_AUDIO_BANDS] {
    if let Ok(sim) = SIMULATED_BANDS.lock()
        && let Some(bands) = *sim
    {
        return bands;
    }

    let capture = GLOBAL_CAPTURE.get_or_init(AudioCapture::start);
    capture.current_bands()
}

/// Override query with simulated bands (useful for testing and deterministic previews).
pub fn set_simulated_bands(bands: Option<[f32; NUM_AUDIO_BANDS]>) {
    if let Ok(mut sim) = SIMULATED_BANDS.lock() {
        *sim = bands;
    }
}
