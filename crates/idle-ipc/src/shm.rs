// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

use std::ffi::CString;
use std::ptr;

use crate::ffi_cell::SharedMemoryHeader;
use crate::path_safety::is_valid_shm_name;

/// POSIX shared-memory region used for terminal-cell IPC.
///
/// Ownership: exclusive over `fd` + `mmap` mapping. `Drop` always `munmap`s,
/// `close`s, and (when `is_owner`) `shm_unlink`s. Named SHM only — memfd is not
/// used because the OOP runner re-opens by name.
pub struct SharedMemory {
    name: String,
    fd: libc::c_int,
    ptr: *mut libc::c_void,
    size: usize,
    is_owner: bool,
}

// SAFETY: `SharedMemory` is the exclusive owner of the fd and mapped pages.
// Moving across threads transfers that ownership; concurrent access to the
// mapping is a protocol concern (single writer per field), not a Send concern.
unsafe impl Send for SharedMemory {}

impl SharedMemory {
    pub fn create(name: &str, size: usize) -> Result<Self, String> {
        if !is_valid_shm_name(name) {
            return Err(format!("invalid shm name: {name}"));
        }
        if size < std::mem::size_of::<SharedMemoryHeader>() || size > 64 * 1024 * 1024 {
            return Err(format!("shm size out of range: {size}"));
        }
        let c_name = CString::new(name).map_err(|e| e.to_string())?;

        // Named POSIX SHM only: the IPC child re-opens by name (`SharedMemory::open`).
        // O_EXCL + 0600: keep the object owner-private. Do NOT unlink before
        // the first open — an unconditional unlink could drop a live peer's
        // object from under its mapping. Only on EEXIST (stale object left by
        // a dead daemon; names are pid-scoped) do we unlink and retry once.
        // SAFETY: `c_name` is a valid CString; name validated above.
        let mut fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
                0o600,
            )
        };
        if fd < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
            // SAFETY: unlink only the colliding stale name, then a single
            // bounded retry. A second EEXIST means a racing peer legitimately
            // owns the name — fail closed below.
            unsafe {
                libc::shm_unlink(c_name.as_ptr());
            }
            fd = unsafe {
                libc::shm_open(
                    c_name.as_ptr(),
                    libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
                    0o600,
                )
            };
        }
        if fd < 0 {
            return Err(format!(
                "shm_open (create) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // SAFETY: `fd` is open; size fits in off_t (capped at 64 MiB above).
        if unsafe { libc::ftruncate(fd, size as libc::off_t) } < 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: clean up partially created object on size failure.
            unsafe {
                libc::close(fd);
                libc::shm_unlink(c_name.as_ptr());
            }
            return Err(format!("ftruncate failed: {err}"));
        }

        // SAFETY: MAP_SHARED over the full sized object; fail closed on MAP_FAILED.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            // SAFETY: release fd + name if mapping failed.
            unsafe {
                libc::close(fd);
                libc::shm_unlink(c_name.as_ptr());
            }
            return Err(format!("mmap failed: {err}"));
        }

        Ok(Self {
            name: name.to_string(),
            fd,
            ptr,
            size,
            is_owner: true,
        })
    }

    pub fn open(name: &str, size: usize) -> Result<Self, String> {
        if !is_valid_shm_name(name) {
            return Err(format!("invalid shm name: {name}"));
        }
        if size < std::mem::size_of::<SharedMemoryHeader>() || size > 64 * 1024 * 1024 {
            return Err(format!("shm size out of range: {size}"));
        }
        let c_name = CString::new(name).map_err(|e| e.to_string())?;

        // SAFETY: open existing named object; name validated above.
        let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDWR, 0) };
        if fd < 0 {
            return Err(format!(
                "shm_open (open) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // SAFETY: MAP_SHARED; close fd on MAP_FAILED (non-owner does not unlink).
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            // SAFETY: fd is open and owned by this path only.
            unsafe {
                libc::close(fd);
            }
            return Err(format!("mmap failed: {err}"));
        }

        Ok(Self {
            name: name.to_string(),
            fd,
            ptr,
            size,
            is_owner: false,
        })
    }

    pub fn fd(&self) -> libc::c_int {
        self.fd
    }

    pub fn ptr(&self) -> *mut libc::c_void {
        self.ptr
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        // SAFETY: reverse of create/open — unmap, close fd, unlink if we own the name.
        // Null/`MAP_FAILED` and fd < 0 guard against double-free after partial init.
        unsafe {
            if !self.ptr.is_null() && self.ptr != libc::MAP_FAILED {
                libc::munmap(self.ptr, self.size);
                self.ptr = ptr::null_mut();
            }
            if self.fd >= 0 {
                libc::close(self.fd);
                self.fd = -1;
            }
            if self.is_owner
                && let Ok(c_name) = CString::new(self.name.as_str())
            {
                libc::shm_unlink(c_name.as_ptr());
            }
        }
    }
}

#[cfg(test)]
#[path = "shm_tests.rs"]
mod tests;
