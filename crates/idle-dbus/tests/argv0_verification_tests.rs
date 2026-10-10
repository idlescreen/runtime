// SPDX-License-Identifier: Apache-2.0
// Empirical Challenger adversarial stress tests for bare argv0 and process validation (M1 Iter 2).

use idle_dbus::service::standalone::{
    is_daemon_cmdline_arg, is_daemon_comm, pid_targets_idle_daemon,
};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::sync::Mutex;

static ARGV0_MUTEX: Mutex<()> = Mutex::new(());

fn make_temp_dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "idle_argv0_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn test_cmdline_arg_exhaustive_matrix() {
    let valid_cases = [
        "idle-daemon",
        "/usr/bin/idle-daemon",
        "/usr/local/bin/idle-daemon",
        "./idle-daemon",
        "../idle-daemon",
        "/opt/idlescreen/bin/idle-daemon",
        "idlescreen-daemon",
        "/usr/bin/idlescreen-daemon",
        "/usr/local/bin/idlescreen-daemon",
        "./idlescreen-daemon",
        "trance-daemon",
        "/usr/bin/trance-daemon",
        "./trance-daemon",
    ];
    for case in valid_cases {
        assert!(
            is_daemon_cmdline_arg(case),
            "Expected '{case}' to match daemon cmdline arg"
        );
    }

    let invalid_cases = [
        "not-idle-daemon",
        "/usr/bin/not-idle-daemon",
        "/usr/bin/my-idle-daemon",
        "idle-daemon-helper",
        "idle-daemon-miner",
        "/usr/bin/idlescreen-daemon-extra",
        "trance-daemon-plugin",
        "idlescreen",
        "idle-cli",
        "bash",
        "python3",
        "",
        "idle-daemon/",
    ];
    for case in invalid_cases {
        assert!(
            !is_daemon_cmdline_arg(case),
            "Expected '{case}' to be rejected as daemon cmdline arg"
        );
    }
}

#[test]
fn test_comm_exhaustive_matrix() {
    let valid_comm = [
        "idle-daemon",
        "idle-daemon\n",
        "idlescreen-daemon",
        "idlescreen-daemon\n",
        "idlescreen-daem", // Linux 15-char TASK_COMM_LEN truncation
        "idlescreen-daem\n",
        "idlescreen-",
        "idlescreen",
        "idlescreen_test",
        "trance-daemon",
        "trance-",
    ];
    for case in valid_comm {
        assert!(
            is_daemon_comm(case),
            "Expected comm '{case}' to match daemon"
        );
    }

    let invalid_comm = [
        "systemd", "bash", "python3", "not-idle", "idle", "daemon", "trance", "gedit", "", "   ",
    ];
    for case in invalid_comm {
        assert!(
            !is_daemon_comm(case),
            "Expected comm '{case}' to be rejected"
        );
    }
}

#[test]
fn test_real_process_bare_and_path_matrix() {
    let _guard = ARGV0_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = make_temp_dir("matrix");
    let src = tmp.join("dummy.c");
    let bin_idle = tmp.join("idle-daemon");
    let bin_screen = tmp.join("idlescreen-daemon");

    std::fs::write(
        &src,
        "#include <unistd.h>\nint main(void) { pause(); return 0; }\n",
    )
    .unwrap();

    let compile1 = std::process::Command::new("/usr/bin/gcc")
        .args([src.to_str().unwrap(), "-o", bin_idle.to_str().unwrap()])
        .status()
        .expect("compile idle-daemon dummy");
    assert!(compile1.success());

    let compile2 = std::process::Command::new("/usr/bin/gcc")
        .args([src.to_str().unwrap(), "-o", bin_screen.to_str().unwrap()])
        .status()
        .expect("compile idlescreen-daemon dummy");
    assert!(compile2.success());

    // 1. Bare argv0 idle-daemon
    let mut c1 = std::process::Command::new(&bin_idle)
        .arg0("idle-daemon")
        .arg("daemon")
        .spawn()
        .expect("spawn bare idle-daemon");
    let pid1 = c1.id() as i32;
    std::thread::sleep(std::time::Duration::from_millis(50));
    let v1 = pid_targets_idle_daemon(pid1);
    let _ = c1.kill();
    let _ = c1.wait();
    assert!(v1, "Bare 'idle-daemon' must be verified");

    // 2. Full path idle-daemon
    let mut c2 = std::process::Command::new(&bin_idle)
        .arg("daemon")
        .spawn()
        .expect("spawn full path idle-daemon");
    let pid2 = c2.id() as i32;
    std::thread::sleep(std::time::Duration::from_millis(50));
    let v2 = pid_targets_idle_daemon(pid2);
    let _ = c2.kill();
    let _ = c2.wait();
    assert!(v2, "Full path 'idle-daemon' must be verified");

    // 3. Bare argv0 idlescreen-daemon (tests 15-char comm truncation)
    let mut c3 = std::process::Command::new(&bin_screen)
        .arg0("idlescreen-daemon")
        .arg("daemon")
        .spawn()
        .expect("spawn bare idlescreen-daemon");
    let pid3 = c3.id() as i32;
    std::thread::sleep(std::time::Duration::from_millis(50));
    let v3 = pid_targets_idle_daemon(pid3);
    let _ = c3.kill();
    let _ = c3.wait();
    assert!(
        v3,
        "Bare 'idlescreen-daemon' with comm truncation must be verified"
    );

    // 4. Spoofing test: comm is dummy, but argv1 mentions idle-daemon
    let mut c4 = std::process::Command::new(&bin_idle)
        .arg0("unrelated-service")
        .args(["daemon", "idle-daemon"])
        .spawn()
        .expect("spawn unrelated-service with idle-daemon arg");
    let pid4 = c4.id() as i32;
    std::thread::sleep(std::time::Duration::from_millis(50));
    let v4 = pid_targets_idle_daemon(pid4);
    let _ = c4.kill();
    let _ = c4.wait();
    assert!(
        !v4,
        "Process with non-matching argv0 and comm must be rejected"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
