// SPDX-License-Identifier: Apache-2.0

use super::auth_peer::{
    PeerExeCheck, TRUSTED_CONTROL_PEERS, check_peer_exe, comm_matches_trusted, peer_comm,
    peer_exe_basename,
};
use super::*;

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn clear_trust_all_env() {
    // Other tests may set these; deny-policy tests must start clean.
    unsafe {
        std::env::remove_var("IDLE_DBUS_TRUST_ALL");
        std::env::remove_var("IDLE_STRICT_CONTROL");
    }
}

#[test]
fn strict_control_denies_comm_fallback_when_exe_unreadable() {
    let _guard = env_lock();
    clear_trust_all_env();
    // Missing PID → Unreadable exe; without strict, comm path still fails (no comm).
    // With strict, deny even before comm is considered — same outcome for missing pid.
    unsafe {
        std::env::set_var("IDLE_STRICT_CONTROL", "1");
    }
    assert!(
        !is_trusted_control_peer(u32::MAX, Some(unsafe { libc::geteuid() }), ":1.strict"),
        "strict control must deny unreadable exe"
    );
    assert!(
        strict_control_enabled(),
        "IDLE_STRICT_CONTROL=1 must enable strict mode"
    );
    unsafe {
        std::env::remove_var("IDLE_STRICT_CONTROL");
    }
    assert!(!strict_control_enabled());
}

#[test]
fn trusted_peer_names_are_fixed() {
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idlescreen"));
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idle-cli"));
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idle-tui"));
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idlescreen-applet"));
    // /usr/bin/idle is python3-idle on Fedora — must not be trusted for D-Bus control.
    assert!(!TRUSTED_CONTROL_PEERS.contains(&"idle"));
    assert!(!TRUSTED_CONTROL_PEERS.contains(&"bash"));
    assert!(!TRUSTED_CONTROL_PEERS.contains(&"python3"));
    assert!(!TRUSTED_CONTROL_PEERS.contains(&"idle-daemon"));
}

#[test]
fn trusted_peer_names_fit_linux_comm() {
    // Kernel task comm is 15 visible chars; long basenames match via prefix truncation.
    for name in TRUSTED_CONTROL_PEERS {
        assert!(
            comm_matches_trusted(name),
            "{name:?} should match full name"
        );
        if name.len() > 15 {
            let trunc = &name[..15];
            assert!(
                comm_matches_trusted(trunc),
                "{name:?} truncated to {trunc:?} should match"
            );
        }
    }
}

#[test]
fn peer_uid_must_match_ours() {
    let _guard = env_lock();
    clear_trust_all_env();
    // Policy: deny when Unix UID credential is unavailable (cross-user / incomplete).
    assert!(!is_trusted_control_peer(std::process::id(), None, ":1.1"));
    assert!(!is_trusted_control_peer(u32::MAX, None, ":1.missing"));
}

#[test]
fn missing_uid_denied_even_for_self_pid() {
    let _guard = env_lock();
    clear_trust_all_env();
    // Same process, missing UID still deny — UID is mandatory, not optional.
    let pid = std::process::id();
    assert!(!is_trusted_control_peer(pid, None, ":1.self"));
}

#[test]
fn comm_matches_trusted_exact_and_truncated() {
    assert!(comm_matches_trusted("idlescreen"));
    assert!(comm_matches_trusted("idle-tui"));
    assert!(comm_matches_trusted("  idlescreen  "));
    // idlescreen-applet is 17 chars → kernel comm is first 15.
    assert!(comm_matches_trusted("idlescreen-appl"));
    // Bare "idle" is Fedora python3-idle — must NOT be a control peer.
    assert!(!comm_matches_trusted("idle"));
    assert!(!comm_matches_trusted("  idle  "));
    assert!(!comm_matches_trusted("idlescreen-tui")); // wrong basename (real is idle-tui)
    assert!(!comm_matches_trusted("trance"));
    assert!(!comm_matches_trusted("trance-applet"));
    assert!(!comm_matches_trusted("bash"));
    assert!(!comm_matches_trusted("python3"));
    assert!(!comm_matches_trusted(""));
    assert!(!comm_matches_trusted("   "));
    assert!(!comm_matches_trusted("idle-cli-extra-long-name"));
    assert!(!comm_matches_trusted("idlescreen-extra"));
}

#[test]
fn peer_comm_of_self_is_readable() {
    let pid = std::process::id();
    let comm = peer_comm(pid);
    assert!(comm.is_some(), "expected /proc/self/comm to be readable");
    assert!(!comm.unwrap_or_default().is_empty());
}

#[test]
fn peer_comm_of_missing_pid_is_none() {
    assert!(peer_comm(u32::MAX).is_none());
    assert!(peer_exe_basename(u32::MAX).is_none());
    assert!(matches!(check_peer_exe(u32::MAX), PeerExeCheck::Unreadable));
}

#[test]
fn wrong_uid_denied_even_if_process_exists() {
    #[cfg(unix)]
    {
        let pid = std::process::id();
        let our = unsafe { libc::geteuid() };
        // Synthetic other uid — must deny before path checks matter.
        assert!(!is_trusted_control_peer(
            pid,
            Some(our.wrapping_add(12345).max(1)),
            ":1.99"
        ));
    }
}

#[test]
fn dbus_trust_all_env_only_in_debug_builds() {
    let _guard = env_lock();
    clear_trust_all_env();
    // In release, env escape hatch is hard-disabled; in debug it may open.
    let prior_idle = std::env::var("IDLE_DBUS_TRUST_ALL").ok();
    unsafe {
        std::env::set_var("IDLE_DBUS_TRUST_ALL", "1");
    }
    let accepted = is_trusted_control_peer(u32::MAX, None, ":1.trust");
    if cfg!(debug_assertions) {
        assert!(accepted, "debug build should honor IDLE_DBUS_TRUST_ALL=1");
    } else {
        assert!(!accepted, "release must ignore IDLE_DBUS_TRUST_ALL");
    }
    match prior_idle {
        Some(v) => unsafe {
            std::env::set_var("IDLE_DBUS_TRUST_ALL", v);
        },
        None => unsafe {
            std::env::remove_var("IDLE_DBUS_TRUST_ALL");
        },
    }
}

#[test]
fn untrusted_basename_never_matches_comm_policy() {
    for bad in [
        "sh",
        "curl",
        "systemd",
        "idle-daemon",
        "idle_daemon",
        "trance",
        "python3",
        "bash",
        "zsh",
        "node",
        "cargo",
        "firefox",
        "idle", // Fedora python3-idle IDE
        "Idle",
        "IDLE",
        "idlescreen-daemon",
        "idlescreen-extra",
        "idlescreen-tui", // wrong; real is idle-tui
        "com.system76.CosmicAppletIdle",
    ] {
        assert!(!comm_matches_trusted(bad), "{bad} must not be trusted");
    }
}

#[test]
fn trusted_control_peers_exact_set() {
    // Package-gate contract: only these four control clients.
    assert_eq!(TRUSTED_CONTROL_PEERS.len(), 4);
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idlescreen"));
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idle-cli"));
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idle-tui"));
    assert!(TRUSTED_CONTROL_PEERS.contains(&"idlescreen-applet"));
}

#[test]
fn cargo_target_bin_dirs_are_recognized() {
    use super::auth_peer::is_cargo_target_bin_dir;
    assert!(is_cargo_target_bin_dir(
        "/home/jeryd/Projects/idlescreen/runtime/target/release"
    ));
    assert!(is_cargo_target_bin_dir(
        "/home/jeryd/Projects/idlescreen/runtime/target/debug"
    ));
    assert!(!is_cargo_target_bin_dir("/usr/bin"));
    assert!(!is_cargo_target_bin_dir("/tmp/idlescreen"));
    assert!(!is_cargo_target_bin_dir("/home/x/target/release/foo"));
    assert!(!is_cargo_target_bin_dir("/home/x/not-target/release"));
}

#[test]
fn applet_comm_truncation_still_trusted() {
    // idlescreen-applet is 17 chars → kernel comm first 15.
    assert!(comm_matches_trusted("idlescreen-appl"));
    assert!(!comm_matches_trusted("idlescreen-app")); // too short / wrong
}

#[test]
fn security_reject_path_like_comms() {
    for bad in [
        "../idlescreen",
        "/usr/bin/idlescreen",
        "idlescreen;rm",
        "idlescreen\n",
        " idlescreen ",
    ] {
        // Trim only applies inside comm_matches for spaces; path forms must fail.
        let trimmed = bad.trim();
        if trimmed == "idlescreen" {
            continue;
        }
        assert!(
            !comm_matches_trusted(trimmed) || trimmed == "idlescreen",
            "{bad:?}"
        );
    }
    // Explicit path forms without trim equality
    assert!(!comm_matches_trusted("../idlescreen"));
    assert!(!comm_matches_trusted("/usr/bin/idlescreen"));
}
