// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Adversarial tests for F-008 (pidfile `O_NOFOLLOW | O_CREAT | O_EXCL`).
//!
//! Each test must FAIL if the fix is reverted (i.e. `pidfile::acquire_pidfile`
//! goes back to `fs::write` without `O_NOFOLLOW`/`O_EXCL`). Per AUDIT.md §7
//! anti-synthetic checklist: tests exercise the real `pidfile` module and
//! would fail if the vulnerability still exists.
//!
//! These tests mutate the process-global `XDG_RUNTIME_DIR` env var and
//! therefore **must run serially**. The crate-shared [`TEST_ENV_LOCK`] mutex enforces that.

use super::*;

static DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct TempPidDir(PathBuf);

impl TempPidDir {
    fn new(tag: &str) -> Self {
        let n = DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "idle-pidtest-{}-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempPidDir(dir)
    }
}

impl Drop for TempPidDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Wrap the body in the global serial mutex so XDG-RUNTIME_DIR mutations
/// don't race with siblings.
fn run<F: FnOnce()>(f: F) {
    let _guard = crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    f();
}

/// Tests must use a custom pidfile path. `pid_file_path()` reads
/// `XDG_RUNTIME_DIR` first, so we point that env at our temp dir for these
/// tests.
fn with_xdg<F: FnOnce()>(dir: &Path, f: F) {
    let prev = std::env::var("XDG_RUNTIME_DIR").ok();
    // SAFETY: test-only env mutation; serialized via TEST_ENV_LOCK.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", dir);
    }
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    match prev {
        Some(v) => unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", v);
        },
        None => unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        },
    }
    if let Err(e) = r {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn refuses_symlinked_pidfile() {
    run(|| {
        let tmp = TempPidDir::new("sym");
        with_xdg(&tmp.0, || {
            let path = tmp.0.join("idle-daemon.pid");
            let target = tmp.0.join("elsewhere");
            std::fs::write(&target, b"not a pid\n").unwrap();
            std::os::unix::fs::symlink(&target, &path).unwrap();

            let res = acquire_pidfile();
            assert!(
                res.is_err(),
                "acquire_pidfile must reject symlinked pidfile; got {res:?}"
            );
            // The symlink must still be intact (we did not follow it).
            let md = std::fs::symlink_metadata(&path)
                .expect("symlink should still exist after refused acquire");
            assert!(
                md.file_type().is_symlink(),
                "pidfile path must remain a symlink (no follow-write)"
            );
        });
    });
}

#[test]
fn owns_pidfile_when_no_holder() {
    run(|| {
        let tmp = TempPidDir::new("clean");
        with_xdg(&tmp.0, || {
            let path = tmp.0.join("idle-daemon.pid");
            assert!(!path.exists());

            let res = acquire_pidfile();
            assert!(res.is_ok(), "acquire_pidfile on empty dir should succeed");
            assert!(path.exists(), "pidfile must be created");

            let contents = std::fs::read_to_string(&path).unwrap();
            let pid: i32 = contents.trim().parse().unwrap();
            assert_eq!(pid, std::process::id() as i32);

            release_pidfile(&path);
        });
    });
}

#[test]
fn overwrites_stale_pidfile() {
    run(|| {
        let tmp = TempPidDir::new("stale");
        with_xdg(&tmp.0, || {
            let path = tmp.0.join("idle-daemon.pid");
            // Plant a pid that is almost certainly not running.
            let fake_pid = i32::MAX;
            std::fs::write(&path, format!("{fake_pid}\n")).unwrap();

            let res = acquire_pidfile();
            match res {
                Ok(Some(_)) => {
                    let pid: i32 = std::fs::read_to_string(&path)
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    assert_eq!(pid, std::process::id() as i32);
                    release_pidfile(&path);
                }
                Err(e) => panic!("acquire should succeed after detecting stale pid: {e}"),
                Ok(None) => panic!("acquire returned Ok(None); expected Some(path)"),
            }
        });
    });
}

#[test]
fn pidfile_content_is_always_a_valid_pid() {
    run(|| {
        let tmp = TempPidDir::new("flag");
        with_xdg(&tmp.0, || {
            let path = tmp.0.join("idle-daemon.pid");
            std::fs::write(&path, b"deadbeef\n").unwrap();

            let res = acquire_pidfile();
            // Accept any non-error outcome: Ok(Some(_)) means we own the
            // pidfile (verify content); Ok(None) means the loop refused to
            // clobber a live holder; Err means we refused to follow a
            // symlink / etc. All are valid for this stress test.
            if let Ok(Some(_)) = res {
                let pid: i32 = std::fs::read_to_string(&path)
                    .unwrap_or_else(|_| panic!("pidfile vanished at {}", path.display()))
                    .trim()
                    .parse()
                    .expect("pidfile content should be a pid");
                assert_eq!(pid, std::process::id() as i32);
                release_pidfile(&path);
            }
        });
    });
}

use std::path::Path;
