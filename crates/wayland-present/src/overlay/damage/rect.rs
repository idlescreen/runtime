// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Damage rectangle geometry for partial surface damage commits.

/// A 2D rectangular region representing damaged / dirty pixel bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DamageRect {
    /// Construct a new damage rectangle.
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Construct a rectangle covering the entire buffer.
    pub const fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    /// True if the damage rectangle has zero area.
    #[allow(dead_code)]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Total pixel area covered by this rectangle.
    pub const fn area(&self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }

    /// Compute the minimal bounding rectangle covering both self and other.
    #[allow(dead_code)]
    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let min_x = self.x.min(other.x);
        let min_y = self.y.min(other.y);
        let max_x = (self.x + self.width).max(other.x + other.width);
        let max_y = (self.y + self.height).max(other.y + other.height);
        Self {
            x: min_x,
            y: min_y,
            width: max_x.saturating_sub(min_x),
            height: max_y.saturating_sub(min_y),
        }
    }
}
