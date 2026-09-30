// SPDX-License-Identifier: MIT

//! Linux DMA-BUF protocol (`zwp_linux_dmabuf_v1`) integration.
//!
//! Enables zero-copy hardware buffer sharing from GPU renderers into the
//! Wayland compositor and manages buffer pool recycling with `wl_buffer.release`.
//! Provides multi-buffered scanout pipelines.

pub mod importer;
pub mod pool;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use importer::import_dmabuf_buffer;
#[allow(unused_imports)]
pub use pool::{DmaBufPool, DmaBufSlot};
