// SPDX-License-Identifier: MIT

// perf: T2 · bench: hot_path · on-demand only; not gated
//! Epoll fd setup + eventfd draining for the overlay event thread.
//!
//! The Wayland socket + the daemon's self-wake eventfd are the two
//! fds the event loop multiplexes. Each helper is a one-purpose
//! page: see `make_epoll`, `epoll_ctl_add`, and `drain_eventfd`.
//!
//! These three are `pub` rather than `pub(super)` solely so
//! `overlay/mod.rs::bench_exports` can re-export them to the
//! `hot_path` bench target. `mod overlay` is private in `lib.rs`, so
//! `pub` here adds no reachable path on its own — see RULES.md §5.

// Create a new epoll fd. CLOEXEC so a child fork doesn't inherit it.
pub fn make_epoll() -> Result<libc::c_int, &'static str> {
    // SAFETY: epoll_create1 with EPOLL_CLOEXEC returns a fresh fd.
    let raw = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if raw < 0 {
        idle_log::error!(
            error = %std::io::Error::last_os_error(),
            "wayland-present: epoll_create1 failed"
        );
        return Err("epoll_create1 failed");
    }
    Ok(raw)
}

/// Add `fd` to the epoll set with the given event mask and a stable
/// tag (the `u64` slot in `epoll_event`). The tag lets us route the
/// wakeup to the right handler in the event loop.
///
/// # Safety
/// `fd` must be a valid descriptor (we own it or it is the Wayland
/// socket lifetime). `event` is a valid `epoll_event` struct; the
/// pointer is taken by `libc::epoll_ctl` only inside this call.
pub fn epoll_ctl_add(
    epoll_fd: libc::c_int,
    fd: libc::c_int,
    mask: libc::c_int,
    tag: u64,
) -> Result<(), &'static str> {
    let mut event = libc::epoll_event {
        events: mask as u32,
        u64: tag,
    };
    let rc = unsafe { libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, fd, &mut event) };
    if rc < 0 {
        idle_log::error!(
            error = %std::io::Error::last_os_error(),
            fd,
            "wayland-present: epoll_ctl ADD failed"
        );
        return Err("epoll_ctl ADD failed");
    }
    Ok(())
}

/// Drain an eventfd by reading 8 bytes (the counter) until EAGAIN.
pub fn drain_eventfd(fd: libc::c_int) {
    let mut buf = [0u8; 8];
    loop {
        // SAFETY: `fd` is a valid eventfd; `buf` is a valid 8-byte
        // stack buffer. EAGAIN (EWOULDBLOCK) means the counter is 0 —
        // we exit the loop.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return;
            }
            // Any other error is fatal-ish; log and exit the loop to
            // avoid spinning on the same error forever.
            idle_log::warn!(
                error = %err,
                fd,
                "wayland-present: eventfd read failed"
            );
            return;
        }
        if n == 0 {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_epoll_returns_valid_fd() {
        // CLOEXEC epoll fd must be a non-negative integer and
        // refer to a live kernel object (epoll_ctl on it must
        // succeed for a valid fd).
        let ep = make_epoll().expect("epoll_create1 must succeed on Linux");
        assert!(ep >= 0);
        // SAFETY: `ep` was just created in this test, so it's a
        // valid epoll fd.
        let rc = unsafe { libc::close(ep) };
        assert_eq!(rc, 0, "freshly-created epoll fd must close cleanly");
    }

    #[test]
    fn epoll_ctl_add_then_close() {
        // Round-trip: create epoll, add a self-pipe, close the
        // pipe, close the epoll. The point is to exercise the
        // syscall sequence without UB.
        let ep = make_epoll().expect("epoll_create1");
        let mut fds = [0i32; 2];
        // SAFETY: pipe2 with O_NONBLOCK | O_CLOEXEC is a standard
        // self-pipe construction; the fds array is valid for 2
        // outputs.
        let rc = unsafe {
            libc::pipe2(
                fds.as_mut_ptr() as *mut _,
                libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        assert_eq!(rc, 0);
        epoll_ctl_add(ep, fds[0], libc::EPOLLIN, 7).expect("add to epoll");
        // SAFETY: fds are live descriptors we own.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
            libc::close(ep);
        }
    }
}
