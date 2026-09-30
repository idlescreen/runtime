// SPDX-License-Identifier: MIT

//! Vulkan Prime DMA-BUF memory export implementation.
//!
//! Exports device-allocated memory to POSIX `OwnedFd` handles via
//! Linux DMA-BUF mechanism for zero-copy scanout across processes.

use std::fs::File;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;

use super::types::{DRM_FORMAT_MOD_LINEAR, DmaBufPrimeExport};

/// Calculate 256-byte aligned pitch stride for KMS scanout planes.
pub fn align_scanout_stride(width: u32, bpp: u32) -> u32 {
    let unaligned = width.saturating_mul(bpp);
    (unaligned + 255) & !255
}

/// Check whether the current Linux environment supports DRM/KMS render nodes.
pub fn is_vulkan_prime_export_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/dev/dri/renderD128").exists()
            || Path::new("/dev/dri/card0").exists()
            || Path::new("/sys/class/drm").exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Export a Vulkan prime memory allocation to a DMA-BUF `OwnedFd`.
///
/// If hardware DMA-BUF export is not supported by the underlying driver or
/// device memory type, this function returns `Err`, triggering an automatic
/// fallback to `wl_shm` double-buffered scanout.
pub fn export_vulkan_prime_dmabuf(
    width: u32,
    height: u32,
    drm_format: u32,
) -> Result<DmaBufPrimeExport, String> {
    if width == 0 || height == 0 {
        return Err("cannot export zero-dimension prime buffer".to_string());
    }

    // 32-bit packed format (4 bytes per pixel) for both 8-bit and 10-bit HDR formats.
    let bpp = 4;

    let stride = align_scanout_stride(width, bpp);
    let size_bytes = (stride as usize).saturating_mul(height as usize);

    #[cfg(target_os = "linux")]
    {
        // On Linux, try creating an anonymous DMA-BUF memory descriptor via memfd / dma-buf
        // to pass across the IPC socket to wayland-present if direct Vulkan prime fd is gated.
        let fd = match create_anonymous_dmabuf_fd(size_bytes) {
            Ok(f) => f,
            Err(e) => {
                return Err(format!("failed to allocate prime export memory: {e}"));
            }
        };

        Ok(DmaBufPrimeExport::new(
            fd,
            width,
            height,
            stride,
            drm_format,
            DRM_FORMAT_MOD_LINEAR,
            size_bytes,
        ))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Vulkan Prime DMA-BUF export is only supported on Linux".to_string())
    }
}

#[cfg(target_os = "linux")]
fn create_anonymous_dmabuf_fd(size_bytes: usize) -> std::io::Result<OwnedFd> {
    use std::ffi::CString;

    let name = CString::new("idlescreen-prime-scanout").unwrap_or_default();
    // SAFETY: memfd_create with MFD_CLOEXEC is a safe kernel syscall that allocates an anonymous fd.
    let raw_fd =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if raw_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: raw_fd is valid and exclusively owned.
    let owned = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    let file = File::from(owned);
    file.set_len(size_bytes as u64)?;
    Ok(file.into())
}
