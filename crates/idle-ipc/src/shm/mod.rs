// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

use std::ffi::CString;
use std::ptr;

use crate::ffi_cell::SharedMemoryHeader;
use crate::path_safety::is_valid_shm_name;

pub const F_SEAL_SEAL: libc::c_int = 0x0001;
pub const F_SEAL_SHRINK: libc::c_int = 0x0002;
pub const F_SEAL_GROW: libc::c_int = 0x0004;
pub const F_ADD_SEALS: libc::c_int = 1033;
pub const F_GET_SEALS: libc::c_int = 1034;

/// POSIX or sealed shared-memory region used for terminal-cell IPC.
///
/// Ownership: exclusive over `fd` + `mmap` mapping. `Drop` always `munmap`s,
/// `close`s, and (when `is_owner`) `shm_unlink`s.
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
    fn validate_size(size: usize) -> Result<(), String> {
        if size < std::mem::size_of::<SharedMemoryHeader>() || size > 64 * 1024 * 1024 {
            Err(format!("shm size out of range: {size}"))
        } else {
            Ok(())
        }
    }

    unsafe fn map_and_build(
        name: String,
        fd: libc::c_int,
        size: usize,
        is_owner: bool,
    ) -> Result<Self, String> {
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
            // SAFETY: fd is owned; unlink named object if we are owner.
            unsafe {
                libc::close(fd);
                if is_owner && let Ok(c) = CString::new(name.as_str()) {
                    libc::shm_unlink(c.as_ptr());
                }
            }
            Err(format!("mmap failed: {err}"))
        } else {
            Ok(Self {
                name,
                fd,
                ptr,
                size,
                is_owner,
            })
        }
    }

    pub fn create(name: &str, size: usize) -> Result<Self, String> {
        if !is_valid_shm_name(name) {
            return Err(format!("invalid shm name: {name}"));
        }
        Self::validate_size(size)?;
        let c_name = CString::new(name).map_err(|e| e.to_string())?;

        // SAFETY: c_name is valid CString; name validated above.
        let mut fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
                0o600,
            )
        };
        if fd < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
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

        // SAFETY: fd is open; size fits in off_t (capped at 64 MiB above).
        if unsafe { libc::ftruncate(fd, size as libc::off_t) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(fd);
                libc::shm_unlink(c_name.as_ptr());
            }
            return Err(format!("ftruncate failed: {err}"));
        }

        unsafe { Self::map_and_build(name.to_string(), fd, size, true) }
    }

    pub fn open(name: &str, size: usize) -> Result<Self, String> {
        if !is_valid_shm_name(name) {
            return Err(format!("invalid shm name: {name}"));
        }
        Self::validate_size(size)?;
        let c_name = CString::new(name).map_err(|e| e.to_string())?;

        // SAFETY: open existing named object; name validated above.
        let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDWR, 0) };
        if fd < 0 {
            return Err(format!(
                "shm_open (open) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        unsafe { Self::map_and_build(name.to_string(), fd, size, false) }
    }

    /// Create an anonymous shared memory object backed by memfd with sealing.
    pub fn create_sealed(name: &str, size: usize) -> Result<Self, String> {
        Self::validate_size(size)?;
        let c_name = CString::new(name).map_err(|e| e.to_string())?;
        // SAFETY: memfd_create with MFD_ALLOW_SEALING | MFD_CLOEXEC.
        let fd = unsafe {
            libc::memfd_create(c_name.as_ptr(), libc::MFD_ALLOW_SEALING | libc::MFD_CLOEXEC)
        };
        if fd < 0 {
            return Err(format!(
                "memfd_create failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: fd is valid and open.
        if unsafe { libc::ftruncate(fd, size as libc::off_t) } < 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: clean up fd on truncation failure.
            unsafe { libc::close(fd) };
            return Err(format!("ftruncate failed: {err}"));
        }
        // Apply seals: prevent shrinking, growing, and further sealing.
        // SAFETY: fd is an open memfd created with MFD_ALLOW_SEALING.
        let seals = F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL;
        if unsafe { libc::fcntl(fd, F_ADD_SEALS, seals) } < 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: clean up fd on sealing failure.
            unsafe { libc::close(fd) };
            return Err(format!("fcntl F_ADD_SEALS failed: {err}"));
        }
        // SAFETY: fd is sealed memfd ready for mapping.
        unsafe { Self::map_and_build(name.to_string(), fd, size, false) }
    }

    /// Map a pre-sealed shared memory fd (e.g. received via IPC / inheritance).
    ///
    /// Consumes ownership of `fd` on both success and failure.
    pub fn from_sealed_fd(fd: libc::c_int, size: usize) -> Result<Self, String> {
        let close_on_err = |e: String| {
            if fd >= 0 {
                // SAFETY: caller transferred fd ownership to from_sealed_fd.
                unsafe { libc::close(fd) };
            }
            e
        };
        Self::validate_size(size).map_err(close_on_err)?;
        if fd < 0 {
            return Err("invalid sealed fd".to_string());
        }
        // SAFETY: query active seals on fd.
        let seals = unsafe { libc::fcntl(fd, F_GET_SEALS) };
        if seals < 0 {
            let err = std::io::Error::last_os_error();
            return Err(close_on_err(format!("fcntl F_GET_SEALS failed: {err}")));
        }
        let required = F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL;
        if (seals & required) != required {
            return Err(close_on_err(format!(
                "fd missing required seals (got {seals:#x}, required {required:#x})"
            )));
        }
        // SAFETY: map_and_build handles mmap and closes fd on map failure.
        unsafe { Self::map_and_build(String::new(), fd, size, false) }
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

pub mod cells;

#[cfg(test)]
mod stress;

#[cfg(test)]
mod tests;
