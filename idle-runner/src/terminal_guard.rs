//! RawTerminalGuard: enters raw terminal mode on startup, restores on drop.
//! Linux-only (Windows support stripped per multi-distro Linux plan).

mod linux_impl {
    pub struct RawTerminalGuard {
        pub original_termios: libc::termios,
    }

    impl RawTerminalGuard {
        pub fn enable() -> Option<Self> {
            unsafe {
                let mut termios: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) != 0 {
                    return None;
                }
                let original_termios = termios;
                libc::cfmakeraw(&mut termios);
                if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios) != 0 {
                    return None;
                }
                use std::io::Write;
                print!("\x1b[?1003h");
                let _ = std::io::stdout().flush();

                // Pure ANSI + raw mode for Linux terminals (xterm, etc.).
                // Works under Wayland via XWayland or native terminals.
                Some(Self { original_termios })
            }
        }
    }

    impl Drop for RawTerminalGuard {
        fn drop(&mut self) {
            unsafe {
                use std::io::Write;
                print!("\x1b[?1003l");
                let _ = std::io::stdout().flush();
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original_termios);
            }
        }
    }
}

pub use linux_impl::RawTerminalGuard;
