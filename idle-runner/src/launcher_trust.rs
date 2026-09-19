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
        if let Ok(meta) = std::fs::metadata(&canonical) {
            // Reject world-writable plugins.
            if meta.permissions().mode() & 0o002 != 0 {
                idle_log::warn!(
                    target: "plugin",
                    path = %canonical.display(),
                    "refusing world-writable plugin library"
                );
                return false;
            }
            // System packages live under /usr; require root or overflow (65534) ownership
            // since root is mapped to overflow UID inside user namespaces.
            if canonical.starts_with("/usr") && meta.uid() != 0 && meta.uid() != 65534 {
                idle_log::warn!(
                    target: "plugin",
                    path = %canonical.display(),
                    uid = meta.uid(),
                    "refusing non-root-owned system plugin library"
                );
                return false;
            }
        }
    }

    true
}
