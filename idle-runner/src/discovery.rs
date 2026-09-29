use super::launcher::{ALLOWED_SAVERS, is_allowed_saver};
use std::path::{Component, Path, PathBuf};

/// Reject env-derived roots that are relative, empty, or contain `..`.
///
/// Absolute roots without `..` still undergo `canonicalize` + trust checks at
/// load time; this only blocks obvious traversal / relative injection via
/// `XDG_DATA_DIRS` / `XDG_DATA_HOME`.
pub(crate) fn is_safe_data_root(path: &str) -> bool {
    if path.is_empty() || path.contains('\0') {
        return false;
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    !p.components().any(|c| matches!(c, Component::ParentDir))
}

/// System-only plugin roots (Daemon mode). No `$HOME` / XDG user trees.
pub fn get_system_screensaver_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/libexec/idle/screensavers"),
        PathBuf::from("/usr/local/libexec/idle/screensavers"),
        PathBuf::from("/usr/libexec/idlescreen/screensavers"),
        PathBuf::from("/usr/local/libexec/idlescreen/screensavers"),
        PathBuf::from("/usr/libexec/trance/screensavers"),
        PathBuf::from("/usr/local/libexec/trance/screensavers"),
    ];

    let xdg_data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for part in xdg_data_dirs.split(':') {
        if is_safe_data_root(part) {
            // Only system-ish prefixes under XDG_DATA_DIRS
            if part.starts_with("/usr") {
                dirs.push(PathBuf::from(part).join("idle").join("screensavers"));
                dirs.push(PathBuf::from(part).join("trance").join("screensavers"));
            }
        }
    }
    dirs
}

/// User-writable plugin roots (Preview / explicit local only).
pub fn get_user_screensaver_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if is_safe_data_root(&xdg_data) {
            dirs.push(PathBuf::from(&xdg_data).join("idle").join("screensavers"));
            dirs.push(PathBuf::from(&xdg_data).join("idle").join("savers"));
            dirs.push(PathBuf::from(xdg_data).join("trance").join("screensavers"));
        }
    } else if let Ok(home) = std::env::var("HOME")
        && is_safe_data_root(&home)
    {
        let home_path = PathBuf::from(home);
        dirs.push(home_path.join(".config").join("idle").join("savers"));
        for brand in ["idle", "trance"] {
            dirs.push(
                home_path
                    .join(".local")
                    .join("share")
                    .join(brand)
                    .join("screensavers"),
            );
            dirs.push(
                home_path
                    .join(".local")
                    .join("share")
                    .join(brand)
                    .join("savers"),
            );
            dirs.push(
                home_path
                    .join(".local")
                    .join("libexec")
                    .join(brand)
                    .join("screensavers"),
            );
        }
    }
    dirs
}

/// All discovery dirs: system first, then user (Preview / listing).
///
/// **Daemon load** uses [`get_system_screensaver_dirs`] only via
/// `LaunchMode::Daemon` in the resolver.
pub fn get_screensaver_dirs() -> Vec<PathBuf> {
    let mut dirs = get_system_screensaver_dirs();
    dirs.extend(get_user_screensaver_dirs());
    dirs
}

/// Detects all screensavers by scanning the user and system directories for executables.
/// Automatically falls back to the built-in ALLOWED_SAVERS list.
pub fn detect_screensavers() -> Vec<String> {
    use std::collections::HashSet;

    // Built-in allowlist first for stable ordering / guaranteed presence.
    let mut savers: Vec<String> = ALLOWED_SAVERS.iter().map(|s| (*s).to_string()).collect();
    let mut seen: HashSet<String> = savers.iter().cloned().collect();

    for dir in get_screensaver_dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let is_so = entry.path().extension().is_some_and(|ext| ext == "so");
                let is_exec = metadata.permissions().mode() & 0o111 != 0;
                if !(is_so || is_exec) {
                    continue;
                }
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if !is_allowed_saver(name) {
                continue;
            }
            // sanitize is infallible when is_allowed_saver returned true
            let clean_name = super::launcher::sanitize_saver_name(name).unwrap_or_default();
            if clean_name.is_empty() {
                continue;
            }
            if seen.insert(clean_name.clone()) {
                savers.push(clean_name);
            }
        }
    }

    savers
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
