// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::ffi_cell::{FfiTerminalCell, SHM_MAGIC, SharedMemoryHeader};

const MAX_SHM: usize = 64 * 1024 * 1024;

fn header_size() -> usize {
    std::mem::size_of::<SharedMemoryHeader>()
}

#[test]
fn create_rejects_bad_name() {
    match SharedMemory::create("/evil", 4096) {
        Err(err) => assert!(err.contains("invalid shm name")),
        Ok(_) => panic!("expected invalid name error"),
    }
}

#[test]
fn open_rejects_bad_name() {
    match SharedMemory::open("../etc/shadow", 4096) {
        Err(err) => assert!(err.contains("invalid shm name")),
        Ok(_) => panic!("expected invalid name error"),
    }
    match SharedMemory::open("/trance-shm-a/b", 4096) {
        Err(err) => assert!(err.contains("invalid shm name")),
        Ok(_) => panic!("expected invalid name error"),
    }
}

#[test]
fn create_rejects_tiny_size() {
    match SharedMemory::create("/trance-shm-1-0", 1) {
        Err(err) => assert!(err.contains("size")),
        Ok(_) => panic!("expected size error"),
    }
    match SharedMemory::create("/trance-shm-size-below-0", header_size().saturating_sub(1)) {
        Err(err) => assert!(err.contains("size")),
        Ok(_) => panic!("expected size error for below-header"),
    }
}

#[test]
fn create_rejects_oversized() {
    let huge = MAX_SHM + 1;
    match SharedMemory::create("/trance-shm-size-cap-0", huge) {
        Err(err) => assert!(err.contains("size")),
        Ok(_) => panic!("expected size error"),
    }
}

#[test]
fn open_rejects_size_out_of_range() {
    match SharedMemory::open("/trance-shm-open-tiny-0", 1) {
        Err(err) => assert!(err.contains("size"), "tiny: {err}"),
        Ok(_) => panic!("expected size error for open tiny"),
    }
    match SharedMemory::open("/trance-shm-open-big-0", MAX_SHM + 1) {
        Err(err) => assert!(err.contains("size"), "oversize: {err}"),
        Ok(_) => panic!("expected size error for open oversize"),
    }
    match SharedMemory::open("/trance-shm-open-below-0", header_size().saturating_sub(1)) {
        Err(err) => assert!(err.contains("size"), "below: {err}"),
        Ok(_) => panic!("expected size error for open below-header"),
    }
}

#[test]
fn create_accepts_header_mid_and_max_sizes() {
    // Exact header size (kills `<` → `<=` on lower bound when combined with success).
    let hdr = header_size();
    let a = SharedMemory::create("/trance-shm-test-hdr-0", hdr).expect("header size");
    assert_eq!(a.size(), hdr);
    assert_eq!(a.name(), "/trance-shm-test-hdr-0");
    assert!(a.fd() > 0, "live fd, got {}", a.fd());
    assert!(!a.ptr().is_null());
    // open must accept the same lower bound while the object still exists.
    let a_peer = SharedMemory::open("/trance-shm-test-hdr-0", hdr).expect("open header size");
    assert_eq!(a_peer.size(), hdr);
    drop(a_peer);
    drop(a);

    // 1 MiB is above the degenerate `64+1024+1024` a `*`→`+` mutant would allow.
    let mid = 1024 * 1024;
    let b = SharedMemory::create("/trance-shm-test-mid-0", mid).expect("1MiB");
    assert_eq!(b.size(), mid);
    let b_peer = SharedMemory::open("/trance-shm-test-mid-0", mid).expect("open 1MiB");
    assert_eq!(b_peer.size(), mid);
    drop(b_peer);
    drop(b);

    // Exact upper bound must succeed (`>` must not become `>=`) for create and open.
    let c = SharedMemory::create("/trance-shm-test-max-0", MAX_SHM).expect("exact max");
    assert_eq!(c.size(), MAX_SHM);
    let c_peer = SharedMemory::open("/trance-shm-test-max-0", MAX_SHM).expect("open exact max");
    assert_eq!(c_peer.size(), MAX_SHM);
}

#[test]
fn cells_mut_rejects_bad_magic() {
    let size = crate::compute_shm_size(4, 2).expect("size");
    let shm = SharedMemory::create("/trance-shm-test-magic-0", size).expect("create");
    // SAFETY: just created; write garbage magic then check cells_mut rejects it.
    unsafe {
        shm.header_mut().magic = 0xDEAD_BEEF;
        shm.header_mut().cols = 4;
        shm.header_mut().rows = 2;
        let err = shm.cells_mut().expect_err("bad magic");
        assert!(err.contains("magic"));
    }
}

#[test]
fn cells_mut_accepts_zero_or_correct_magic() {
    let size = crate::compute_shm_size(3, 2).expect("size");
    let shm = SharedMemory::create("/trance-shm-test-magic-ok-0", size).expect("create");
    // SAFETY: init header within map; magic 0 allowed pre-handshake.
    unsafe {
        shm.header_mut().magic = 0;
        shm.header_mut().cols = 3;
        shm.header_mut().rows = 2;
        let cells = shm.cells_mut().expect("magic 0 ok");
        assert_eq!(cells.len(), 6);

        shm.header_mut().magic = SHM_MAGIC;
        let cells = shm.cells_mut().expect("correct magic");
        assert_eq!(cells.len(), 6);
    }
}

#[test]
fn cells_mut_rejects_dims_exceeding_map() {
    let size = crate::compute_shm_size(2, 2).expect("size");
    let shm = SharedMemory::create("/trance-shm-test-dims-0", size).expect("create");
    // SAFETY: claim more cells than the map holds.
    unsafe {
        shm.header_mut().magic = SHM_MAGIC;
        shm.header_mut().cols = 10_000;
        shm.header_mut().rows = 10_000;
        let err = shm.cells_mut().expect_err("oversized dims");
        assert!(
            err.contains("need") || err.contains("overflow") || err.contains("map"),
            "unexpected err: {err}"
        );
    }
}

#[test]
fn create_open_roundtrip_name() {
    let size = crate::compute_shm_size(4, 2).expect("size");
    let name = "/trance-shm-test-roundtrip-0";
    let owner = SharedMemory::create(name, size).expect("create");
    assert_eq!(owner.name(), name);
    assert_eq!(owner.size(), size);
    assert!(owner.fd() > 0);
    // SAFETY: exclusive owner initializes header + one cell for the open peer.
    unsafe {
        let h = owner.header_mut();
        h.magic = SHM_MAGIC;
        h.cols = 4;
        h.rows = 2;
        h.frame_counter = 0;
        let cells = owner.cells_mut().expect("owner cells");
        cells[0] = FfiTerminalCell {
            ch: b'Z' as u32,
            fg_r: 9,
            fg_g: 8,
            fg_b: 7,
            bg_r: 0,
            bg_g: 0,
            bg_b: 0,
            bold: 0,
        };
    }
    {
        let peer = SharedMemory::open(name, size).expect("open while owner live");
        assert_eq!(peer.size(), size);
        assert_eq!(peer.name(), name);
        // SAFETY: peer mapping; dims match create.
        unsafe {
            let cells = peer.cells_mut().expect("peer cells");
            assert_eq!(cells[0].ch, b'Z' as u32);
        }
    }
    // Drop owner unlinks; open should then fail.
    drop(owner);
    let reopen = SharedMemory::open(name, size);
    assert!(reopen.is_err(), "open after owner drop should fail");
}

#[test]
fn test_immune_rail_shm_security_and_bounds() {
    use crate::{SHM_MAGIC, SharedMemory, compute_shm_size, is_valid_shm_name};

    // 1. Path traversal & invalid prefix rejection
    assert!(!is_valid_shm_name("/idle-shm-../etc/passwd"));
    assert!(!is_valid_shm_name("/dev/shm/idle-shm-1-0"));
    assert!(!is_valid_shm_name("/other-prefix-1"));
    assert!(!is_valid_shm_name("/idle-shm-"));
    assert!(SharedMemory::create("/idle-shm-../etc/passwd", 4096).is_err());

    // 2. Corrupted SHM header magic rejection
    let sz = compute_shm_size(4, 2).unwrap();
    let shm = SharedMemory::create("/idle-shm-magic-test-0", sz).unwrap();
    unsafe {
        shm.header_mut().magic = 0xDEAD_BEEF;
        shm.header_mut().cols = 4;
        shm.header_mut().rows = 2;
        let err = shm.cells_mut().expect_err("corrupt magic must fail");
        assert!(err.contains("magic"));
    }

    // 3. Cell allocation overflow within mapped region
    let sz_small = compute_shm_size(2, 2).unwrap();
    let shm_over = SharedMemory::create("/idle-shm-overflow-test-0", sz_small).unwrap();
    unsafe {
        shm_over.header_mut().magic = SHM_MAGIC;
        shm_over.header_mut().cols = 10_000;
        shm_over.header_mut().rows = 10_000;
        let err = shm_over
            .cells_mut()
            .expect_err("oversized cell dims must fail");
        assert!(err.contains("need") || err.contains("map"));
    }
}
