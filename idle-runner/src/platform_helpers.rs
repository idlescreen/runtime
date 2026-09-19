//! Linux terminal/screen helpers used by the screensaver runner.
//! (Windows support has been removed; this is now Linux-only for the supported
//! distros: Debian family, Red Hat family, Gentoo, Arch.)

// ---------------------------------------------------------------------------
// Monitor refresh rate
// ---------------------------------------------------------------------------

pub fn get_monitor_refresh_rate() -> u32 {
    120 // Reasonable default for terminal-based screensavers on Linux
}

// ---------------------------------------------------------------------------
// Terminal size
// ---------------------------------------------------------------------------

pub fn get_terminal_size() -> (usize, usize) {
    // crossterm::terminal::size() equivalent: TIOCGWINSZ on stdout.
    #[repr(C)]
    struct WinSize {
        ws_row: libc::c_ushort,
        ws_col: libc::c_ushort,
        ws_xpixel: libc::c_ushort,
        ws_ypixel: libc::c_ushort,
    }
    let mut ws = WinSize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if ok == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
        (ws.ws_col as usize, ws.ws_row as usize)
    } else {
        (80, 24)
    }
}

// ---------------------------------------------------------------------------
// Mouse activity
// ---------------------------------------------------------------------------

pub fn check_mouse_activity(_initial_pos: &mut Option<(i32, i32)>) -> bool {
    false // Mouse activity detection not needed for these fullscreen terminal savers
}

// ---------------------------------------------------------------------------
// Keypress detection
// ---------------------------------------------------------------------------

/// `poll(fd, 0)` for POLLIN — split out so tests can exercise it on pipes
/// without touching the process's real stdin.
fn fd_ready(fd: libc::c_int) -> bool {
    let mut fds = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut fds, 1, 0) };
    ready > 0 && fds.revents & libc::POLLIN != 0
}

pub fn check_keypress() -> bool {
    // crossterm event::poll(0)+read() equivalent: a fullscreen saver treats any
    // pending stdin byte as a quit keypress.
    if !fd_ready(libc::STDIN_FILENO) {
        return false;
    }
    let mut buf = [0u8; 64];
    unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), buf.len()) > 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_rate_is_stable_positive_default() {
        assert_eq!(get_monitor_refresh_rate(), 120);
    }

    #[test]
    fn terminal_size_is_fallback_or_real() {
        let (cols, rows) = get_terminal_size();
        // Either the real TTY size or the documented (80, 24) fallback —
        // never zero or nonsense.
        assert!(cols > 0 && rows > 0);
        assert!(cols <= 1000 && rows <= 1000);
        if (cols, rows) != (80, 24) {
            // On a real TTY the values are plausible terminal dims.
            assert!(cols >= 20 && rows >= 5);
        }
    }

    #[test]
    fn mouse_activity_is_stably_inert() {
        let mut pos = Some((10, 20));
        assert!(!check_mouse_activity(&mut pos));
        assert_eq!(pos, Some((10, 20)), "must not mutate tracked position");
        let mut none = None;
        assert!(!check_mouse_activity(&mut none));
        assert_eq!(none, None);
    }

    #[test]
    fn fd_ready_reflects_pipe_data() {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (r, w) = (fds[0], fds[1]);

        // Empty pipe: not ready.
        assert!(!fd_ready(r));

        // After writing a byte: ready.
        assert_eq!(unsafe { libc::write(w, b"k".as_ptr().cast(), 1) }, 1);
        assert!(fd_ready(r));

        // Drain it: back to not-ready.
        let mut buf = [0u8; 8];
        assert_eq!(unsafe { libc::read(r, buf.as_mut_ptr().cast(), 8) }, 1);
        assert!(!fd_ready(r));

        unsafe {
            libc::close(r);
            libc::close(w);
        }
    }

    #[test]
    fn fd_ready_false_on_invalid_fd() {
        assert!(!fd_ready(-1));
    }
}
