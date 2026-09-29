// SPDX-License-Identifier: Apache-2.0

use super::kill_and_reap;
use std::fs;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_socket_path(tag: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "idle-kill-reap-{}-{}-{}.sock",
        tag,
        std::process::id(),
        nanos
    ))
}

#[test]
fn kill_and_reap_removes_socket_and_reaps_running_child() {
    let socket_path = unique_socket_path("running");
    fs::write(&socket_path, b"placeholder").expect("write socket placeholder");
    assert!(socket_path.exists());

    let mut child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    assert!(pid > 0);

    kill_and_reap(&mut child, &socket_path);

    assert!(
        !socket_path.exists(),
        "kill_and_reap must unlink the UDS path"
    );
    // Second wait must not hang: child already reaped.
    let second = child.try_wait();
    assert!(
        second.is_ok(),
        "try_wait after reap should not panic: {second:?}"
    );
}

#[test]
fn kill_and_reap_handles_already_exited_child() {
    let socket_path = unique_socket_path("dead");
    fs::write(&socket_path, b"x").expect("write");

    let mut child = Command::new("true").spawn().expect("spawn true");
    // Ensure the process has exited before we reap.
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(20));
    let _ = child.try_wait();

    kill_and_reap(&mut child, &socket_path);
    assert!(!socket_path.exists());
}

#[test]
fn kill_and_reap_tolerates_missing_socket_file() {
    let socket_path = unique_socket_path("missing");
    assert!(!socket_path.exists());

    let mut child = Command::new("true").spawn().expect("spawn true");
    std::thread::sleep(Duration::from_millis(10));
    kill_and_reap(&mut child, &socket_path);
    assert!(!socket_path.exists());
}

#[test]
fn saver_param_env_maps_and_sanitizes() {
    let mut params = std::collections::BTreeMap::new();
    params.insert("hearth.fire_size".to_string(), "1.5".to_string());
    params.insert("glow".to_string(), "0.8".to_string());
    params.insert("...".to_string(), "dropped".to_string());
    let env = super::saver_param_env(&params);
    assert!(env.contains(&("IDLE_SAVER_PARAM_HEARTH_FIRE_SIZE".into(), "1.5".into())));
    assert!(env.contains(&("IDLE_SAVER_PARAM_GLOW".into(), "0.8".into())));
    assert_eq!(env.len(), 2, "unsanitizable key must be dropped: {env:?}");
}
