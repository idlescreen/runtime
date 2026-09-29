use wayland_client::backend::WaylandError;

/// `prepare_read`/`read` can return WouldBlock (EAGAIN) when the socket was
/// already drained — that must not kill the presenter thread.
pub fn is_wayland_would_block(err: &WaylandError) -> bool {
    match err {
        WaylandError::Io(e) => {
            e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::Interrupted
        }
        WaylandError::Protocol(_) => false,
    }
}

#[cfg(test)]
mod would_block_tests {
    use super::is_wayland_would_block;
    use wayland_client::backend::WaylandError;

    #[test]
    fn eagain_is_would_block() {
        // Regression: EAGAIN killed presenter → daemon recovered in a loop.
        let err = WaylandError::Io(std::io::Error::from(std::io::ErrorKind::WouldBlock));
        assert!(is_wayland_would_block(&err));
    }

    #[test]
    fn interrupted_is_retryable() {
        let err = WaylandError::Io(std::io::Error::from(std::io::ErrorKind::Interrupted));
        assert!(is_wayland_would_block(&err));
    }

    #[test]
    fn protocol_error_is_fatal() {
        // Connection loss / abort must still tear down the thread.
        let err = WaylandError::Io(std::io::Error::from(std::io::ErrorKind::ConnectionAborted));
        assert!(!is_wayland_would_block(&err));
        let err2 = WaylandError::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        assert!(!is_wayland_would_block(&err2));
    }
}
