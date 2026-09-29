use super::*;

#[test]
fn sanitize_accepts_clean_names() {
    assert_eq!(
        sanitize_saver_name("security"),
        Some("security".to_string())
    );
    assert_eq!(
        sanitize_saver_name("/path/to/beams.scr"),
        Some("beams".to_string())
    );
    assert_eq!(
        sanitize_saver_name("screensaver-storm"),
        Some("storm".to_string())
    );
}

#[test]
fn sanitize_rejects_bad_names() {
    assert!(sanitize_saver_name("evil;rm -rf /").is_none());
    assert_eq!(
        sanitize_saver_name("../../etc/passwd"),
        Some("passwd".to_string())
    );
    assert_eq!(
        sanitize_saver_name("not-a-real-saver"),
        Some("not-a-real-saver".to_string())
    );
    assert!(sanitize_saver_name("").is_none());
}

#[test]
fn allowlist_blocks_unknown_savers() {
    assert!(!is_allowed_saver("not-a-real-saver"));
    assert!(!is_allowed_saver("passwd"));
    assert!(is_allowed_saver("cosmos"));
    assert!(resolve_saver_binary("evil-plugin", &LaunchMode::Daemon).is_err());
}

#[test]
fn allowlist_is_complete() {
    assert_eq!(ALLOWED_SAVERS.len(), 11);
    assert!(ALLOWED_SAVERS.contains(&"aurora"));
    assert!(ALLOWED_SAVERS.contains(&"beams"));
    assert!(ALLOWED_SAVERS.contains(&"storm"));
    assert!(ALLOWED_SAVERS.contains(&"hearth"));
    assert!(ALLOWED_SAVERS.contains(&"ripple"));
}

#[test]
fn is_trusted_plugin_path_rejects_nonexistent() {
    let p = std::path::Path::new("/nonexistent/foo.so");
    assert!(!is_trusted_plugin_path(p, &[]));
}

#[test]
fn is_trusted_plugin_path_rejects_when_not_in_trusted_dirs() {
    let p = std::path::Path::new("/nonexistent/foo.so");
    let trusted = vec![std::path::PathBuf::from("/also/nonexistent")];
    assert!(!is_trusted_plugin_path(p, &trusted));
}

#[test]
fn is_trusted_plugin_path_accepts_file_inside_trusted_dir() {
    let dir = crate::test_util::tempdir().expect("tempdir");
    let plugin = dir.path().join("libscreensaver_beams.so");
    std::fs::write(&plugin, b"fake").expect("write plugin");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&plugin).expect("meta").permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&plugin, perms).expect("chmod");
    }
    let trusted = vec![dir.path().to_path_buf()];
    assert!(is_trusted_plugin_path(&plugin, &trusted));
}

#[test]
fn is_trusted_plugin_path_rejects_sibling_outside_trust_root() {
    let trusted_root = crate::test_util::tempdir().expect("trusted");
    let other = crate::test_util::tempdir().expect("other");
    let plugin = other.path().join("libscreensaver_beams.so");
    std::fs::write(&plugin, b"fake").expect("write");
    let trusted = vec![trusted_root.path().to_path_buf()];
    assert!(!is_trusted_plugin_path(&plugin, &trusted));
}

#[cfg(unix)]
#[test]
fn is_trusted_plugin_path_rejects_world_writable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::test_util::tempdir().expect("tempdir");
    let plugin = dir.path().join("libscreensaver_beams.so");
    std::fs::write(&plugin, b"fake").expect("write");
    let mut perms = std::fs::metadata(&plugin).expect("meta").permissions();
    perms.set_mode(0o666); // world-writable
    std::fs::set_permissions(&plugin, perms).expect("chmod");
    let trusted = vec![dir.path().to_path_buf()];
    assert!(!is_trusted_plugin_path(&plugin, &trusted));
}

#[test]
fn sanitize_rejects_shell_metacharacters() {
    assert!(sanitize_saver_name("name|pipe").is_none());
    assert!(sanitize_saver_name("name&bg").is_none());
    assert!(sanitize_saver_name("name$dollar").is_none());
    assert!(sanitize_saver_name("name space").is_none());
}

#[test]
fn sanitize_accepts_alphanumeric_and_dash() {
    assert_eq!(sanitize_saver_name("abc-123"), Some("abc-123".to_string()));
    assert_eq!(sanitize_saver_name("ABC"), Some("ABC".to_string()));
}

#[test]
fn sanitize_strips_idle_saver_package_prefix() {
    assert_eq!(
        sanitize_saver_name("idle-saver-beams"),
        Some("beams".to_string())
    );
    assert_eq!(
        sanitize_saver_name("idle-saver-storm"),
        Some("storm".to_string())
    );
    assert_eq!(
        sanitize_saver_name("idle-saver-ripple.so"),
        Some("ripple".to_string())
    );
}

#[test]
fn sanitize_idle_saver_empty_stem_rejected() {
    // Prefix alone leaves empty stem after strip.
    assert!(sanitize_saver_name("idle-saver-").is_none());
    assert!(sanitize_saver_name("idle-saver").is_some()); // no prefix match (needs trailing -)
}

#[test]
fn sanitize_idle_saver_only_strips_once() {
    assert_eq!(
        sanitize_saver_name("idle-saver-idle-saver-beams"),
        Some("idle-saver-beams".to_string())
    );
}

#[test]
fn is_allowed_after_idle_saver_prefix_strip() {
    assert!(is_allowed_saver("idle-saver-beams"));
    assert!(is_allowed_saver("idle-saver-hearth"));
    assert!(!is_allowed_saver("idle-saver-evil"));
    assert!(!is_allowed_saver("idle-saver-"));
}

#[test]
fn sanitize_strips_screensaver_prefix_only() {
    assert_eq!(
        sanitize_saver_name("screensaver-bursts"),
        Some("bursts".to_string())
    );
    assert_eq!(
        sanitize_saver_name("screensaver-screensaver-x"),
        Some("screensaver-x".to_string())
    );
}

#[test]
fn is_allowed_saver_rejects_path_separators() {
    assert!(!is_allowed_saver("../beams"));
    assert!(!is_allowed_saver("dir\\beams"));
    assert!(!is_allowed_saver("/beams"));
}

#[test]
fn resolve_saver_binary_rejects_path_separators() {
    assert!(resolve_saver_binary("../beams", &LaunchMode::Daemon).is_err());
    assert!(resolve_saver_binary("dir\\beams", &LaunchMode::Daemon).is_err());
    assert!(resolve_saver_binary("/abs/beams", &LaunchMode::Daemon).is_err());
}

#[test]
fn resolve_saver_binary_rejects_unknown_saver() {
    assert!(resolve_saver_binary("not-in-allowlist", &LaunchMode::Daemon).is_err());
    assert!(resolve_saver_binary("", &LaunchMode::Daemon).is_err());
}

#[test]
fn launch_mode_clone_eq() {
    let a = LaunchMode::Daemon;
    let b = a.clone();
    assert_eq!(a, b);
    let c = LaunchMode::Preview;
    assert_ne!(a, c);
}

#[test]
fn plugin_error_display_includes_context() {
    let err = PluginError::NotAllowed("evil".to_string());
    let s = err.to_string();
    assert!(s.contains("evil"));
    let err = PluginError::PathTraversal;
    assert!(err.to_string().contains(".."));
}

#[test]
fn test_dev_plugin_dirs_env_behavior() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    use crate::launcher_resolve::dev_plugin_dirs;

    unsafe {
        std::env::set_var("IDLE_DEV_PLUGINS", "1");
    }
    let dirs_idle_env = dev_plugin_dirs("beams");
    assert!(!dirs_idle_env.is_empty());

    unsafe {
        std::env::remove_var("IDLE_DEV_PLUGINS");
    }
    let dirs_no_env = dev_plugin_dirs("beams");
    if cfg!(debug_assertions) {
        assert!(!dirs_no_env.is_empty());
    } else {
        assert!(dirs_no_env.is_empty());
    }
}

#[test]
fn test_immune_rail_saver_name_and_path_traversal() {
    let malicious = [
        "../../../etc/passwd",
        "../../sh",
        "/usr/bin/malicious",
        "; rm -rf / ;",
        "$(whoami)",
        "`whoami`",
        "beams\0malicious",
        "custom_untrusted_saver",
        &"A".repeat(4096),
    ];
    for input in malicious {
        assert!(!is_allowed_saver(input), "reject: {input:?}");
        assert!(
            resolve_saver_binary(input, &LaunchMode::Daemon).is_err(),
            "Err for: {input:?}"
        );
    }
    assert_eq!(sanitize_saver_name("; rm -rf / ;"), None);
    assert_eq!(sanitize_saver_name("$(whoami)"), None);
    assert_eq!(sanitize_saver_name("beams\0malicious"), None);
}
