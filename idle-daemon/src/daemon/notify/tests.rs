// SPDX-License-Identifier: MIT

use super::*;
use std::io::Read;
use std::os::unix::net::UnixDatagram;

#[test]
fn test_systemd_missing_socket_is_noop() {
    let res = systemd::send_systemd_notify("READY=1\n");
    assert!(res.is_ok());
}

#[test]
fn test_systemd_pathname_socket_delivery() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("idlescreen_test_sock_{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let receiver = UnixDatagram::bind(&path).expect("bind receiver socket");
    receiver
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();

    let path_bytes = path.as_os_str().as_encoded_bytes();
    let sent = systemd::send_to_socket_bytes(path_bytes, b"READY=1\n").expect("send datagram");
    assert!(sent);

    let mut buf = [0u8; 64];
    let (len, _) = receiver.recv_from(&mut buf).expect("receive datagram");
    assert_eq!(&buf[..len], b"READY=1\n");

    let _ = std::fs::remove_file(&path);
}

#[cfg(target_os = "linux")]
#[test]
fn test_systemd_abstract_socket_delivery() {
    use std::os::linux::net::SocketAddrExt;
    let abstract_name = format!("idlescreen_test_abs_{}", std::process::id());
    let addr = std::os::unix::net::SocketAddr::from_abstract_name(abstract_name.as_bytes())
        .expect("create abstract addr");

    let receiver = UnixDatagram::bind_addr(&addr).expect("bind abstract receiver");
    receiver
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();

    let target = format!("@{abstract_name}");
    let sent = systemd::send_to_socket_bytes(target.as_bytes(), b"WATCHDOG=1\n")
        .expect("send abstract datagram");
    assert!(sent);

    let mut buf = [0u8; 64];
    let (len, _) = receiver.recv_from(&mut buf).expect("receive datagram");
    assert_eq!(&buf[..len], b"WATCHDOG=1\n");
}

#[test]
fn test_fd_pipe_notification_lifecycle() {
    let mut fds = [0i32; 2];
    // SAFETY: Creating a test pipe pair.
    let res = unsafe { libc::pipe(fds.as_mut_ptr()) };
    assert_eq!(res, 0);

    let (read_fd, write_fd) = (fds[0], fds[1]);

    let notified = fd_pipe::notify_fd(write_fd).expect("notify pipe");
    assert!(notified);

    // Read newline byte from read end
    // SAFETY: read_fd is a valid descriptor created by libc::pipe.
    let mut read_file = unsafe { std::fs::File::from_raw_fd(read_fd) };
    use std::os::unix::io::FromRawFd;

    let mut out = String::new();
    read_file
        .read_to_string(&mut out)
        .expect("read pipe content");
    assert_eq!(out, "\n");

    // Subsequent notification on the same process is rejected by atomic guard
    let second = fd_pipe::notify_fd(write_fd).expect("second notify");
    assert!(!second);
}

#[test]
fn test_fd_pipe_invalid_fd_is_safe() {
    let res = fd_pipe::notify_fd(-1).expect("negative fd is handled");
    assert!(!res);

    let res999 = fd_pipe::notify_fd(9999).expect("unopened fd is handled");
    assert!(!res999);
}

#[test]
fn test_bus_verification_fallback() {
    let claimed = bus::verify_bus_readiness(Duration::from_millis(10));
    let _ = claimed;
}

#[test]
fn test_facade_notifications_do_not_panic() {
    notify_ready();
    notify_stopping();
    notify_watchdog();
}
