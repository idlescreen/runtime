// SPDX-License-Identifier: Apache-2.0

//! Peer executable / comm inspection for D-Bus control auth.

/// Basenames of processes allowed to call control methods on the daemon.
pub(super) const TRUSTED_CONTROL_PEERS: &[&str] = &[
    // Never trust basename "idle" — that is Fedora's python3-idle IDE binary.
    "idlescreen",
    "idle-cli",
    "idle-tui",
    "idlescreen-applet",
];

/// Linux `TASK_COMM_LEN` is 16 bytes including NUL → 15 visible chars in `/proc/pid/comm`.
const COMM_MAX: usize = 15;

/// Result of inspecting a peer executable path.
#[derive(Debug)]
pub(super) enum PeerExeCheck {
    /// Path readable and matches trusted name + install prefix (+ root ownership).
    Trusted,
    /// Path readable but not an allowed control client.
    Untrusted,
    /// Cannot read `/proc/<pid>/exe` (common under systemd hardening + Yama).
    Unreadable,
}

#[cfg(test)]
pub(super) fn peer_exe_basename(pid: u32) -> Option<String> {
    let path = format!("/proc/{pid}/exe");
    let target = std::fs::canonicalize(path).ok()?;
    target
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

pub(super) fn check_peer_exe(pid: u32) -> PeerExeCheck {
    let path = format!("/proc/{pid}/exe");
    let target = match std::fs::canonicalize(&path) {
        Ok(t) => t,
        Err(e) => {
            // EACCES/EPERM: hardened services often cannot ptrace-read peer
            // `/proc/<pid>/exe` (Yama / ProtectProc). ENOENT: peer already exited.
            // Expected path — fall through to same-UID + /proc/pid/comm. Do not
            // warn; that spams journal on every control call from CLI/TUI.
            idle_log::debug!(
                "D-Bus auth check: /proc/{pid}/exe unreadable ({e}); will try peer comm"
            );
            return PeerExeCheck::Unreadable;
        }
    };
    let name = match target.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => {
            idle_log::warn!("D-Bus auth check: failed to get file name from {target:?}");
            return PeerExeCheck::Untrusted;
        }
    };
    if !TRUSTED_CONTROL_PEERS.contains(&name) {
        idle_log::warn!(
            "D-Bus auth check: process name {name:?} is not in trusted control peers list"
        );
        return PeerExeCheck::Untrusted;
    }
    let parent = target.parent().and_then(|p| p.to_str()).unwrap_or("");
    // Trusted locations:
    // - packaged install prefixes
    // - cargo target/{debug,release} (local builds; release daemon used to reject these)
    // - same directory as this daemon binary (dev / monorepo layout)
    let path_ok = is_system_bin_dir(parent)
        || is_cargo_target_bin_dir(parent)
        || same_dir_as_current_exe(&target);
    if !path_ok {
        idle_log::warn!("D-Bus auth check: path {target:?} parent {parent:?} not trusted");
        return PeerExeCheck::Untrusted;
    }

    // Not world-writable; system prefixes must be root-owned (or nobody).
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&target) {
            Ok(meta) => {
                if meta.mode() & 0o002 != 0 {
                    idle_log::warn!(
                        "D-Bus auth check: refusing world-writable peer binary {target:?}"
                    );
                    return PeerExeCheck::Untrusted;
                }
                if is_system_bin_dir(parent) && meta.uid() != 0 && meta.uid() != 65534 {
                    idle_log::warn!(
                        "D-Bus auth check: refusing non-root-owned peer binary {target:?} (uid {})",
                        meta.uid()
                    );
                    return PeerExeCheck::Untrusted;
                }
                // Cargo target builds: must be owned by our euid (not another user's tree).
                if is_cargo_target_bin_dir(parent) {
                    let our = unsafe { libc::geteuid() };
                    if meta.uid() != our {
                        idle_log::warn!(
                            "D-Bus auth check: refusing cargo-target peer {target:?} owned by uid {} (ours {our})",
                            meta.uid()
                        );
                        return PeerExeCheck::Untrusted;
                    }
                }
            }
            Err(e) => {
                idle_log::warn!("D-Bus auth check: cannot stat peer binary {target:?}: {e:?}");
                return PeerExeCheck::Untrusted;
            }
        }
    }

    PeerExeCheck::Trusted
}

fn is_system_bin_dir(parent: &str) -> bool {
    parent == "/usr/bin" || parent == "/usr/local/bin"
}

/// `…/target/debug` or `…/target/release` cargo output directories.
pub(super) fn is_cargo_target_bin_dir(parent: &str) -> bool {
    parent.ends_with("/target/debug") || parent.ends_with("/target/release")
}

fn same_dir_as_current_exe(target: &std::path::Path) -> bool {
    let Ok(current_exe) = std::env::current_exe() else {
        return false;
    };
    let Ok(current_canonical) = std::fs::canonicalize(current_exe) else {
        return false;
    };
    target.parent() == current_canonical.parent()
}

/// Read `/proc/<pid>/comm` (usually world-readable even when `exe` is not).
pub(super) fn peer_comm(pid: u32) -> Option<String> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = raw.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Match a `/proc/pid/comm` value against trusted peer basenames (15-char truncation).
pub(super) fn comm_matches_trusted(comm: &str) -> bool {
    let c = comm.trim();
    TRUSTED_CONTROL_PEERS.iter().any(|name| {
        if *name == c {
            return true;
        }
        // Kernel truncates task comm to COMM_MAX chars.
        name.len() > COMM_MAX && name.as_bytes().get(..COMM_MAX) == Some(c.as_bytes())
    })
}

pub(super) fn our_euid() -> Option<u32> {
    #[cfg(unix)]
    {
        Some(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        None
    }
}
