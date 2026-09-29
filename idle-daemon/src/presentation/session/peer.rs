// SPDX-License-Identifier: MIT

//! UDS peer identity: XDG runtime dir + SO_PEERCRED checks.

use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Child;

/// Require user-private runtime dir — never fall back to world-writable `/tmp`.
pub fn runtime_socket_dir() -> Result<PathBuf, String> {
    let dir = std::env::var("XDG_RUNTIME_DIR").map_err(|_| {
        "XDG_RUNTIME_DIR is required for IPC sockets (refusing /tmp fallback)".to_string()
    })?;
    let p = PathBuf::from(dir);
    if !p.is_dir() {
        return Err(format!(
            "XDG_RUNTIME_DIR is not a directory: {}",
            p.display()
        ));
    }
    Ok(p)
}

fn peer_ucred(stream: &UnixStream) -> Result<libc::ucred, String> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::from_mut(&mut cred).cast::<libc::c_void>(),
            std::ptr::from_mut(&mut len),
        )
    };
    if rc != 0 {
        return Err(format!(
            "SO_PEERCRED failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(cred)
}

pub fn require_child_peer(stream: &UnixStream, child: &Child) -> Result<(), String> {
    let cred = peer_ucred(stream)?;
    let our_uid = unsafe { libc::geteuid() };
    if cred.uid != our_uid {
        return Err(format!(
            "IPC peer uid {} != our euid {}; rejecting",
            cred.uid, our_uid
        ));
    }
    let child_pid = child.id();
    if cred.pid as u32 != child_pid {
        return Err(format!(
            "IPC peer pid {} != child pid {}; rejecting (accept race?)",
            cred.pid, child_pid
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_socket_dir_requires_xdg() {
        // XDG_RUNTIME_DIR is shared with pidfile tests — serialize.
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
        // SAFETY: test isolation; holds TEST_ENV_LOCK.
        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        assert!(runtime_socket_dir().is_err());
        let dir = std::env::temp_dir().join(format!("idle-xdg-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &dir);
        }
        assert!(runtime_socket_dir().is_ok());
        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
