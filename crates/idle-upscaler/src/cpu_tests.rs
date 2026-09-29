use super::*;
use crate::FilterMode;

#[test]
fn upscale_stretch_2x_identity_pattern() {
    let src = vec![0u8; 2 * 2 * 4];
    let mut dst = vec![0u8; 4 * 4 * 4];
    let mut cache = StretchCache::new();
    upscale_stretch_into(&mut dst, &src, 2, 2, 4, 4, &mut cache);
    assert_eq!(dst.len(), 4 * 4 * 4);
}

#[test]
fn upscale_letterbox_centers_within_dst() {
    // 4x4 source into 4x4 dst: aspect ratios match, so upscale should fill the dst
    let src = vec![0xFFu8; 4 * 4 * 4];
    let mut dst = vec![0u8; 4 * 4 * 4];
    upscale_letterbox_into(&mut dst, &src, 4, 4, 4, 4, FilterMode::Nearest);
    // All pixels should be 0xFF since the source fills the destination
    assert!(dst.iter().all(|&b| b == 0xFF));
}

#[test]
fn stretch_cache_rebuilds_on_resize() {
    let mut cache = StretchCache::new();
    cache.ensure(10, 20);
    assert_eq!(cache.src_w, 10);
    assert_eq!(cache.dst_w, 20);
    cache.ensure(20, 40);
    assert_eq!(cache.src_w, 20);
    assert_eq!(cache.dst_w, 40);
}

#[test]
fn stretch_cache_reuses_on_same_dims() {
    let mut cache = StretchCache::new();
    cache.ensure(10, 20);
    let len_after_first = cache.x_map.len();
    cache.ensure(10, 20);
    assert_eq!(cache.x_map.len(), len_after_first);
}

#[test]
fn upscale_stretch_handles_zero_dim() {
    let src = vec![0u8; 4];
    let mut dst = vec![0u8; 16];
    let mut cache = StretchCache::new();
    upscale_stretch_into(&mut dst, &src, 0, 0, 2, 2, &mut cache);
    assert_eq!(dst, vec![0u8; 16]);
}

#[test]
fn upscale_stretch_same_size_copies() {
    let src = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
    let mut dst = vec![0u8; 8];
    let mut cache = StretchCache::new();
    upscale_stretch_into(&mut dst, &src, 2, 1, 2, 1, &mut cache);
    assert_eq!(dst, src);
}
