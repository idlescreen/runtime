// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Path / name validation for IPC sockets and POSIX SHM objects.

/// POSIX SHM object names the daemon creates look like `/idle-shm-<id>-<idx>`.
/// Legacy `/trance-shm-…` names are still accepted so older peers can open.
/// Reject anything else so a compromised arg vector cannot open arbitrary objects.
pub fn is_valid_shm_name(name: &str) -> bool {
    let rest = if let Some(r) = name.strip_prefix("/idle-shm-") {
        r
    } else if let Some(r) = name.strip_prefix("/trance-shm-") {
        r
    } else {
        return false;
    };
    if rest.is_empty() || rest.len() > 64 {
        return false;
    }
    rest.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// UDS control paths must be absolute, end in `.sock`, stay under path limits,
/// and must not contain nulls or `..` segments.
pub fn is_plausible_socket_path(path: &str) -> bool {
    if path.is_empty() || path.len() >= 108 {
        return false;
    }
    if path.contains('\0') || !path.starts_with('/') || !path.ends_with(".sock") {
        return false;
    }
    if path.split('/').any(|seg| seg == "..") {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shm_name_accepts_daemon_format() {
        assert!(is_valid_shm_name("/idle-shm-1234-0"));
        assert!(is_valid_shm_name("/idle-shm-1-99"));
        assert!(is_valid_shm_name("/idle-shm-test_name-0"));
        assert!(is_valid_shm_name("/trance-shm-1234-0")); // legacy
        assert!(is_valid_shm_name(&format!("/idle-shm-{}", "a".repeat(64))));
    }

    #[test]
    fn shm_name_accepts_live_daemon_preview_format() {
        // Regression: hard-cut create uses /idle-shm-{pid}-{idx}; old allowlist
        // only accepted /trance-shm-* → preview aborted with "invalid shm name".
        assert!(is_valid_shm_name("/idle-shm-249870-0"));
        assert!(is_valid_shm_name("/idle-shm-249870-1"));
        assert!(is_valid_shm_name(&format!(
            "/idle-shm-{}-{}",
            std::process::id(),
            0
        )));
    }

    #[test]
    fn shm_name_rejects_traversal_and_oddities() {
        assert!(!is_valid_shm_name("idle-shm-1-0"));
        assert!(!is_valid_shm_name("/other-1-0"));
        assert!(!is_valid_shm_name("/idle-shm-../etc"));
        assert!(!is_valid_shm_name("/idle-shm-"));
        assert!(!is_valid_shm_name("/idle-shm-a/b"));
        assert!(!is_valid_shm_name("/idle-shm-a b"));
        assert!(!is_valid_shm_name("/idle-shm-a;b"));
        assert!(!is_valid_shm_name("/IDLE-SHM-1-0"));
        assert!(!is_valid_shm_name(&format!("/idle-shm-{}", "x".repeat(80))));
        assert!(!is_valid_shm_name(&format!("/idle-shm-{}", "x".repeat(65))));
    }

    #[test]
    fn socket_path_rejects_relative_and_dots() {
        assert!(is_plausible_socket_path("/run/user/1000/idle-uds-1-0.sock"));
        assert!(!is_plausible_socket_path("relative.sock"));
        assert!(!is_plausible_socket_path("/tmp/../etc/passwd.sock"));
        assert!(!is_plausible_socket_path("/tmp/foo"));
        assert!(!is_plausible_socket_path(""));
        assert!(!is_plausible_socket_path("/tmp/foo.sock\0x"));
        assert!(!is_plausible_socket_path(&format!(
            "/{}.sock",
            "a".repeat(108)
        )));
        assert!(!is_plausible_socket_path(&"x".repeat(108)));
    }

    #[test]
    fn socket_path_accepts_tmp_and_runtime() {
        assert!(is_plausible_socket_path("/tmp/idle-uds-1-0.sock"));
        assert!(is_plausible_socket_path(
            "/run/user/1000/idle-uds-42-1.sock"
        ));
        // sun_path is 108 bytes — length 107 must remain accepted.
        let ok_107 = format!("/{}.sock", "a".repeat(101));
        assert_eq!(ok_107.len(), 107);
        assert!(is_plausible_socket_path(&ok_107));
        let bad_108 = format!("/{}.sock", "a".repeat(102));
        assert_eq!(bad_108.len(), 108);
        assert!(!is_plausible_socket_path(&bad_108));
    }

    #[test]
    fn socket_path_rejects_dotdot_middle_segment() {
        assert!(!is_plausible_socket_path("/run/../user/x.sock"));
        assert!(!is_plausible_socket_path("/a/b/../c.sock"));
    }

    #[test]
    fn shm_rejects_absolute_system_paths_disguised() {
        assert!(!is_valid_shm_name("/etc/passwd"));
        assert!(!is_valid_shm_name("/idle-shm-/etc/passwd"));
        assert!(!is_valid_shm_name("/dev/shm/idle-shm-1-0"));
        assert!(!is_valid_shm_name("idle-shm-1-0"));
    }

    #[test]
    fn shm_accepts_only_prefix_forms() {
        assert!(is_valid_shm_name("/idle-shm-0-0"));
        assert!(is_valid_shm_name("/trance-shm-0-0"));
        assert!(!is_valid_shm_name("/idlescreen-shm-0-0"));
        assert!(!is_valid_shm_name("/idle_shm_0_0"));
    }

    #[test]
    fn socket_rejects_non_sock_suffix() {
        assert!(!is_plausible_socket_path("/run/user/1000/idle-uds-1-0"));
        assert!(!is_plausible_socket_path(
            "/run/user/1000/idle-uds-1-0.socket"
        ));
        assert!(is_plausible_socket_path("/run/user/1000/idle-uds-1-0.sock"));
    }
}

#[cfg(test)]
mod proptests {
    use super::is_valid_shm_name;

    #[test]
    fn idle_shm_pid_idx_always_valid() {
        // Deterministic spread over the pid/idx domain (was proptest).
        let mut x = 0x5AFE_0001u64;
        for _ in 0..64 {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let pid = 1 + (x.wrapping_mul(0x2545_F491_4F6C_DD1D) % u32::MAX as u64) as u32;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let idx = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) % 1000) as u32;
            let name = format!("/idle-shm-{pid}-{idx}");
            assert!(
                is_valid_shm_name(&name),
                "daemon format must be valid: {name}"
            );
        }
    }

    #[test]
    fn trance_legacy_pid_idx_always_valid() {
        let mut x = 0x5AFE_0002u64;
        for _ in 0..64 {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let pid = 1 + (x.wrapping_mul(0x2545_F491_4F6C_DD1D) % u32::MAX as u64) as u32;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let idx = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) % 1000) as u32;
            let name = format!("/trance-shm-{pid}-{idx}");
            assert!(is_valid_shm_name(&name));
        }
    }
}
