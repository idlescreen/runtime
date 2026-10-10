// SPDX-License-Identifier: MIT

//! Systemd `NOTIFY_SOCKET` readiness notification backend.
//!
//! Supports both filesystem sockets (e.g. `/run/systemd/notify`) and Linux
//! abstract namespace sockets (paths starting with `@`).

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::Path;

#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;

/// Send a newline-terminated state datagram to `NOTIFY_SOCKET` if configured.
pub fn send_systemd_notify(state: &str) -> std::io::Result<bool> {
    let socket_env = match std::env::var_os("NOTIFY_SOCKET") {
        Some(val) => val,
        None => return Ok(false),
    };

    let bytes = socket_env.as_bytes();
    if bytes.is_empty() {
        return Ok(false);
    }

    send_to_socket_bytes(bytes, state.as_bytes())
}

/// Send raw payload to the specified UNIX datagram socket bytes.
///
/// If `target` starts with `@`, the leading byte is stripped and the address
/// is treated as a Linux abstract namespace socket. Otherwise, it is parsed
/// as a standard filesystem path.
pub fn send_to_socket_bytes(target: &[u8], payload: &[u8]) -> std::io::Result<bool> {
    let sock = UnixDatagram::unbound()?;

    if let Some(abstract_name) = target.strip_prefix(b"@") {
        #[cfg(target_os = "linux")]
        {
            let addr = SocketAddr::from_abstract_name(abstract_name)?;
            sock.send_to_addr(payload, &addr)?;
            Ok(true)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = abstract_name;
            idle_log::debug!("abstract NOTIFY_SOCKET is only supported on Linux");
            Ok(false)
        }
    } else {
        let path = Path::new(OsStr::from_bytes(target));
        let addr = SocketAddr::from_pathname(path)?;
        sock.send_to_addr(payload, &addr)?;
        Ok(true)
    }
}

/// Emit `READY=1\n` to systemd supervisor.
pub fn notify_ready() -> std::io::Result<bool> {
    send_systemd_notify("READY=1\n")
}

/// Emit `STOPPING=1\n` to systemd supervisor.
pub fn notify_stopping() -> std::io::Result<bool> {
    send_systemd_notify("STOPPING=1\n")
}

/// Emit `WATCHDOG=1\n` to systemd supervisor.
pub fn notify_watchdog() -> std::io::Result<bool> {
    send_systemd_notify("WATCHDOG=1\n")
}
