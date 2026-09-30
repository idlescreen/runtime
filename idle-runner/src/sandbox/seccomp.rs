// SPDX-License-Identifier: MIT
//! Seccomp-BPF filter for plugin host processes.
//!
//! Enforces:
//! - Disallow network socket creation (AF_INET, AF_INET6 return EACCES; AF_UNIX allowed).
//! - Disallow process execution / injection (execve, fork, ptrace, bpf return EPERM).
//! - Fallback clone3 to ENOSYS (forcing pthread_create to clone).
//! - Permit clone with CLONE_THREAD for multithreaded plugins.

#[cfg(target_arch = "x86_64")]
pub const CURRENT_ARCH: u32 = 0xc000_003e; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
pub const CURRENT_ARCH: u32 = 0xc000_00b7; // AUDIT_ARCH_AARCH64
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const CURRENT_ARCH: u32 = 0;

const BPF_LD: u16 = 0x00;
const BPF_ALU: u16 = 0x04;
const BPF_JMP: u16 = 0x05;
const BPF_RET: u16 = 0x06;

const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_K: u16 = 0x00;

const BPF_JEQ: u16 = 0x10;
const BPF_AND: u16 = 0x50;

const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

const fn bpf_stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

const fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

/// Builds the 24-instruction verified BPF filter.
pub fn build_filter() -> [libc::sock_filter; 24] {
    [
        // [0] Load architecture
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 4),
        // [1] Verify architecture
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, CURRENT_ARCH, 1, 0),
        // [2] Kill if architecture mismatch
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        // [3] Load syscall number
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 0),
        // [4..9] Disallowed syscalls: execve, execveat, fork, vfork, ptrace, bpf -> [10]
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_execve as u32, 5, 0),
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_execveat as u32, 4, 0),
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_fork as u32, 3, 0),
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_vfork as u32, 2, 0),
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_ptrace as u32, 1, 0),
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_bpf as u32, 0, 1),
        // [10] Return EPERM for banned syscalls
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (libc::EPERM as u32)),
        // [11] Check clone3 -> [12] ENOSYS, else [13]
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_clone3 as u32, 0, 1),
        // [12] Return ENOSYS on clone3 (forces glibc fallback to clone)
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (libc::ENOSYS as u32)),
        // [13] Check clone -> [14], else skip to sockets at [18]
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_clone as u32, 0, 4),
        // [14] Load clone flags (args[0])
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 16),
        // [15] Mask with CLONE_THREAD
        bpf_stmt(BPF_ALU | BPF_AND | BPF_K, libc::CLONE_THREAD as u32),
        // [16] If CLONE_THREAD set, jump to ALLOW at [23], else fall through to [17]
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::CLONE_THREAD as u32, 6, 0),
        // [17] Return EPERM for non-thread clone
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (libc::EPERM as u32)),
        // [18..19] Check socket and socketpair -> [20], else ALLOW at [23]
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_socket as u32, 1, 0),
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_socketpair as u32, 0, 3),
        // [20] Load domain (args[0])
        bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 16),
        // [21] Allow AF_UNIX (1) -> [23], else EACCES at [22]
        bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, libc::AF_UNIX as u32, 1, 0),
        // [22] Return EACCES for non-UNIX sockets (AF_INET, AF_INET6)
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (libc::EACCES as u32)),
        // [23] Allow all remaining syscalls
        bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    ]
}

/// Applies the seccomp-BPF sandbox filter.
pub fn apply_seccomp() -> Result<(), String> {
    let mut filter = build_filter();
    let prog = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };

    // Ensure PR_SET_NO_NEW_PRIVS is active before seccomp filter installation.
    // SAFETY: PR_SET_NO_NEW_PRIVS is a pure state transition with no invalid inputs.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!(
            "prctl(NO_NEW_PRIVS): {}",
            std::io::Error::last_os_error()
        ));
    }

    // SAFETY: prog points to valid array of 24 sock_filter instructions.
    let ret = unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &prog) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if std::env::var_os("IDLE_REQUIRE_SECCOMP").is_some() {
            return Err(format!(
                "seccomp required by IDLE_REQUIRE_SECCOMP but unavailable: {err}"
            ));
        }
        idle_log::warn!(
            "seccomp filter unavailable ({err}); continuing under Landlock-only enforcement"
        );
    }
    Ok(())
}
