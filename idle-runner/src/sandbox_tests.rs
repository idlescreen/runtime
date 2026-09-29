// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for Landlock sandbox policy.
//!
//! 1. With a real plugin path, enforcement succeeds on Landlock kernels.
//! 2. Escape hatch: debug/release rules for IDLE_DISABLE_SANDBOX.
//! 3. Static order: `enforce_sandbox_for_plugin` before `Library::new`.
//! 4. After enforce, opening a disallowed path fails (when Landlock works).

use super::*;
use std::fs;
use std::path::PathBuf;

fn temp_plugin_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("idle-sandbox-test-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let p = dir.join("libscreensaver_test.so");
    if !p.exists() {
        fs::write(&p, b"not-a-real-elf").expect("write dummy plugin");
    }
    p
}

#[test]
fn enforce_sandbox_for_plugin_succeeds_on_modern_kernel() {
    if !landlock_available() {
        eprintln!("skipping: Landlock unavailable on this kernel");
        return;
    }
    let p = temp_plugin_path();
    let r = enforce_sandbox_for_plugin(&p);
    assert!(r.is_ok(), "sandbox enforcement should succeed: {r:?}");
}

#[test]
fn skip_path_when_disable_env_var_set() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    // SAFETY: test-only env mutation.
    unsafe {
        std::env::set_var("IDLE_DISABLE_SANDBOX", "1");
        if !cfg!(debug_assertions) {
            std::env::set_var("IDLE_RENDER_PIPELINE", "1");
        }
    }
    let r = enforce_sandbox_or_skip_for_render();
    unsafe {
        std::env::remove_var("IDLE_DISABLE_SANDBOX");
        std::env::remove_var("IDLE_RENDER_PIPELINE");
    }
    assert!(r.is_ok(), "skip path should succeed when escape hatch set");
}

#[test]
fn without_escape_pathless_entry_fails_closed() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::remove_var("IDLE_DISABLE_SANDBOX");
        std::env::remove_var("IDLE_RENDER_PIPELINE");
    }
    let r = enforce_sandbox_or_skip_for_render();
    assert!(
        r.is_err(),
        "pathless enforce must fail closed without escape"
    );
}

#[test]
fn plugin_loaders_order_sandbox_before_library_new() {
    let idr = format!("{}/src/idle_runner.rs", env!("CARGO_MANIFEST_DIR"));
    let loading = format!(
        "{}/src/plugin_session/loading.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    check_order(&idr);
    check_order(&loading);
}

#[test]
fn post_sandbox_denied_path_unreadable() {
    if !landlock_available() {
        eprintln!("skipping: Landlock unavailable");
        return;
    }
    let p = temp_plugin_path();
    // Fork so Landlock does not poison the parent test process.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        let code = match enforce_sandbox_for_plugin(&p) {
            Ok(()) => match fs::OpenOptions::new().read(true).open("/etc/passwd") {
                Err(_) => 0,
                Ok(_) => 2,
            },
            Err(_) => 1,
        };
        unsafe { libc::_exit(code) };
    }
    let mut status: libc::c_int = 0;
    let w = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(w, pid);
    let exit = libc::WEXITSTATUS(status);
    assert_eq!(
        exit, 0,
        "child exit {exit}: 0=denied ok, 1=enforce fail, 2=passwd still readable"
    );
}

fn check_order(path: &str) {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    // Strip line comments to avoid matching the docstring mention of
    // "Library::new" while still catching the actual call site.
    let code_only: String = src
        .lines()
        .map(|l| match l.find("//") {
            Some(idx) => &l[..idx],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let enforce_idx = code_only
        .find("enforce_sandbox_for_plugin")
        .or_else(|| code_only.find("enforce_sandbox_or_skip_for_render"));
    let lib_idx = code_only.find("Library::new");
    match (enforce_idx, lib_idx) {
        (Some(e), Some(l)) => {
            assert!(
                e < l,
                "{path}: sandbox call must come BEFORE Library::new \
                 (got enforce@{e}, Library::new@{l})"
            );
        }
        (None, None) => {}
        _ => {
            panic!(
                "{path}: inconsistent sandbox/Library::new presence \
                 (enforce={enforce_idx:?}, lib={lib_idx:?})"
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn landlock_available() -> bool {
    std::path::Path::new("/sys/kernel/security/landlock").exists()
        || std::fs::read_to_string("/proc/sys/kernel/seccomp/actions_avail")
            .map(|_| true)
            .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn landlock_available() -> bool {
    false
}
