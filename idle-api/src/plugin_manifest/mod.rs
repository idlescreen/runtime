// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! `.idleplugin.toml` capability manifest (schema v1, `DECISION-MANIFEST-01`).
//!
//! Every plugin library ships a sibling manifest declaring its identity,
//! entry point, requested capabilities and sandbox profile. The host refuses
//! to load a plugin whose manifest is missing, unparseable, or inconsistent
//! with the resolved library — see `idle-runner`'s `plugin_session::loading`.
//!
//! The manifest is named `<stem>.idleplugin.toml` rather than a bare
//! `.idleplugin.toml` because every saver installs into one shared directory
//! (`/usr/libexec/idle/screensavers/`), where a single fixed name would collide.

mod schema;

pub mod host;
pub mod signature;

pub use schema::{Capabilities, Dependencies, Entry, HeadlessRender, Manifest, Sandbox};

use std::path::{Path, PathBuf};

/// Manifest schema version understood by this host.
pub const SCHEMA_VERSION: u32 = 1;

/// Why a manifest was rejected. Every variant is fail-closed at the loader.
#[derive(Debug)]
pub enum ManifestError {
    Missing(PathBuf),
    Parse { path: PathBuf, source: String },
    UnsupportedSchemaVersion { path: PathBuf, found: u32 },
    InvalidPluginId(PathBuf, String),
    Invalid(String, PathBuf),
    SignatureMissing(String),
    SignatureInvalid(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(p) => write!(
                f,
                "no manifest at {} (plugin refused; set IDLE_ALLOW_UNSIGNED_PLUGINS=1 to override)",
                p.display()
            ),
            Self::Parse { path, source } => {
                write!(f, "malformed manifest {}: {source}", path.display())
            }
            Self::UnsupportedSchemaVersion { path, found } => write!(
                f,
                "manifest {} declares schema_version {found}, host supports {SCHEMA_VERSION}",
                path.display()
            ),
            Self::InvalidPluginId(p, id) => write!(
                f,
                "manifest {} has invalid plugin_id '{id}' (expected reverse-DNS, e.g. io.github.x.y)",
                p.display()
            ),
            Self::Invalid(m, p) => write!(f, "manifest {} invalid: {m}", p.display()),
            Self::SignatureMissing(m) => write!(
                f,
                "manifest signature required but missing: {m} (set IDLE_REQUIRE_MANIFEST_SIGNATURE=1 to enforce)"
            ),
            Self::SignatureInvalid(m) => write!(f, "manifest signature invalid: {m}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Path of the manifest that belongs to `plugin_path`.
pub fn sibling_path(plugin_path: &Path) -> PathBuf {
    let stem = plugin_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin");
    plugin_path.with_file_name(format!("{stem}.idleplugin.toml"))
}

/// Read and parse the manifest belonging to `plugin_path`. Does not validate.
pub fn load_for(plugin_path: &Path) -> Result<Manifest, ManifestError> {
    let path = sibling_path(plugin_path);
    let text = std::fs::read_to_string(&path).map_err(|_| ManifestError::Missing(path.clone()))?;
    let mut manifest = parse_str(&text, &path)?;
    manifest.source_path = path;
    Ok(manifest)
}

/// Parse manifest text. `path` is used only for error reporting.
pub fn parse_str(text: &str, path: &Path) -> Result<Manifest, ManifestError> {
    let doc = crate::toml::parse(text).map_err(|e| ManifestError::Parse {
        path: path.to_path_buf(),
        source: e.to_string(),
    })?;
    Manifest::from_value(&doc).map_err(|e| ManifestError::Parse {
        path: path.to_path_buf(),
        source: e,
    })
}

/// Enforce schema, identity and entry-point invariants.
pub fn validate(manifest: &Manifest) -> Result<(), ManifestError> {
    let path = manifest.source_path.clone();
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(ManifestError::UnsupportedSchemaVersion {
            path,
            found: manifest.schema_version,
        });
    }
    if !is_reverse_dns(&manifest.plugin_id) {
        return Err(ManifestError::InvalidPluginId(
            path,
            manifest.plugin_id.clone(),
        ));
    }
    let invalid = |m: &str| Err(ManifestError::Invalid(m.to_string(), path.clone()));
    if manifest.plugin_version.trim().is_empty() {
        return invalid("plugin_version must not be empty");
    }
    if !matches!(manifest.entry.runtime.as_str(), "native" | "wasm") {
        return invalid("entry.runtime must be \"native\" or \"wasm\"");
    }
    if manifest.entry.library.trim().is_empty() || manifest.entry.library.contains('/') {
        return invalid("entry.library must be a bare file name");
    }
    if !matches!(
        manifest.sandbox.profile.as_str(),
        "minimal" | "renderer" | "asset-author" | "experimental" | "seatbelt" | "appcontainer"
    ) {
        return invalid("sandbox.profile is not a known profile");
    }
    validate_capability_paths(
        &manifest.capabilities.filesystem_read,
        "filesystem_read",
        &invalid,
    )?;
    validate_capability_paths(
        &manifest.capabilities.filesystem_write,
        "filesystem_write",
        &invalid,
    )?;
    Ok(())
}

/// Each declared FS path must be absolute, non-empty, free of `..` and NUL —
/// relative paths would resolve against an attacker-influenced cwd and `..`
/// would widen the sandbox beyond what the manifest claims.
fn validate_capability_paths<F>(
    paths: &[String],
    field: &str,
    invalid: &F,
) -> Result<(), ManifestError>
where
    F: Fn(&str) -> Result<(), ManifestError>,
{
    use std::path::{Component, Path};
    for raw in paths {
        let p = Path::new(raw);
        if raw.is_empty() {
            return invalid(&format!("{field}: empty path"));
        }
        if raw.contains('\0') {
            return invalid(&format!("{field}: NUL byte in path"));
        }
        if !p.is_absolute() {
            return invalid(&format!("{field}: path must be absolute: {raw}"));
        }
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return invalid(&format!("{field}: path contains '..': {raw}"));
        }
    }
    Ok(())
}

/// Reverse-DNS check: three or more non-empty dot-separated labels.
fn is_reverse_dns(id: &str) -> bool {
    let labels: Vec<&str> = id.split('.').collect();
    labels.len() >= 3
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}
