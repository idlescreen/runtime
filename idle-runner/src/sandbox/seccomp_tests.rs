// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]

use super::seccomp::*;

#[test]
fn test_seccomp_filter_instruction_count_is_24() {
    let filter = build_filter();
    assert_eq!(filter.len(), 24);
}

#[cfg(target_os = "linux")]
#[test]
fn test_seccomp_blocks_inet_allows_unix_and_permits_threads() {
    let _g = crate::ENV_LOCK.lock().unwrap();

    // SAFETY: fork child to test seccomp in isolation.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");

    if pid == 0 {
        if apply_seccomp().is_err() {
            unsafe { libc::_exit(1) };
        }

        // 1. AF_INET must be blocked with EACCES
        let inet_fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        if inet_fd >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EACCES) {
            unsafe { libc::_exit(2) };
        }

        // 2. AF_INET6 must be blocked with EACCES
        let inet6_fd = unsafe { libc::socket(libc::AF_INET6, libc::SOCK_STREAM, 0) };
        if inet6_fd >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EACCES) {
            unsafe { libc::_exit(3) };
        }

        // 3. AF_UNIX must be permitted
        let unix_fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if unix_fd < 0 {
            unsafe { libc::_exit(4) };
        }
        unsafe { libc::close(unix_fd) };

        // 4. Thread creation must succeed via CLONE_THREAD
        let handle = std::thread::spawn(|| 42);
        if handle.join().ok() != Some(42) {
            unsafe { libc::_exit(5) };
        }

        // 5. execve must be blocked with EPERM
        let cmd = c"/bin/sh";
        let argv = [cmd.as_ptr(), std::ptr::null()];
        let envp = [std::ptr::null()];
        let exec_res = unsafe { libc::execve(cmd.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
        if exec_res >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
            unsafe { libc::_exit(6) };
        }

        // 6. fork must be blocked with EPERM
        let fork_res = unsafe { libc::fork() };
        if fork_res >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
            unsafe { libc::_exit(7) };
        }

        // 7. ptrace must be blocked with EPERM
        let ptrace_res = unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) };
        if ptrace_res >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
            unsafe { libc::_exit(8) };
        }

        unsafe { libc::_exit(0) };
    }

    let mut status: libc::c_int = 0;
    // SAFETY: waitpid on child pid.
    let w = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(w, pid);
    let exit = libc::WEXITSTATUS(status);
    assert_eq!(
        exit, 0,
        "child exited with error code {exit}: 1=seccomp_failed, 2=inet_allowed, 3=inet6_allowed, 4=unix_blocked, 5=threads_failed, 6=execve_allowed, 7=fork_allowed, 8=ptrace_allowed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_seccomp_require_env_var() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        // SAFETY: set env in isolated child process
        unsafe { std::env::set_var("IDLE_REQUIRE_SECCOMP", "1") };
        let exit_code = match apply_seccomp() {
            Ok(()) => 0,
            Err(_) => 1,
        };
        unsafe { libc::_exit(exit_code) };
    }
    let mut status: libc::c_int = 0;
    // SAFETY: wait on child
    let w = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(w, pid);
    assert_eq!(libc::WEXITSTATUS(status), 0);
}
