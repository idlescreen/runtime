// SPDX-License-Identifier: MIT

/// Visual style for an overlay frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayAppearance {
    /// RGB fill color.
    pub color: [u8; 3],
}

impl OverlayAppearance {
    pub fn solid(color: [u8; 3]) -> Self {
        Self { color }
    }

    /// Deterministic preview color derived from a screensaver name.
    pub fn for_saver(name: &str) -> Self {
        let mut hash = 0u32;
        for byte in name.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(u32::from(byte));
        }

        Self::solid([
            ((hash & 0xFF) / 3 + 18) as u8,
            (((hash >> 8) & 0xFF) / 3 + 18) as u8,
            (((hash >> 16) & 0xFF) / 2 + 40) as u8,
        ])
    }
}
