// SPDX-License-Identifier: MIT

//! File descriptor pipe readiness notification for s6 and dinit.
//!
//! Writes a newline character `\n` to the readiness descriptor (default fd 3)
//! and closes it to transition the service to the ready state.

use std::sync::atomic::{AtomicBool, Ordering};

/// Process-global guard ensuring the readiness pipe is notified and closed
/// at most once across the daemon lifecycle.
static FD_NOTIFIED: AtomicBool = AtomicBool::new(false);

/// Reset the single-shot notification guard for testing purposes.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    FD_NOTIFIED.store(false, Ordering::SeqCst);
}

fn is_fifo(fd: i32) -> bool {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat queries descriptor status into uninitialized stat struct.
    let res = unsafe { libc::fstat(fd, stat.as_mut_ptr()) };
    if res != 0 {
        return false;
    }
    // SAFETY: fstat returned 0, so stat is initialized.
    let stat = unsafe { stat.assume_init() };
    (stat.st_mode & libc::S_IFMT) == libc::S_IFIFO
}

/// Signal readiness on a specific file descriptor.
///
/// Validates descriptor liveness, writes `\n`, closes the descriptor,
/// and guards against collision with systemd socket activation.
pub fn notify_fd(fd: i32) -> std::io::Result<bool> {
    // Descriptors 0 (stdin), 1 (stdout), and 2 (stderr) are reserved for standard I/O.
    // Writing or closing them corrupts process streams. Readiness descriptors must be >= 3.
    if fd < 3 {
        idle_log::debug!(
            "refusing readiness notification on low/standard descriptor fd {fd} (< 3)"
        );
        return Ok(false);
    }

    // Safety check: do not touch fd if systemd socket activation is active
    if std::env::var_os("LISTEN_FDS").is_some() {
        idle_log::debug!("LISTEN_FDS detected; skipping readiness pipe on fd {fd}");
        return Ok(false);
    }

    if !is_fifo(fd) {
        return Ok(false);
    }

    // Write newline readiness byte to supervisor
    let buf = b"\n";
    let mut written = 0;
    while written < buf.len() {
        // SAFETY: fd is validated as an open FIFO descriptor.
        let res = unsafe { libc::write(fd, buf[written..].as_ptr().cast(), buf.len() - written) };
        if res <= 0 {
            if res < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
            }
            break;
        }
        written += res as usize;
    }

    // SAFETY: Closing the validated FIFO descriptor to signal EOF to the supervisor.
    unsafe {
        libc::close(fd);
    }

    if written == buf.len() {
        idle_log::info!("readiness signaled on fd {fd}");
        Ok(true)
    } else {
        idle_log::warn!("failed to write readiness newline to supervisor on fd {fd}");
        Ok(false)
    }
}

/// Signal readiness using the configured descriptor (from `NOTIFICATION_FD` or fd 3).
pub fn notify_ready() -> std::io::Result<bool> {
    if FD_NOTIFIED.swap(true, Ordering::SeqCst) {
        return Ok(false);
    }

    if let Some(fd) = std::env::var("NOTIFICATION_FD")
        .ok()
        .and_then(|val| val.parse::<i32>().ok())
    {
        return notify_fd(fd);
    }

    let init = idle_dbus::service::detect_init_system();
    if init == idle_dbus::service::InitSystem::S6 || init == idle_dbus::service::InitSystem::Dinit {
        return notify_fd(3);
    }

    Ok(false)
}
