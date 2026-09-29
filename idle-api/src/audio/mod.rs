// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Audio reactivity data structures and queries.

pub mod bands;
#[cfg(test)]
mod tests;

pub use bands::{AudioBands, NUM_AUDIO_BANDS};

use std::sync::OnceLock;

static GLOBAL_AUDIO_BANDS: AudioBands = AudioBands::new();

/// Global lock-free atomic audio bands handle.
pub fn global_audio_bands() -> &'static AudioBands {
    &GLOBAL_AUDIO_BANDS
}

/// Host-provided factory for audio bands.
pub static AUDIO_BANDS_CALLBACK: OnceLock<fn() -> [f32; NUM_AUDIO_BANDS]> = OnceLock::new();

/// Returns live audio bands snapshot by calling the host's callback or reading global bands.
pub fn query_audio_bands() -> [f32; NUM_AUDIO_BANDS] {
    if let Some(cb) = AUDIO_BANDS_CALLBACK.get() {
        cb()
    } else {
        GLOBAL_AUDIO_BANDS.get_all()
    }
}
