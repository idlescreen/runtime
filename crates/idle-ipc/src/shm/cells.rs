// SPDX-License-Identifier: MIT
//! Borrowed views into a live shared-memory mapping.
//!
//! Split out of `shm.rs`, which sat exactly on the 256-line ceiling.
//! These are the only two methods that hand out a raw pointer into the
//! mapping, so they are worth a page of their own: this is where the
//! `# Safety` contract is written down, and where a reader auditing that
//! contract should be looking.

use crate::ffi_cell::{FfiTerminalCell, SHM_MAGIC, SharedMemoryHeader};
use super::SharedMemory;

impl SharedMemory {
    /// Mutable header view.
    ///
    /// # Safety
    /// - Mapping is live (`create`/`open` succeeded; `Drop` not yet run).
    /// - Caller serializes writers (daemon initializes; runner updates counter).
    /// - `self.size() >= size_of::<SharedMemoryHeader>()` (enforced at open/create).
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn header_mut(&self) -> &mut SharedMemoryHeader {
        let ptr = self.ptr();
        debug_assert!(!ptr.is_null() && ptr != libc::MAP_FAILED);
        debug_assert!(self.size() >= std::mem::size_of::<SharedMemoryHeader>());
        // SAFETY: caller upholds mapping lifetime and single-writer protocol.
        unsafe { &mut *(ptr as *mut SharedMemoryHeader) }
    }

    /// Bounds-checked cell view. Rejects bad magic / dims that would exceed the map.
    ///
    /// # Safety
    /// Region must be mapped; length is validated against `self.size`. Concurrent
    /// mutation of header dims while this slice is live is undefined.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn cells_mut(&self) -> Result<&mut [FfiTerminalCell], String> {
        // SAFETY: same mapping invariants as `header_mut`.
        let header = unsafe { self.header_mut() };
        if header.magic != 0 && header.magic != SHM_MAGIC {
            return Err(format!(
                "shm header magic {:#x} != expected {:#x}",
                header.magic, SHM_MAGIC
            ));
        }
        let cols = header.cols as usize;
        let rows = header.rows as usize;
        let count = cols
            .checked_mul(rows)
            .ok_or_else(|| "shm header cell count overflow".to_string())?;
        let header_sz = std::mem::size_of::<SharedMemoryHeader>();
        let cell_sz = std::mem::size_of::<FfiTerminalCell>();
        let needed = header_sz
            .checked_add(
                count
                    .checked_mul(cell_sz)
                    .ok_or_else(|| "shm cell byte count overflow".to_string())?,
            )
            .ok_or_else(|| "shm size overflow".to_string())?;
        if needed > self.size() {
            return Err(format!(
                "shm header dims {cols}x{rows} need {needed} bytes, map is {}",
                self.size()
            ));
        }
        // SAFETY: `needed <= self.size`; cells begin immediately after the header.
        let cells_ptr = unsafe { (self.ptr() as *mut u8).add(header_sz) as *mut FfiTerminalCell };
        Ok(unsafe { std::slice::from_raw_parts_mut(cells_ptr, count) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_safety::is_valid_shm_name;

    /// `cells_mut` is the one place that hands a caller a slice into the
    /// mapping, so its bounds check is the thing most worth pinning: a
    /// header claiming more cells than the map can hold must be refused
    /// rather than read past the end.
    #[test]
    fn cells_mut_refuses_a_header_larger_than_the_mapping() {
        let name = format!("/idle-shm-cells-test-{}", std::process::id());
        assert!(
            is_valid_shm_name(&name),
            "test name rejected by the validator"
        );
        let hdr = std::mem::size_of::<SharedMemoryHeader>();
        let shm = SharedMemory::create(&name, hdr * 2).expect("create failed");

        // SAFETY: the mapping is live for the duration of `shm`, and this
        // test is the only writer.
        let header = unsafe { shm.header_mut() };
        header.magic = SHM_MAGIC;
        // Claim 1M cells into a map that holds at most a couple.
        header.rows = 1024;
        header.cols = 1024;

        // SAFETY: same invariants; the call is expected to fail its own
        // bounds check rather than read out of range.
        let res = unsafe { shm.cells_mut() };
        assert!(
            res.is_err(),
            "oversized header must be rejected, got {:?}",
            res.map(|s| s.len())
        );
    }
}
