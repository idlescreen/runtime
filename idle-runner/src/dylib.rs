// SPDX-License-Identifier: MIT

//! Minimal `libloading` replacement: `dlopen`/`dlsym`/`dlclose` over libc.
//! Covers the two features the runner uses — `Library::new(path)` and
//! `lib.get::<T>(name)` — with the same error surface (`dlerror()` text).

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// dlopen/dlsym failure carrying the `dlerror()` message.
#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn last_error(fallback: &str) -> Error {
    unsafe {
        let e = libc::dlerror();
        if e.is_null() {
            Error(fallback.to_string())
        } else {
            Error(std::ffi::CStr::from_ptr(e).to_string_lossy().into_owned())
        }
    }
}

/// A dynamically loaded shared object (libloading `Library` equivalent).
/// `dlsym` is thread-safe per POSIX, so the handle may be shared.
pub struct Library {
    handle: *mut std::ffi::c_void,
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}

impl Library {
    /// `dlopen(path, RTLD_NOW | RTLD_LOCAL)`.
    ///
    /// # Safety
    /// Loading a library runs its ELF constructors — same contract as
    /// `libloading::Library::new`.
    pub unsafe fn new<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let c_path = CString::new(path.as_ref().as_os_str().as_bytes())
            .map_err(|e| Error(format!("invalid library path: {e}")))?;
        let handle = unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            Err(last_error("dlopen failed"))
        } else {
            Ok(Self { handle })
        }
    }

    /// `dlsym` for `symbol` (a NUL is appended if missing — `OPS_SYMBOL` and
    /// friends are plain `b"..."` slices). The returned `T` is a plain value
    /// (typically a fn pointer); callers keep the `Library` alive as long as
    /// the symbol may be used, same as libloading's `Symbol` borrow contract.
    ///
    /// # Safety
    /// `T` must be a pointer-sized type matching the symbol's real type.
    pub unsafe fn get<T: Copy>(&self, symbol: &[u8]) -> Result<T, Error> {
        assert_eq!(
            std::mem::size_of::<T>(),
            std::mem::size_of::<*mut std::ffi::c_void>(),
            "Library::get requires a pointer-sized type"
        );
        let trimmed = match symbol.iter().position(|&b| b == 0) {
            Some(i) => &symbol[..i],
            None => symbol,
        };
        let c_sym =
            CString::new(trimmed).map_err(|e| Error(format!("invalid symbol name: {e}")))?;
        unsafe { libc::dlerror() }; // clear stale error
        let ptr = unsafe { libc::dlsym(self.handle, c_sym.as_ptr()) };
        unsafe {
            let e = libc::dlerror();
            if !e.is_null() {
                return Err(Error(
                    std::ffi::CStr::from_ptr(e).to_string_lossy().into_owned(),
                ));
            }
        }
        if ptr.is_null() {
            return Err(Error(format!(
                "dlsym: symbol '{}' not found",
                String::from_utf8_lossy(trimmed)
            )));
        }
        Ok(unsafe { std::mem::transmute_copy::<*mut std::ffi::c_void, T>(&ptr) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.handle) };
    }
}
