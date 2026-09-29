// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Shared memory layout and control protocol for out-of-process screensaver execution.

pub mod ffi_cell;
pub mod path_safety;
pub mod protocol;
pub mod shm;
pub mod shm_cells;

#[cfg(test)]
mod shm_stress;

pub use ffi_cell::{
    FfiTerminalCell, MAX_GRID_CELLS, MAX_GRID_DIM, SHM_MAGIC, SharedMemoryHeader, compute_shm_size,
    validate_grid_dims,
};
pub use path_safety::{is_plausible_socket_path, is_valid_shm_name};
pub use protocol::{IpcCommand, IpcResponse};
pub use shm::SharedMemory;

#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ipc_proptests.rs"]
mod proptests;
