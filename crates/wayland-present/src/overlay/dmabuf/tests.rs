// SPDX-License-Identifier: MIT

use super::pool::DmaBufPool;

#[test]
fn pool_acquires_and_reuses_slots() {
    let mut pool = DmaBufPool::new(3);

    let s0 = pool.acquire_slot(1920, 1080).expect("slot 0");
    let s1 = pool.acquire_slot(1920, 1080).expect("slot 1");
    let s2 = pool.acquire_slot(1920, 1080).expect("slot 2");
    assert_eq!(s0, 0);
    assert_eq!(s1, 1);
    assert_eq!(s2, 2);

    // When all 3 are in use, acquire fails
    assert!(pool.acquire_slot(1920, 1080).is_none());

    // Release slot 1 and re-acquire
    pool.release_slot_by_index(s1);
    let s_new = pool.acquire_slot(1920, 1080).expect("re-acquired slot");
    assert_eq!(s_new, 1);
}

#[test]
fn pool_resizes_slot_dimensions() {
    let mut pool = DmaBufPool::new(2);
    let s0 = pool.acquire_slot(1920, 1080).expect("slot 0");
    pool.release_slot_by_index(s0);

    // Acquire with new dimensions
    let s0_resized = pool.acquire_slot(2560, 1440).expect("slot resized");
    assert_eq!(s0_resized, 0);
    assert_eq!(
        pool.get_slot(0).map(|s| (s.width, s.height)),
        Some((2560, 1440))
    );
}
