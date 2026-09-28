// SPDX-License-Identifier: Apache-2.0
// perf: T2 · bench: stretch · on-demand only; not gated
// Copyright 2026 IdleScreen

//! Cached nearest-neighbor column map for stretch upscale.

/// Cached nearest-neighbor column map for stretch upscale.
///
/// `x_map[dx]` is the source column for destination column `dx`, computed
/// once in [`StretchCache::ensure`] for a given `(src_w, dst_w)` pair.
/// The hot path (`stretch_u32_rows` / `stretch_byte_rows`) reads
/// `x_map[dx]` instead of dividing per pixel.
pub struct StretchCache {
    /// Source width last used to build `x_map` (test/cache introspection).
    pub src_w: u32,
    /// Destination width last used to build `x_map`.
    pub dst_w: u32,
    /// Nearest-neighbor source-x for each destination column.
    pub x_map: Vec<u32>,
}

/// `StretchCache::new` and `Default` are the same empty state.
impl Default for StretchCache {
    fn default() -> Self {
        Self::new()
    }
}

impl StretchCache {
    pub fn new() -> Self {
        Self {
            src_w: 0,
            dst_w: 0,
            x_map: Vec::new(),
        }
    }

    /// Build (or rebuild) the column map for `(src_w, dst_w)`.
    ///
    /// No-op if the cache already matches and `x_map` length is correct;
    /// the per-frame hot path skips the rebuild.
    pub fn ensure(&mut self, src_w: u32, dst_w: u32) {
        if self.src_w == src_w && self.dst_w == dst_w && self.x_map.len() == dst_w as usize {
            return;
        }
        self.src_w = src_w;
        self.dst_w = dst_w;
        self.x_map = (0..dst_w)
            .map(|dx| (dx as u64 * src_w as u64 / dst_w as u64) as u32)
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::StretchCache;

    #[test]
    fn stretch_cache_rebuilds_on_resize() {
        let mut cache = StretchCache::new();
        cache.ensure(10, 20);
        assert_eq!(cache.src_w, 10);
        assert_eq!(cache.dst_w, 20);
        let len_after_first = cache.x_map.len();
        assert_eq!(len_after_first, 20);
        cache.ensure(20, 40);
        assert_eq!(cache.src_w, 20);
        assert_eq!(cache.x_map.len(), 40);
        // Re-ensuring with the same args must be a no-op (length unchanged).
        cache.ensure(20, 40);
        assert_eq!(cache.x_map.len(), 40);
    }

    #[test]
    fn stretch_cache_x_map_monotonic_within_run() {
        // For src_w=4, dst_w=8, the column map is [0,0,1,1,2,2,3,3].
        let mut cache = StretchCache::new();
        cache.ensure(4, 8);
        assert_eq!(cache.x_map, vec![0, 0, 1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn stretch_cache_x_map_downscale_rounds_down() {
        // For src_w=10, dst_w=4 the floor is used: dx*10/4 -> [0,2,5,7].
        let mut cache = StretchCache::new();
        cache.ensure(10, 4);
        assert_eq!(cache.x_map, vec![0, 2, 5, 7]);
    }
}
