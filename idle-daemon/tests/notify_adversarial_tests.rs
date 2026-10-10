// SPDX-License-Identifier: MIT
// Empirical Challenger stress tests for idle-daemon readiness notification (M1/R3).

use idle_daemon::notify::{fd_pipe, systemd};
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixDatagram;
use std::sync::Mutex;
use std::time::Duration;

static NOTIFY_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_abstract_socket_edge_cases_and_limits() {
    #[cfg(target_os = "linux")]
    {
        use std::os::linux::net::SocketAddrExt;

        // 1. Unbound / nonexistent abstract socket must return Err without panicking
        let unbound_target = format!("@idlescreen_unbound_{}", std::process::id());
        let res = systemd::send_to_socket_bytes(unbound_target.as_bytes(), b"READY=1\n");
        assert!(res.is_err(), "sending to unbound abstract socket must fail");

        // 2. Empty abstract socket name (@)
        let empty_res = systemd::send_to_socket_bytes(b"@", b"READY=1\n");
        let _ = empty_res; // must not panic

        // 3. Oversized abstract socket name (>108 bytes)
        let oversized = vec![b'a'; 150];
        let mut target = vec![b'@'];
        target.extend_from_slice(&oversized);
        let over_res = systemd::send_to_socket_bytes(&target, b"READY=1\n");
        assert!(
            over_res.is_err(),
            "oversized abstract socket name must fail"
        );

        // 4. Abstract socket with embedded null bytes
        let null_target = b"@idle\0screen\0test";
        let null_res = systemd::send_to_socket_bytes(null_target, b"READY=1\n");
        let _ = null_res; // must not panic

        // 5. Valid abstract socket roundtrip
        let name = format!("idlescreen_valid_{}", std::process::id());
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        let receiver = UnixDatagram::bind_addr(&addr).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();

        let target_str = format!("@{name}");
        let sent = systemd::send_to_socket_bytes(target_str.as_bytes(), b"WATCHDOG=1\n").unwrap();
        assert!(sent);

        let mut buf = [0u8; 64];
        let (len, _) = receiver.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..len], b"WATCHDOG=1\n");
    }
}

#[test]
fn test_pathname_socket_abnormal_targets() {
    let tmp = std::env::temp_dir();

    // 1. Target is a regular file, not a socket
    let reg_file = tmp.join(format!("idle_not_a_sock_{}.txt", std::process::id()));
    std::fs::write(&reg_file, "hello").unwrap();
    let reg_res =
        systemd::send_to_socket_bytes(reg_file.as_os_str().as_encoded_bytes(), b"READY=1\n");
    assert!(reg_res.is_err(), "regular file is not a datagram socket");
    let _ = std::fs::remove_file(&reg_file);

    // 2. Target is a directory
    let dir_res = systemd::send_to_socket_bytes(tmp.as_os_str().as_encoded_bytes(), b"READY=1\n");
    assert!(dir_res.is_err(), "directory is not a datagram socket");

    // 3. Target does not exist
    let missing = tmp.join(format!("idle_missing_{}.sock", std::process::id()));
    let missing_res =
        systemd::send_to_socket_bytes(missing.as_os_str().as_encoded_bytes(), b"READY=1\n");
    assert!(missing_res.is_err(), "missing path must return error");

    // 4. Target path exceeds max sockaddr_un limit
    let long_name = "x".repeat(200);
    let long_path = tmp.join(format!("{long_name}.sock"));
    let long_res =
        systemd::send_to_socket_bytes(long_path.as_os_str().as_encoded_bytes(), b"READY=1\n");
    assert!(long_res.is_err(), "oversized pathname must return error");
}

#[test]
fn test_fd_pipe_non_fifo_protection() {
    let _guard = NOTIFY_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // 1. Negative FDs must return Ok(false)
    assert!(!fd_pipe::notify_fd(-1).unwrap());
    assert!(!fd_pipe::notify_fd(-100).unwrap());

    // 2. Unopened high FD must return Ok(false)
    assert!(!fd_pipe::notify_fd(99999).unwrap());

    // 3. Regular file descriptor must NOT be notified or closed
    let tmp_file = std::fs::File::create(
        std::env::temp_dir().join(format!("idle_not_fifo_{}.tmp", std::process::id())),
    )
    .unwrap();
    use std::os::unix::io::AsRawFd;
    let raw_fd = tmp_file.as_raw_fd();
    let res = fd_pipe::notify_fd(raw_fd).unwrap();
    assert!(!res, "notify_fd must reject regular file descriptors");

    // Verify descriptor is still valid and not closed
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let stat_res = unsafe { libc::fstat(raw_fd, stat.as_mut_ptr()) };
    assert_eq!(stat_res, 0, "regular file fd must remain open");

    // 4. Socket descriptor must NOT be treated as FIFO
    let sock = UnixDatagram::unbound().unwrap();
    assert!(!fd_pipe::notify_fd(sock.as_raw_fd()).unwrap());
}

#[test]
fn test_fd_pipe_listen_fds_guard() {
    let _guard = NOTIFY_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (fds[0], fds[1]);

    unsafe { std::env::set_var("LISTEN_FDS", "1") };
    let notified = fd_pipe::notify_fd(write_fd).unwrap();
    assert!(
        !notified,
        "LISTEN_FDS must prevent notify_fd from touching the descriptor"
    );

    // Verify write_fd is still open
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    assert_eq!(unsafe { libc::fstat(write_fd, stat.as_mut_ptr()) }, 0);

    unsafe {
        std::env::remove_var("LISTEN_FDS");
        libc::close(read_fd);
        libc::close(write_fd);
    }
}

#[test]
fn test_fd_pipe_broken_pipe_safety() {
    let _guard = NOTIFY_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // Pipe with read end immediately closed
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (fds[0], fds[1]);

    // Close reader before notifying
    unsafe { libc::close(read_fd) };

    // Must not terminate on SIGPIPE and should cleanly close descriptor
    let res = fd_pipe::notify_fd(write_fd);
    assert!(res.is_ok(), "broken pipe must not panic or crash");

    // Verify descriptor was closed
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    assert_eq!(unsafe { libc::fstat(write_fd, stat.as_mut_ptr()) }, -1);
}

#[test]
fn test_fd_pipe_notification_lifecycle_roundtrip() {
    let _guard = NOTIFY_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (fds[0], fds[1]);

    assert!(fd_pipe::notify_fd(write_fd).unwrap());

    // Verify reader gets exactly '\n' and then EOF
    let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
    let mut buf = String::new();
    file.read_to_string(&mut buf).unwrap();
    assert_eq!(buf, "\n");
}

#[test]
fn test_notify_facade_concurrency_stress() {
    let _guard = NOTIFY_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let mut handles = Vec::new();
    for _ in 0..10 {
        handles.push(std::thread::spawn(|| {
            idle_daemon::notify::notify_ready();
            idle_daemon::notify::notify_watchdog();
            idle_daemon::notify::notify_stopping();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}
