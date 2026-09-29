// Temporary stress verifier for SHM boundary and alignment checks
#[cfg(test)]
mod stress_tests {
    use crate::ffi_cell::{FfiTerminalCell, SHM_MAGIC, SharedMemoryHeader, compute_shm_size};
    use crate::shm::SharedMemory;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::thread;

    #[test]
    fn test_rapid_shm_lifecycle_churn() {
        let name = "/idle-shm-rapid-churn-test-0";
        let size = compute_shm_size(80, 24).unwrap();

        for i in 0..500 {
            let shm = SharedMemory::create(name, size)
                .unwrap_or_else(|_| panic!("create failed at iteration {i}"));
            assert_eq!(shm.size(), size);
            unsafe {
                let header = shm.header_mut();
                header.magic = SHM_MAGIC;
                header.cols = 80;
                header.rows = 24;
                header.frame_counter = i as u64;
                let cells = shm.cells_mut().expect("cells_mut");
                cells[0].ch = 'A' as u32;
            }
            drop(shm);
            // Re-open after drop should fail since owner dropped and unlinked
            assert!(
                SharedMemory::open(name, size).is_err(),
                "open should fail after drop at iteration {i}"
            );
        }
    }

    #[test]
    fn test_shm_alignment_and_zero_copy_access() {
        let size = compute_shm_size(100, 100).unwrap();
        let name = "/idle-shm-align-test-0";
        let shm = SharedMemory::create(name, size).unwrap();

        // Verify alignment of header and cells
        let ptr = shm.ptr() as usize;
        let header_align = std::mem::align_of::<SharedMemoryHeader>();
        let cell_align = std::mem::align_of::<FfiTerminalCell>();

        assert_eq!(header_align, 8);
        assert_eq!(cell_align, 4);
        assert_eq!(ptr % header_align, 0, "Header pointer unaligned!");

        let header_sz = std::mem::size_of::<SharedMemoryHeader>();
        assert_eq!(header_sz, 24);
        let cells_ptr = ptr + header_sz;
        assert_eq!(cells_ptr % cell_align, 0, "Cells pointer unaligned!");

        unsafe {
            let header = shm.header_mut();
            header.magic = SHM_MAGIC;
            header.cols = 100;
            header.rows = 100;

            let cells = shm.cells_mut().unwrap();
            for (idx, cell) in cells.iter_mut().enumerate() {
                cell.ch = idx as u32;
                cell.fg_r = (idx & 0xFF) as u8;
                cell.bold = if idx % 2 == 0 { 1 } else { 0 };
            }

            // Verify reads match
            let cells_read = shm.cells_mut().unwrap();
            for (idx, cell) in cells_read.iter().enumerate() {
                assert_eq!(cell.ch, idx as u32);
                assert_eq!(cell.fg_r, (idx & 0xFF) as u8);
                assert_eq!(cell.bold, if idx % 2 == 0 { 1 } else { 0 });
            }
        }
    }

    #[test]
    fn test_integer_overflows_and_boundary_dims() {
        // Test u32::MAX overflow in cols * rows
        let size = compute_shm_size(10, 10).unwrap();
        let name = "/idle-shm-overflow-bounds-0";
        let shm = SharedMemory::create(name, size).unwrap();

        unsafe {
            shm.header_mut().magic = SHM_MAGIC;

            // cols * rows overflow u32 / usize
            shm.header_mut().cols = u32::MAX;
            shm.header_mut().rows = u32::MAX;
            assert!(
                shm.cells_mut().is_err(),
                "u32::MAX cols*rows should overflow check"
            );

            // cols * rows fits in usize but count * cell_sz overflows
            shm.header_mut().cols = 1 << 30;
            shm.header_mut().rows = 8;
            assert!(
                shm.cells_mut().is_err(),
                "Exceeding size should return error"
            );

            // Needed > size check
            shm.header_mut().cols = 11;
            shm.header_mut().rows = 10; // slightly more than 10x10 map
            let err = shm
                .cells_mut()
                .expect_err("Oversized dims within map bounds");
            assert!(err.contains("need"));
        }
    }

    #[test]
    fn test_shm_magic_boundary_rejections() {
        let size = compute_shm_size(5, 5).unwrap();
        let name = "/idle-shm-magic-boundary-0";
        let shm = SharedMemory::create(name, size).unwrap();

        unsafe {
            shm.header_mut().cols = 5;
            shm.header_mut().rows = 5;

            // magic == 0 allowed (pre-handshake)
            shm.header_mut().magic = 0;
            assert!(shm.cells_mut().is_ok());

            // magic == SHM_MAGIC allowed
            shm.header_mut().magic = SHM_MAGIC;
            assert!(shm.cells_mut().is_ok());

            // magic off-by-one lower
            shm.header_mut().magic = SHM_MAGIC - 1;
            assert!(shm.cells_mut().is_err());

            // magic off-by-one higher
            shm.header_mut().magic = SHM_MAGIC + 1;
            assert!(shm.cells_mut().is_err());

            // random corrupt magic
            shm.header_mut().magic = 0x12345678;
            assert!(shm.cells_mut().is_err());
        }
    }

    #[test]
    fn test_concurrent_owner_peer_synchronized_access() {
        let name = "/idle-shm-concurrent-sync-0";
        let size = compute_shm_size(64, 64).unwrap();
        let owner = SharedMemory::create(name, size).unwrap();

        unsafe {
            let h = owner.header_mut();
            h.magic = SHM_MAGIC;
            h.cols = 64;
            h.rows = 64;
            h.frame_counter = 0;
        }

        let mtx = Arc::new(Mutex::new(()));
        let mtx_clone = mtx.clone();
        let name_str = name.to_string();

        let handle = thread::spawn(move || {
            let peer = SharedMemory::open(&name_str, size).expect("peer open");
            for _ in 0..100 {
                let _guard = mtx_clone.lock().unwrap();
                unsafe {
                    let h = peer.header_mut();
                    if h.frame_counter > 0 {
                        let cells = peer.cells_mut().expect("peer cells");
                        let val = cells[0].ch;
                        assert_eq!(val, (h.frame_counter & 0xFF) as u32);
                    }
                }
            }
        });

        for frame in 1..=100 {
            {
                let _guard = mtx.lock().unwrap();
                unsafe {
                    let cells = owner.cells_mut().unwrap();
                    cells[0].ch = (frame & 0xFF) as u32;
                    owner.header_mut().frame_counter = frame;
                }
            }
            thread::yield_now();
        }

        handle.join().unwrap();
    }
}
