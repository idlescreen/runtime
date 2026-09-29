// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Lock-free atomic audio frequency bands.

use std::sync::atomic::{AtomicU32, Ordering};

pub const NUM_AUDIO_BANDS: usize = 4;

/// Lock-free atomic representation of audio frequency bands.
pub struct AudioBands {
    bands: [AtomicU32; NUM_AUDIO_BANDS],
}

impl Default for AudioBands {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBands {
    /// Create new zero-initialized audio bands.
    pub const fn new() -> Self {
        Self {
            bands: [
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
            ],
        }
    }

    /// Set an individual frequency band (0.0..=1.0).
    pub fn set_band(&self, index: usize, val: f32) {
        if index < NUM_AUDIO_BANDS {
            let clamped = if val.is_nan() || val < 0.0 {
                0.0
            } else if val > 1.0 {
                1.0
            } else {
                val
            };
            self.bands[index].store(clamped.to_bits(), Ordering::Release);
        }
    }

    /// Read an individual frequency band.
    pub fn get_band(&self, index: usize) -> f32 {
        if index < NUM_AUDIO_BANDS {
            f32::from_bits(self.bands[index].load(Ordering::Acquire))
        } else {
            0.0
        }
    }

    /// Set all frequency bands atomically.
    pub fn set_all(&self, values: &[f32; NUM_AUDIO_BANDS]) {
        for (i, &v) in values.iter().enumerate() {
            self.set_band(i, v);
        }
    }

    /// Read snapshot of all frequency bands.
    pub fn get_all(&self) -> [f32; NUM_AUDIO_BANDS] {
        let mut out = [0.0; NUM_AUDIO_BANDS];
        for (i, val) in out.iter_mut().enumerate() {
            *val = self.get_band(i);
        }
        out
    }

    /// Bass / low-end energy (band 0).
    pub fn bass(&self) -> f32 {
        self.get_band(0)
    }

    /// Low-mid frequency energy (band 1).
    pub fn low_mid(&self) -> f32 {
        self.get_band(1)
    }

    /// Mid frequency energy (band 2).
    pub fn mid(&self) -> f32 {
        self.get_band(2)
    }

    /// High / treble energy (band 3).
    pub fn treble(&self) -> f32 {
        self.get_band(3)
    }

    /// Total combined audio energy.
    pub fn total_energy(&self) -> f32 {
        let all = self.get_all();
        all.iter().sum::<f32>() / (NUM_AUDIO_BANDS as f32)
    }
}
