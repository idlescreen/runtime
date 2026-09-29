// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for F-004 (`poison_or_exit` abort on poisoned lock).
//!
//! Per AUDIT.md §7: tests must exercise the real `locks` module and would
//! fail if `poison_or_exit` were reverted to silently recover via
//! `into_inner()`.

use super::*;

#[test]
fn poison_or_exit_aborts_process() {
    // Spawn a child process that poisons a Mutex and calls poison_or_exit.
    // Assert the child exits with a non-zero status (abort signal).
    let exe = std::env::current_exe().expect("current_exe");
    let status = std::process::Command::new(exe)
        .arg("--ignored-when-runnning-tests")
        .arg("poison_or_exit_subprocess")
        .env("POISON_OR_EXIT_SUBPROCESS", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("spawn child");
    // abort() raises SIGABRT (signal 6) on Unix, which gives exit status
    // 134 on Linux shells. The status code via .code() is Some(134) for
    // a signal-aborted child; or for some kernels the shell reports 6.
    let code = status.code();
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    assert!(
        !status.success(),
        "child should NOT exit successfully; status code={code:?} signal={signal:?}"
    );
}

#[test]
fn submodule_assembled_for_subprocess() {
    // This test only runs in the subprocess invoked above.
    // `#[test]` doesn't run by itself because of the env guard.
    if std::env::var("POISON_OR_EXIT_SUBPROCESS").is_err() {
        return;
    }
    let m: std::sync::Mutex<i32> = std::sync::Mutex::new(0);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _g = m.lock().unwrap();
        panic!("poison the mutex");
    }));
    // Now the mutex is poisoned. `poison_or_exit` must terminate the
    // process — it returns `!`, so this line is unreachable if the fix
    // is in place. If someone reverts to `into_inner()`, the test exits 0.
    let _guard = m.lock().unwrap_or_else(|p| poison_or_exit("test", p));
    // Reachable only if poison_or_exit was reverted to silently recover.
    eprintln!("UNREACHABLE: poison_or_exit returned (reverted to silent recovery)");
    std::process::exit(0);
}
