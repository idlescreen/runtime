//! Trusted plugin path validation (permissions and directory confinement).

use std::path::{Path, PathBuf};

/// True when `path` resolves under one of the trusted plugin directories.
///
/// Also rejects world-writable plugin files (mode `o+w`), which would let
/// any local user plant a payload next to a legitimate allowlisted name.
pub fn is_trusted_plugin_path(path: &Path, trusted_dirs: &[PathBuf]) -> bool {
    let canonical_dirs: Vec<PathBuf> = trusted_dirs
        .iter()
        .filter_map(|dir| std::fs::canonicalize(dir).ok())
        .collect();
    is_trusted_plugin_path_cached(path, &canonical_dirs)
}

/// Like [`is_trusted_plugin_path`] but reuses already-canonicalized trust roots
/// (avoids repeated `canonicalize` syscalls while scanning candidates).
pub(crate) fn is_trusted_plugin_path_cached(
    path: &Path,
    canonical_trusted_dirs: &[PathBuf],
) -> bool {
    let canonical = match std::fs::canonicalize(path) {
        Ok(path) => path,
        Err(_) => return false,
    };
    if !canonical_trusted_dirs
        .iter()
        .any(|canonical_dir| canonical.starts_with(canonical_dir))
    {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = match std::fs::metadata(&canonical) {
            Ok(m) => m,
            Err(e) => {
                idle_log::warn!(path = %canonical.display(), "failed to stat plugin file: {e}");
                return false;
            }
        };
        // Reject group/world-writable plugins (mode & 0o022 != 0).
        if meta.permissions().mode() & 0o022 != 0 {
            idle_log::warn!(
                target: "plugin",
                path = %canonical.display(),
                "refusing group- or world-writable plugin library"
            );
            return false;
        }
        // Enforce root (0), current user, or user-namespace overflow (65534) ownership.
        let uid = meta.uid();
        // SAFETY: getuid() is a pure query syscall with no pointer side effects.
        let current_uid = unsafe { libc::getuid() };
        if uid != 0 && uid != current_uid && uid != 65534 {
            idle_log::warn!(
                target: "plugin",
                path = %canonical.display(),
                uid,
                current_uid,
                "refusing untrusted plugin ownership"
            );
            return false;
        }
    }

    true
}
