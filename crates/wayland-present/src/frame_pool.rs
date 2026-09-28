// SPDX-License-Identifier: MIT

//! Frame buffer pool for the overlay presenter.
//!
//! Triple-buffered across the daemon and the event thread. The
//! daemon hands the presenter an `Arc<Vec<u8>>` via `Arc::new(pixels)`,
//! the presenter holds it across one Wayland commit, then pushes it
//! back. The Vec keeps `try_unwrap` available (sized), so the
//! recycling path can reclaim the heap allocation without a clone.
//!
//! Replaces the prior `mpsc::Sender<Vec<u8>>` + `sync_channel(1)`
//! round-trip. Steady-state cost is one `Arc::clone` per frame.
//! Type alias: `Arc<Mutex<VecDeque<Arc<Vec<u8>>>>>`. We can't use
//! `Arc<[u8]>` here because `[u8]` is unsized and lacks
//! `try_unwrap`/`into_inner`, so the recycler couldn't reclaim
//! the bytes without a copy.

use std::sync::{Arc, Mutex};

/// Frame buffer pool. See module docs for the recycler contract.
pub type FramePool = Arc<Mutex<std::collections::VecDeque<Arc<Vec<u8>>>>>;

/// Construct a fresh empty pool.
pub fn empty_frame_pool() -> FramePool {
    Arc::new(Mutex::new(std::collections::VecDeque::new()))
}

/// Pop a recyclable buffer from the pool, sizing it to `size` if the
/// cached buffer doesn't match. If the pool is empty we allocate a
/// fresh zeroed `Vec<u8>` — the daemon's first frame.
///
/// Returns a `Vec<u8>` so the daemon can write into it via
/// `&mut [u8]`; the producer wraps it in `Arc::new(pixels)` before
/// submitting. Recycling relies on `Arc::try_unwrap` succeeding
/// (refcount == 1, only the pool held the Arc).
pub fn get_frame_buffer(pool: &FramePool, size: usize) -> Vec<u8> {
    let mut pool = pool.lock().unwrap_or_else(|p| {
        idle_log::warn!("wayland-present: frame_pool mutex poisoned; recovering");
        p.into_inner()
    });
    while let Some(arc) = pool.pop_front() {
        match Arc::try_unwrap(arc) {
            Ok(v) => {
                if v.len() == size {
                    return v;
                }
                let mut v = v;
                v.resize(size, 0);
                return v;
            }
            Err(arc) => {
                // Another caller still references this buffer
                // (e.g. the event thread hasn't dropped its Arc
                // yet). Skip it; the next iteration pulls the
                // next available one. Bounded: steady-state each
                // frame produces one buffer and consumes one.
                idle_log::debug!("wayland-present: skip contested frame buffer (refcount > 1)");
                drop(arc);
            }
        }
    }
    vec![0; size]
}

/// Push a returned `Arc<Vec<u8>>` back onto the pool. Single-call
/// helper so the event thread's UpdateFrame arm has one obvious
/// place to recycle.
pub fn return_frame_buffer(pool: &FramePool, frame: Arc<Vec<u8>>) {
    match pool.lock() {
        Ok(mut p) => p.push_back(frame),
        Err(_) => {
            idle_log::warn!("wayland-present: frame_pool mutex poisoned; dropping returned frame");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_frame_buffer_returns_zeroed_vec_when_pool_empty() {
        let pool = empty_frame_pool();
        let buf = get_frame_buffer(&pool, 64);
        assert_eq!(buf.len(), 64);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn return_then_get_recycles_exact_size() {
        let pool = empty_frame_pool();
        let original: Arc<Vec<u8>> = Arc::new(vec![1u8, 2, 3, 4]);
        return_frame_buffer(&pool, original.clone());
        // Drop the original Arc so try_unwrap succeeds.
        drop(original);
        let recovered = get_frame_buffer(&pool, 4);
        assert_eq!(recovered, vec![1u8, 2, 3, 4]);
    }

    #[test]
    fn get_resizes_mismatched_buffer() {
        let pool = empty_frame_pool();
        let original: Arc<Vec<u8>> = Arc::new(vec![0xAAu8; 8]);
        return_frame_buffer(&pool, original.clone());
        drop(original);
        // Ask for a smaller size: get_frame_buffer must resize.
        let smaller = get_frame_buffer(&pool, 4);
        assert_eq!(smaller.len(), 4);
        assert!(smaller.iter().all(|&b| b == 0xAA));
    }

    #[test]
    fn get_skips_contested_buffer() {
        // A returned buffer that's still referenced by another Arc
        // must be skipped (the next pop_front either finds another
        // entry or falls through to vec![0; size]).
        let pool = empty_frame_pool();
        let contested: Arc<Vec<u8>> = Arc::new(vec![0x77u8; 4]);
        return_frame_buffer(&pool, contested.clone());
        // Keep `contested` alive; try_unwrap will fail and the
        // function must fall through to the fresh allocation.
        let fresh = get_frame_buffer(&pool, 4);
        assert_eq!(fresh, vec![0u8; 4]);
        drop(contested);
    }
}

#[cfg(test)]
mod benches {
    use super::*;
    use criterion::Criterion;
    use std::hint::black_box;

    #[test]
    fn bench_get_frame_buffer() {
        let mut c = Criterion::default().sample_size(10);
        let pool = empty_frame_pool();
        // Pre-populate so the hot path is try_unwrap + return,
        // not the empty-pool allocation.
        let size = 1920 * 1080 * 4;
        let buf: Arc<Vec<u8>> = Arc::new(vec![0u8; size]);
        for _ in 0..4 {
            return_frame_buffer(&pool, buf.clone());
        }
        drop(buf);
        c.bench_function("get_frame_buffer_recycle", |b| {
            b.iter(|| {
                let v = get_frame_buffer(black_box(&pool), black_box(size));
                return_frame_buffer(&pool, Arc::new(v));
            });
        });
    }
}
