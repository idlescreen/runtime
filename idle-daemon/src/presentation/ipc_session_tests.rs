// perf: T3 · metric: test-only page, not compiled into the shipped binary · check: test
// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for the subprocess-isolation primitive.

use super::ipc_session::IpcPluginSession;
use idle_runner::launcher::LaunchMode;

#[test]
fn is_timeout_classifies_timed_out_and_would_block() {
    use super::timeout::is_timeout;
    let timed_out = std::io::Error::new(std::io::ErrorKind::TimedOut, "test");
    let would_block = std::io::Error::new(std::io::ErrorKind::WouldBlock, "test");
    let other = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "test");
    assert!(is_timeout(&timed_out));
    assert!(is_timeout(&would_block));
    assert!(!is_timeout(&other), "BrokenPipe is not a timeout");
}

#[test]
fn read_timeout_env_override_works() {
    use super::timeout::read_timeout;
    let _g = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_IPC_READ_TIMEOUT_MS", "123") };
    let t = read_timeout();
    unsafe { std::env::remove_var("IDLE_IPC_READ_TIMEOUT_MS") };
    assert_eq!(t, std::time::Duration::from_millis(123));
}

#[test]
fn read_timeout_default_when_env_unset() {
    use super::timeout::{DEFAULT_IPC_READ_TIMEOUT, read_timeout};
    let _g = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_IPC_READ_TIMEOUT_MS") };
    assert_eq!(read_timeout(), DEFAULT_IPC_READ_TIMEOUT);
}

#[test]
fn kill_child_is_idempotent_without_child() {
    let mut s = IpcPluginSession::load_with_options(
        "beams",
        &LaunchMode::Daemon,
        None,
        std::collections::BTreeMap::new(),
        false,
    )
    .expect("load");
    // No child yet; kill must not panic.
    s.kill_child();
    s.kill_child();
}

#[test]
fn kill_child_clears_handle() {
    let mut s = IpcPluginSession::load_with_options(
        "beams",
        &LaunchMode::Daemon,
        None,
        std::collections::BTreeMap::new(),
        false,
    )
    .expect("load");
    // Simulate a live child by inserting a dummy process handle would
    // require spawning; we instead assert that the kill path on a
    // `None` child clears state correctly (the slot stays None).
    s.kill_child();
    assert!(
        s.child.is_none(),
        "kill on None must leave child slot empty"
    );
}

#[test]
fn expected_stop_is_set_after_kill() {
    let mut s = IpcPluginSession::load_with_options(
        "beams",
        &LaunchMode::Daemon,
        None,
        std::collections::BTreeMap::new(),
        false,
    )
    .expect("load");
    s.kill_child();
    assert!(
        s.expected_stop.load(std::sync::atomic::Ordering::Acquire),
        "kill_child must mark expected_stop so an intentional teardown is not logged as an unexpected child death"
    );
}

/// Spawn a real long-running child process (sleep 30), inject it into
/// the session, kill it, and assert the kernel reaped it. This exercises
/// the actual `Child::kill` path — the docstring-stub tests above only
/// cover state transitions.
#[test]
fn kill_child_reaps_real_process() {
    use std::process::Command;

    let child = Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id() as i32;
    assert!(pid > 0, "spawn returned a real pid");

    let mut s = IpcPluginSession::load_with_options(
        "beams",
        &LaunchMode::Daemon,
        None,
        std::collections::BTreeMap::new(),
        false,
    )
    .expect("load");
    s.child = Some(child);

    let before = unsafe { libc::kill(pid, 0) };
    assert_eq!(before, 0, "child must be alive before kill_child()");

    s.kill_child();

    assert!(
        s.child.is_none(),
        "kill_child must consume the Child handle"
    );
    let after = unsafe { libc::kill(pid, 0) };
    assert_ne!(
        after, 0,
        "process pid {} must be reaped (kill returned {})",
        pid, after
    );
    if after == -1 {
        let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        assert_eq!(
            err, 3, /* ESRCH */
            "kill(pid, 0) post-reap must return ESRCH"
        );
    }

    s.kill_child();
}

/// Regression for the respawn storm: a saver that misses the IPC deadline was
/// respawned *every frame*, re-running Landlock, cgroup attach, renderer init
/// and a fresh SHM mapping about once a second for the whole idle session.
#[test]
fn consecutive_timeouts_are_budgeted_and_progress_resets_them() {
    let _g = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("IDLE_MAX_RUNNER_TIMEOUTS") };

    let mut s = IpcPluginSession::load_with_options(
        "beams",
        &LaunchMode::Daemon,
        None,
        std::collections::BTreeMap::new(),
        false,
    )
    .expect("load");

    let budget = IpcPluginSession::max_consecutive_timeouts();
    assert_eq!(budget, 3, "default budget is 3 consecutive timeouts");

    assert!(s.should_recover(80, 24), "a fresh session may respawn");
    for i in 1..budget {
        s.note_timeout();
        assert_eq!(s.consecutive_timeouts, i);
        assert!(
            s.should_recover(80, 24),
            "must still respawn at timeout {i} (budget {budget})"
        );
    }
    s.note_timeout();
    assert!(
        !s.should_recover(80, 24),
        "budget exhausted — must refuse to respawn and let the fault path run"
    );

    s.note_progress();
    assert_eq!(s.consecutive_timeouts, 0, "a good frame clears the run");
    assert!(s.should_recover(80, 24));
}

#[test]
fn exhausted_geometry_is_never_retried_again() {
    let mut s = IpcPluginSession::load_with_options(
        "beams",
        &LaunchMode::Daemon,
        None,
        std::collections::BTreeMap::new(),
        false,
    )
    .expect("load");

    s.mark_exhausted(80, 24);
    assert!(
        !s.should_recover(80, 24),
        "a geometry that already burned its budget must not be retried"
    );
    assert!(
        s.should_recover(120, 40),
        "a different geometry is still allowed a fresh budget"
    );
}

#[test]
fn max_runner_timeouts_env_override() {
    let _g = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("IDLE_MAX_RUNNER_TIMEOUTS", "7") };
    assert_eq!(IpcPluginSession::max_consecutive_timeouts(), 7);
    // A zero budget would disable recovery entirely; reject it.
    unsafe { std::env::set_var("IDLE_MAX_RUNNER_TIMEOUTS", "0") };
    assert_eq!(IpcPluginSession::max_consecutive_timeouts(), 3);
    unsafe { std::env::remove_var("IDLE_MAX_RUNNER_TIMEOUTS") };
    assert_eq!(IpcPluginSession::max_consecutive_timeouts(), 3);
}
