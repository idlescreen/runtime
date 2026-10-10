// SPDX-License-Identifier: Apache-2.0
// Empirical Challenger stress tests for idle-dbus service abstraction (M1/R1).

use idle_dbus::service::{
    InitSystem, detect_init_system, restart_daemon_service, start_daemon_service,
    stop_daemon_service,
};
use std::io::ErrorKind;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::sync::Mutex;

static TEST_MUTEX: Mutex<()> = Mutex::new(());

fn make_temp_dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "idle_test_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn is_valid_init(s: InitSystem) -> bool {
    matches!(
        s,
        InitSystem::Systemd
            | InitSystem::OpenRc
            | InitSystem::Runit
            | InitSystem::Dinit
            | InitSystem::S6
            | InitSystem::Standalone
    )
}

#[test]
fn test_detect_init_system_empty_and_corrupt_path() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let original_path = std::env::var_os("PATH");
    unsafe { std::env::set_var("PATH", "") };
    assert!(is_valid_init(detect_init_system()));

    unsafe { std::env::set_var("PATH", "/nonexistent/a:/nonexistent/b:   ") };
    assert!(is_valid_init(detect_init_system()));

    if let Some(p) = original_path {
        unsafe { std::env::set_var("PATH", p) };
    }
}

#[test]
fn test_detect_init_system_env_override_matrix() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let cases = [
        ("systemd", InitSystem::Systemd),
        ("SYSTEMD", InitSystem::Systemd),
        ("  openrc  ", InitSystem::OpenRc),
        ("Runit", InitSystem::Runit),
        ("dinit", InitSystem::Dinit),
        ("s6", InitSystem::S6),
        ("standalone", InitSystem::Standalone),
    ];
    for (val, expected) in cases {
        unsafe { std::env::set_var("IDLE_INIT_SYSTEM", val) };
        assert_eq!(detect_init_system(), expected, "Failed for {val}");
    }
    unsafe { std::env::remove_var("IDLE_INIT_SYSTEM") };
}

#[test]
fn test_start_and_stop_never_return_not_found_on_missing_binaries() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_INIT_SYSTEM", "standalone") };
    let start_res = start_daemon_service();
    if let Err(start_err) = start_res {
        assert_ne!(
            start_err.kind(),
            ErrorKind::NotFound,
            "start_daemon_service must not return NotFound"
        );
        assert_eq!(start_err.kind(), ErrorKind::Other);
    }

    let stop_res = stop_daemon_service();
    if let Err(stop_err) = stop_res {
        assert_ne!(
            stop_err.kind(),
            ErrorKind::NotFound,
            "stop_daemon_service must not return NotFound"
        );
        assert_eq!(stop_err.kind(), ErrorKind::Other);
    }

    let restart_res = restart_daemon_service();
    if let Err(restart_err) = restart_res {
        assert_ne!(
            restart_err.kind(),
            ErrorKind::NotFound,
            "restart_daemon_service must not return NotFound"
        );
        assert_eq!(restart_err.kind(), ErrorKind::Other);
    }
    unsafe { std::env::remove_var("IDLE_INIT_SYSTEM") };
}

#[test]
fn test_read_pidfile_safely_rejects_symlinks() {
    let tmp = make_temp_dir("symlink");
    let target = tmp.join("real.pid");
    std::fs::write(&target, "4242\n").unwrap();

    let link = tmp.join("symlink.pid");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let res = idle_dbus::service::standalone::read_pidfile_safely(&link);
    assert_eq!(res, None, "O_NOFOLLOW must reject symlinks");

    let real_res = idle_dbus::service::standalone::read_pidfile_safely(&target);
    assert_eq!(real_res, Some(4242));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_read_pidfile_safely_malformed_inputs() {
    let tmp = make_temp_dir("malformed");

    let empty = tmp.join("empty.pid");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(
        idle_dbus::service::standalone::read_pidfile_safely(&empty),
        None
    );

    let garbage = tmp.join("garbage.pid");
    std::fs::write(&garbage, "not-a-number").unwrap();
    assert_eq!(
        idle_dbus::service::standalone::read_pidfile_safely(&garbage),
        None
    );

    let overflow = tmp.join("overflow.pid");
    std::fs::write(&overflow, "9999999999999999999999999999").unwrap();
    assert_eq!(
        idle_dbus::service::standalone::read_pidfile_safely(&overflow),
        None
    );

    let non_existent = tmp.join("missing.pid");
    assert_eq!(
        idle_dbus::service::standalone::read_pidfile_safely(&non_existent),
        None
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_pid_targets_idle_daemon_rejects_arbitrary_pids() {
    assert!(!idle_dbus::service::standalone::pid_targets_idle_daemon(1));
    assert!(!idle_dbus::service::standalone::pid_targets_idle_daemon(-1));
    assert!(!idle_dbus::service::standalone::pid_targets_idle_daemon(0));
    assert!(!idle_dbus::service::standalone::pid_targets_idle_daemon(
        999999
    ));
    assert!(!idle_dbus::service::standalone::pid_targets_idle_daemon(
        std::process::id() as i32
    ));
}

#[test]
fn test_pid_targets_idle_daemon_accepts_bare_argv0() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = make_temp_dir("bare_elf");
    let src = tmp.join("main.c");
    let bin = tmp.join("idle-daemon");
    std::fs::write(
        &src,
        "#include <unistd.h>\nint main(void) { pause(); return 0; }\n",
    )
    .unwrap();

    let compile = std::process::Command::new("/usr/bin/gcc")
        .args([src.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .status()
        .expect("compile dummy idle-daemon");
    assert!(compile.success());

    // Explicitly launch with bare argv[0] = "idle-daemon" (no leading '/')
    let mut child = std::process::Command::new(&bin)
        .arg0("idle-daemon")
        .arg("daemon")
        .spawn()
        .expect("spawn idle-daemon with bare argv0");

    let pid = child.id() as i32;
    std::thread::sleep(std::time::Duration::from_millis(100));

    let verified = idle_dbus::service::standalone::pid_targets_idle_daemon(pid);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp);

    assert!(
        verified,
        "pid_targets_idle_daemon must accept bare 'idle-daemon' argv[0]"
    );
}

#[test]
fn test_pid_targets_idle_daemon_accepts_trance_daemon() {
    let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = make_temp_dir("trance_check");
    let src = tmp.join("main.c");
    let bin = tmp.join("trance-daemon");
    std::fs::write(
        &src,
        "#include <unistd.h>\nint main(void) { pause(); return 0; }\n",
    )
    .unwrap();

    let compile = std::process::Command::new("/usr/bin/gcc")
        .args([src.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .status()
        .expect("compile dummy trance-daemon");
    assert!(compile.success());

    let mut child = std::process::Command::new(&bin)
        .arg("daemon")
        .spawn()
        .expect("spawn trance-daemon with path");

    let pid = child.id() as i32;
    std::thread::sleep(std::time::Duration::from_millis(100));

    let verified = idle_dbus::service::standalone::pid_targets_idle_daemon(pid);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp);

    assert!(
        verified,
        "pid_targets_idle_daemon must accept trance-daemon"
    );
}
