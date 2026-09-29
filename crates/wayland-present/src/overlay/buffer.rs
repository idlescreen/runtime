// SPDX-License-Identifier: MIT

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::ptr;

use wayland_client::QueueHandle;
use wayland_client::protocol::{wl_buffer, wl_shm};

use super::state::SessionState;

/// mmap'd ARGB8888 buffer backed by a sealed memfd + wl_buffer.
///
/// Drop order: `munmap` the mapping, then `OwnedFd` closes the memfd, then the
/// Wayland proxy drops/`destroy`s the `wl_buffer`. Pool is destroyed after
/// `create_buffer` returns (Wayland allows buffer lifetime independent of pool).
pub struct MappedBuffer {
    pub wl_buffer: wl_buffer::WlBuffer,
    _memfd: OwnedFd,
    mapped_ptr: *mut u8,
    mapped_len: usize,
    width: u32,
    height: u32,
}

impl Drop for MappedBuffer {
    fn drop(&mut self) {
        self.wl_buffer.destroy();
        if !self.mapped_ptr.is_null() && self.mapped_len > 0 {
            // SAFETY: `mapped_ptr`/`mapped_len` come from a successful MAP_SHARED
            // mmap in `allocate_buffer`; we null them after munmap to make Drop idempotent.
            unsafe {
                libc::munmap(self.mapped_ptr.cast(), self.mapped_len);
            }
            self.mapped_ptr = ptr::null_mut();
            self.mapped_len = 0;
        }
    }
}

impl MappedBuffer {
    pub fn write_pixels(&mut self, pixels: &[u8]) -> bool {
        let stride = self.width.saturating_mul(4);
        let length = stride.saturating_mul(self.height) as usize;
        if pixels.len() < length || length > self.mapped_len {
            return false;
        }

        // SAFETY: mapping is live until Drop; `length <= mapped_len` checked above.
        unsafe {
            let mapped = std::slice::from_raw_parts_mut(self.mapped_ptr, length);
            mapped.copy_from_slice(&pixels[..length]);
        }
        true
    }
}

pub fn create_solid_buffer(
    shm: &wl_shm::WlShm,
    queue: &QueueHandle<SessionState>,
    width: u32,
    height: u32,
    color: [u8; 3],
) -> Option<MappedBuffer> {
    let buffer = allocate_buffer(shm, queue, width, height)?;
    // SAFETY: buffer mapping is exclusive to this value until returned.
    unsafe {
        let mapped = std::slice::from_raw_parts_mut(buffer.mapped_ptr, buffer.mapped_len);
        fill_argb8888(mapped, color);
    }
    Some(buffer)
}

pub fn ensure_frame_buffer(
    existing: &mut Option<MappedBuffer>,
    shm: &wl_shm::WlShm,
    queue: &QueueHandle<SessionState>,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }

    let needs_new = existing
        .as_ref()
        .is_none_or(|buffer| buffer.width != width || buffer.height != height);

    if needs_new {
        *existing = allocate_buffer(shm, queue, width, height);
    }

    let Some(buffer) = existing.as_mut() else {
        return false;
    };
    if buffer.write_pixels(pixels) {
        return true;
    }

    *existing = allocate_buffer(shm, queue, width, height);
    existing
        .as_mut()
        .is_some_and(|buffer| buffer.write_pixels(pixels))
}

fn allocate_buffer(
    shm: &wl_shm::WlShm,
    queue: &QueueHandle<SessionState>,
    width: u32,
    height: u32,
) -> Option<MappedBuffer> {
    if width == 0 || height == 0 {
        return None;
    }

    let stride = width.saturating_mul(4);
    let length = stride.saturating_mul(height) as usize;
    let memfd = create_memfd(length)?;

    // SAFETY: memfd is sized to `length`; MAP_SHARED for compositor readback.
    // On MAP_FAILED we return None; OwnedFd drops and closes the memfd.
    let mapped_ptr = unsafe {
        let address = libc::mmap(
            ptr::null_mut(),
            length,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            memfd.as_fd().as_raw_fd(),
            0,
        );
        if address == libc::MAP_FAILED {
            return None;
        }
        address as *mut u8
    };

    // Pool is temporary: create buffer then drop pool (Wayland keeps buffer valid).
    let pool = shm.create_pool(memfd.as_fd(), length as i32, queue, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        queue,
        (),
    );
    pool.destroy();

    Some(MappedBuffer {
        wl_buffer: buffer,
        _memfd: memfd,
        mapped_ptr,
        mapped_len: length,
        width,
        height,
    })
}

fn create_memfd(length: usize) -> Option<OwnedFd> {
    // SAFETY: memfd_create with CLOEXEC; name is a static CStr.
    let fd = unsafe { libc::memfd_create(c"idle-overlay".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return None;
    }

    // SAFETY: `fd` is a fresh owned descriptor from memfd_create.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: ftruncate to pixel buffer length; OwnedFd closes fd on failure via Drop.
    if unsafe { libc::ftruncate(owned.as_fd().as_raw_fd(), length as i64) } != 0 {
        return None;
    }

    Some(owned)
}

fn fill_argb8888(pixels: &mut [u8], color: [u8; 3]) {
    let mut offset = 0;
    while offset + 3 < pixels.len() {
        pixels[offset] = color[2];
        pixels[offset + 1] = color[1];
        pixels[offset + 2] = color[0];
        pixels[offset + 3] = 0xFF;
        offset += 4;
    }
}
