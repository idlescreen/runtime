// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Path search helpers for trusted screensaver plugin libraries.

use std::path::{Path, PathBuf};

use crate::launcher::{LaunchMode, sanitize_saver_name};
use crate::launcher_trust::is_trusted_plugin_path_cached;

/// Candidate basenames for a cleaned saver name under a plugin directory.
pub(crate) fn plugin_candidate_names(clean: &str) -> [String; 3] {
    [
        format!("libscreensaver_{clean}.so"),
        format!("lib{clean}.so"),
        clean.to_string(),
    ]
}

/// True when local development plugin trees may be searched.
///
/// Debug builds: always on. Release: **never** via env (Preview can still use
/// user XDG dirs from discovery; `~/Projects/...` trees are debug-only).
fn dev_plugins_env_enabled() -> bool {
    cfg!(debug_assertions)
}

pub(crate) fn dev_plugin_dirs(clean: &str) -> Vec<PathBuf> {
    if !dev_plugins_env_enabled() {
        return Vec::new();
    }
    let Ok(home) = std::env::var("HOME") else {
        return Vec::new();
    };
    let projects = PathBuf::from(home).join("Projects");
    // Prefer crateria/ layout; keep ubermetroid/ for local checkouts during the rebrand.
    let plugin_roots = [
        projects.join("crateria").join("trance-plugins"),
        projects.join("ubermetroid").join("trance-plugins"),
    ];
    let mut dirs = Vec::new();
    for root in plugin_roots {
        dirs.push(root.join("target").join("release"));
        dirs.push(root.join("target").join("debug"));
        dirs.push(root.join(clean).join("target").join("release"));
        dirs.push(root.join(clean).join("target").join("debug"));
    }
    dirs.push(
        projects
            .join(format!("trance-plugin-{clean}"))
            .join("target")
            .join("release"),
    );
    dirs.push(
        projects
            .join(format!("trance-plugin-{clean}"))
            .join("target")
            .join("debug"),
    );
    dirs
}

fn trusted_plugin_dirs(clean: &str, mode: &LaunchMode) -> Vec<PathBuf> {
    match mode {
        LaunchMode::Daemon => crate::discovery::get_system_screensaver_dirs(),
        LaunchMode::Preview => {
            let mut dirs = crate::discovery::get_screensaver_dirs();
            dirs.extend(dev_plugin_dirs(clean));
            dirs
        }
    }
}

fn find_candidate_in_dir(base: &Path, candidates: &[String]) -> Option<PathBuf> {
    for candidate in candidates {
        let path = base.join(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Validate `name` is not a path and maps to a clean allowlisted basename.
pub(crate) fn cleaned_allowed_name(name: &str) -> std::io::Result<String> {
    if name.contains('/') || name.contains('\\') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("saver name must not be a path: {name}"),
        ));
    }
    let clean = sanitize_saver_name(name).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown or invalid screensaver name: {name}"),
        )
    })?;
    if !crate::launcher::ALLOWED_SAVERS.contains(&clean.as_str()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("screensaver '{clean}' is not in the trusted allowlist"),
        ));
    }
    Ok(clean)
}

/// Search trusted (and optionally dev) plugin dirs for a library path.
pub(crate) fn search_trusted_plugin(clean: &str, mode: &LaunchMode) -> std::io::Result<PathBuf> {
    let candidates = plugin_candidate_names(clean);
    let trusted_dirs = trusted_plugin_dirs(clean, mode);
    // Canonicalize trust roots once — not per candidate file.
    let canonical_trusted: Vec<PathBuf> = trusted_dirs
        .iter()
        .filter_map(|dir| std::fs::canonicalize(dir).ok())
        .collect();
    let dev_dirs = dev_plugin_dirs(clean);
    let search_order: Vec<&Path> = if *mode == LaunchMode::Preview {
        trusted_dirs
            .iter()
            .map(|p| p.as_path())
            .chain(dev_dirs.iter().map(|p| p.as_path()))
            .collect()
    } else {
        trusted_dirs.iter().map(|p| p.as_path()).collect()
    };

    for base in search_order {
        if let Some(path) = find_candidate_in_dir(base, &candidates)
            && is_trusted_plugin_path_cached(&path, &canonical_trusted)
        {
            return Ok(path);
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("no trusted plugin found for saver '{clean}' (mode {mode:?})"),
    ))
}
