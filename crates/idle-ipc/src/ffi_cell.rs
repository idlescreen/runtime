// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

use idle_api::TerminalCell;

/// Hard cap on a single grid axis (cols or rows) for IPC shared memory.
pub const MAX_GRID_DIM: usize = 4096;

/// Hard cap on total cells (`cols * rows`) to bound SHM / mmap DoS.
pub const MAX_GRID_CELLS: usize = 512 * 512;

/// FFI-safe representation of `TerminalCell` for shared memory communication.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FfiTerminalCell {
    pub ch: u32,
    pub fg_r: u8,
    pub fg_g: u8,
    pub fg_b: u8,
    pub bg_r: u8,
    pub bg_g: u8,
    pub bg_b: u8,
    pub bold: u8,
}

impl From<TerminalCell> for FfiTerminalCell {
    fn from(cell: TerminalCell) -> Self {
        Self {
            ch: cell.ch as u32,
            fg_r: cell.fg.0,
            fg_g: cell.fg.1,
            fg_b: cell.fg.2,
            bg_r: cell.bg.0,
            bg_g: cell.bg.1,
            bg_b: cell.bg.2,
            bold: if cell.bold { 1 } else { 0 },
        }
    }
}

impl From<FfiTerminalCell> for TerminalCell {
    fn from(ffi: FfiTerminalCell) -> Self {
        Self {
            ch: std::char::from_u32(ffi.ch).unwrap_or(' '),
            fg: (ffi.fg_r, ffi.fg_g, ffi.fg_b),
            bg: (ffi.bg_r, ffi.bg_g, ffi.bg_b),
            bold: ffi.bold != 0,
        }
    }
}

#[repr(C)]
pub struct SharedMemoryHeader {
    pub magic: u32,
    pub cols: u32,
    pub rows: u32,
    pub frame_counter: u64,
}

pub const SHM_MAGIC: u32 = 0x54524e43;

/// Reject zero, oversized, or overflowing grid dimensions before SHM allocate.
pub fn validate_grid_dims(cols: usize, rows: usize) -> Result<(), &'static str> {
    if cols == 0 || rows == 0 {
        return Err("grid dimensions must be non-zero");
    }
    if cols > MAX_GRID_DIM || rows > MAX_GRID_DIM {
        return Err("grid dimension exceeds maximum");
    }
    match cols.checked_mul(rows) {
        Some(n) if n <= MAX_GRID_CELLS => Ok(()),
        Some(_) => Err("grid cell count exceeds maximum"),
        None => Err("grid cell count overflow"),
    }
}

/// Byte size of header + `cols * rows` cells. `None` on overflow.
pub fn compute_shm_size(cols: usize, rows: usize) -> Option<usize> {
    let cells = cols.checked_mul(rows)?;
    let cell_bytes = cells.checked_mul(std::mem::size_of::<FfiTerminalCell>())?;
    std::mem::size_of::<SharedMemoryHeader>().checked_add(cell_bytes)
}
