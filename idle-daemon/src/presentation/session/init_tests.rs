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

// ---- logo asset delivery -------------------------------------------------
//
// The daemon is the only process that opens the configured art file. These
// tests pin that contract: what the plugin eventually reads via
// `idle_api::asset` is exactly what the daemon read, bounded, and nothing
// else.

fn temp_art(name: &str, contents: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("idle-logo-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join(name);
    fs::write(&path, contents).expect("write");
    path
}

#[test]
fn no_logo_file_yields_no_env() {
    assert!(super::logo_asset_env(None).is_empty());
}

#[test]
fn a_configured_logo_is_delivered_under_the_well_known_key() {
    let art = "  _  _\n | || |\n |__   _|";
    let path = temp_art("delivered.txt", art);
    let env = super::logo_asset_env(Some(path.to_str().unwrap()));
    assert_eq!(env.len(), 1);
    assert_eq!(env[0].0, idle_api::asset_env_key(idle_api::ASSET_LOGO));
    assert_eq!(env[0].1, art);
}

#[test]
fn an_unreadable_logo_file_is_not_fatal() {
    // A broken logo must never stop the screensaver from presenting.
    let env = super::logo_asset_env(Some("/nonexistent/idle/no-such-logo.txt"));
    assert!(env.is_empty());
}

#[test]
fn an_oversized_logo_file_is_refused_rather_than_truncated() {
    // Truncating would hand the plugin a half-drawn logo that looks like a
    // rendering bug. Dropping it makes the saver fall back to its default.
    let big = "x".repeat(idle_api::asset::MAX_ASSET_BYTES + 1);
    let path = temp_art("too-big.txt", &big);
    let env = super::logo_asset_env(Some(path.to_str().unwrap()));
    assert!(env.is_empty());
}

#[test]
fn a_logo_exactly_at_the_limit_is_accepted() {
    let exact = "y".repeat(idle_api::asset::MAX_ASSET_BYTES);
    let path = temp_art("at-limit.txt", &exact);
    let env = super::logo_asset_env(Some(path.to_str().unwrap()));
    assert_eq!(env.len(), 1, "the limit itself must be allowed");
}

#[test]
fn shell_branding_is_used_when_no_logo_file_is_configured() {
    // `omarchy branding screensaver image` writes here and then force-relaunches.
    // Honouring it is what keeps already-set artwork working after the hand-off.
    let dir = std::env::temp_dir().join(format!("idle-branding-{}", std::process::id()));
    let branding = dir.join("omarchy/branding");
    fs::create_dir_all(&branding).expect("mkdir");
    let file = branding.join("screensaver.txt");
    fs::write(&file, "BRANDING").expect("write");

    let home = std::env::temp_dir().join(format!("idle-home-{}", std::process::id()));
    let target = home.join(".config/omarchy/branding");
    fs::create_dir_all(&target).expect("mkdir");
    fs::write(target.join("screensaver.txt"), "BRANDING").expect("write");

    unsafe { std::env::set_var("HOME", &home) };
    let env = super::logo_asset_env(None);
    unsafe { std::env::remove_var("HOME") };

    assert_eq!(
        env.len(),
        1,
        "shell branding should be picked up automatically"
    );
    assert_eq!(env[0].1, "BRANDING");
}

#[test]
fn an_explicit_logo_file_beats_the_shell_branding() {
    let home = std::env::temp_dir().join(format!("idle-home-b-{}", std::process::id()));
    let target = home.join(".config/omarchy/branding");
    fs::create_dir_all(&target).expect("mkdir");
    fs::write(target.join("screensaver.txt"), "BRANDING").expect("write");
    let mine = temp_art("explicit.txt", "MINE");

    unsafe { std::env::set_var("HOME", &home) };
    let env = super::logo_asset_env(Some(mine.to_str().unwrap()));
    unsafe { std::env::remove_var("HOME") };

    assert_eq!(env[0].1, "MINE", "logo_file must win over the fallback");
}
