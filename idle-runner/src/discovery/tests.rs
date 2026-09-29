// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn system_dirs_precede_user_dirs() {
    // With HOME set and no XDG_DATA_HOME, /usr paths must appear before ~/.local.
    let dirs = get_screensaver_dirs();
    let usr = dirs.iter().position(|p| p.starts_with("/usr"));
    let home = dirs
        .iter()
        .position(|p| p.components().any(|c| c.as_os_str() == ".local"));
    assert!(usr.is_some(), "expected at least one /usr plugin dir");
    if let (Some(u), Some(h)) = (usr, home) {
        assert!(
            u < h,
            "system dirs must be searched before user dirs: {dirs:?}"
        );
    }
}

#[test]
fn idle_system_path_is_first() {
    let dirs = get_screensaver_dirs();
    assert_eq!(
        dirs.first().map(|p| p.as_os_str()),
        Some(std::ffi::OsStr::new("/usr/libexec/idle/screensavers")),
        "canonical idle path must win over legacy trees: {dirs:?}"
    );
}

#[test]
fn legacy_trance_path_still_searched() {
    let dirs = get_screensaver_dirs();
    assert!(
        dirs.iter().any(|p| p.ends_with("trance/screensavers")
            || p.as_os_str() == "/usr/libexec/trance/screensavers"),
        "legacy trance plugin trees must remain for upgrades: {dirs:?}"
    );
}

#[test]
fn safe_data_root_rejects_relative_and_dotdot() {
    assert!(is_safe_data_root("/usr/share"));
    assert!(is_safe_data_root("/home/user/.local/share"));
    assert!(!is_safe_data_root(""));
    assert!(!is_safe_data_root("relative/path"));
    assert!(!is_safe_data_root("./local"));
    assert!(!is_safe_data_root("~/share"));
    assert!(!is_safe_data_root("/usr/share/../etc"));
    assert!(!is_safe_data_root("/tmp/\0evil"));
    assert!(!is_safe_data_root(".."));
}

#[test]
fn safe_data_root_accepts_absolute_without_parent() {
    assert!(is_safe_data_root("/var/lib/idle"));
    assert!(is_safe_data_root("/home/user"));
    // Absolute with redundant segments but no ParentDir is still absolute-safe;
    // canonicalize+trust happens later at load.
    assert!(is_safe_data_root("/usr/./share"));
}

#[test]
fn relative_xdg_data_home_not_injected_into_dirs() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    let prior_home = std::env::var("XDG_DATA_HOME").ok();
    let prior_xdg = std::env::var("XDG_DATA_DIRS").ok();
    unsafe {
        std::env::set_var("XDG_DATA_HOME", "relative/evil-home");
        std::env::set_var("XDG_DATA_DIRS", "/usr/share");
    }
    let dirs = get_screensaver_dirs();
    for p in &dirs {
        let s = p.to_string_lossy();
        assert!(
            !s.contains("relative/evil-home") && !s.contains("evil-home"),
            "relative XDG_DATA_HOME leaked into plugin search: {s}"
        );
    }
    // Absolute XDG_DATA_DIRS entry must still expand.
    assert!(
        dirs.iter()
            .any(|p| p.ends_with("idle/screensavers") && p.starts_with("/usr/share")),
        "expected /usr/share idle path: {dirs:?}"
    );
    restore_env("XDG_DATA_HOME", prior_home);
    restore_env("XDG_DATA_DIRS", prior_xdg);
}

#[test]
fn relative_xdg_data_dirs_entries_skipped() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    let prior = std::env::var("XDG_DATA_DIRS").ok();
    unsafe {
        std::env::set_var(
            "XDG_DATA_DIRS",
            "relative/path:/usr/share:../escape:/tmp/../etc",
        );
    }
    let dirs = get_screensaver_dirs();
    for p in &dirs {
        let s = p.to_string_lossy();
        assert!(
            !s.contains("relative/path") && !s.contains("../escape") && !s.contains("/etc/idle"),
            "unsafe XDG_DATA_DIRS segment leaked: {s}"
        );
    }
    assert!(
        dirs.iter().any(|p| p.starts_with("/usr/share")),
        "absolute safe segment must remain: {dirs:?}"
    );
    restore_env("XDG_DATA_DIRS", prior);
}

#[test]
fn relative_home_not_used_as_user_root() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    let prior_home = std::env::var("HOME").ok();
    let prior_xdg = std::env::var("XDG_DATA_HOME").ok();
    unsafe {
        std::env::remove_var("XDG_DATA_HOME");
        std::env::set_var("HOME", "not/absolute");
    }
    let dirs = get_screensaver_dirs();
    for p in &dirs {
        let s = p.to_string_lossy();
        assert!(
            !s.contains("not/absolute") && !s.starts_with("not/"),
            "relative HOME leaked into plugin search: {s}"
        );
    }
    // System paths still present.
    assert!(dirs.iter().any(|p| p.starts_with("/usr")));
    restore_env("HOME", prior_home);
    restore_env("XDG_DATA_HOME", prior_xdg);
}

#[test]
fn detect_screensavers_includes_allowlist() {
    let _g = crate::ENV_LOCK.lock().unwrap();
    let savers = detect_screensavers();
    assert!(!savers.is_empty());
    for name in ALLOWED_SAVERS {
        assert!(
            savers.iter().any(|s| s == *name),
            "allowlist member {name} missing from {savers:?}"
        );
    }
}

fn restore_env(key: &str, prior: Option<String>) {
    match prior {
        Some(v) => unsafe {
            std::env::set_var(key, v);
        },
        None => unsafe {
            std::env::remove_var(key);
        },
    }
}
