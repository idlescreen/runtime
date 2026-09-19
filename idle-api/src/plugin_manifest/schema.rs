// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! `.idleplugin.toml` schema v1, decoded by hand from [`crate::toml::Value`].
//!
//! Unknown fields are ignored by design (forward-compat): a v1 host tolerates
//! manifests written against a later minor revision instead of failing closed.

use crate::toml::Value;
use std::path::PathBuf;

/// Parsed `.idleplugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub schema_version: u32,
    pub plugin_id: String,
    pub plugin_version: String,
    pub api_version: u32,
    pub entry: Entry,
    pub capabilities: Capabilities,
    pub sandbox: Sandbox,
    pub dependencies: Dependencies,
    pub headless_render: HeadlessRender,
    /// Path the manifest was read from. Not part of the TOML surface.
    pub source_path: PathBuf,
}

/// Plugin entry point: which runtime loads it, and which library file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub runtime: String,
    pub library: String,
}

/// Capabilities the plugin requests. Absent = denied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub network: bool,
    pub audio_capture: bool,
    pub audio_output: bool,
    pub filesystem_read: Vec<String>,
    pub filesystem_write: Vec<String>,
}

/// Requested sandbox profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    pub profile: String,
}

/// Advisory dependency lists (not enforced in Sprint 02).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependencies {
    pub native: Vec<String>,
    pub wasm: Vec<String>,
}

/// Hints for the offline/headless renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessRender {
    pub default_fps: u32,
    pub deterministic_seed: bool,
    pub gpu_optional: bool,
}

mod fields;
use fields::*;
impl Manifest {
    /// Decode from a parsed TOML document (serde `Deserialize` equivalent).
    pub fn from_value(doc: &Value) -> Result<Self, String> {
        let entry_v = doc.get("entry").ok_or_else(|| missing("entry"))?;
        Ok(Manifest {
            schema_version: req_u32(doc, "schema_version")?,
            plugin_id: req_str(doc, "plugin_id")?,
            plugin_version: req_str(doc, "plugin_version")?,
            api_version: req_u32(doc, "api_version")?,
            entry: Entry::from_value(entry_v)?,
            capabilities: match doc.get("capabilities") {
                None => Capabilities::default(),
                Some(v) => Capabilities::from_value(v)?,
            },
            sandbox: match doc.get("sandbox") {
                None => Sandbox::default(),
                Some(v) => Sandbox::from_value(v)?,
            },
            dependencies: match doc.get("dependencies") {
                None => Dependencies::default(),
                Some(v) => Dependencies::from_value(v)?,
            },
            headless_render: match doc.get("headless_render") {
                None => HeadlessRender::default(),
                Some(v) => HeadlessRender::from_value(v)?,
            },
            source_path: PathBuf::new(),
        })
    }
}

impl Entry {
    fn from_value(t: &Value) -> Result<Self, String> {
        Ok(Entry {
            runtime: req_str(t, "runtime")?,
            library: req_str(t, "library")?,
        })
    }
}

impl Capabilities {
    fn from_value(t: &Value) -> Result<Self, String> {
        Ok(Capabilities {
            network: opt_bool(t, "network", false)?,
            audio_capture: opt_bool(t, "audio_capture", false)?,
            audio_output: opt_bool(t, "audio_output", false)?,
            filesystem_read: opt_str_list(t, "filesystem_read")?,
            filesystem_write: opt_str_list(t, "filesystem_write")?,
        })
    }
}

impl Sandbox {
    fn from_value(t: &Value) -> Result<Self, String> {
        Ok(Sandbox {
            profile: opt_str(t, "profile", default_profile())?,
        })
    }
}

impl Dependencies {
    fn from_value(t: &Value) -> Result<Self, String> {
        Ok(Dependencies {
            native: opt_str_list(t, "native")?,
            wasm: opt_str_list(t, "wasm")?,
        })
    }
}

impl HeadlessRender {
    fn from_value(t: &Value) -> Result<Self, String> {
        Ok(HeadlessRender {
            default_fps: opt_u32(t, "default_fps", 60)?,
            deterministic_seed: opt_bool(t, "deterministic_seed", true)?,
            gpu_optional: opt_bool(t, "gpu_optional", true)?,
        })
    }
}

// ---- TOML emission (was serde Serialize / toml::to_string) ----

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn str_list(xs: &[String]) -> String {
    let inner: Vec<String> = xs.iter().map(|s| format!("\"{}\"", esc(s))).collect();
    format!("[{}]", inner.join(", "))
}

impl Manifest {
    /// Serialize back to `.idleplugin.toml` text (serde `Serialize`
    /// equivalent; `source_path` is not part of the TOML surface).
    pub fn to_toml_string(&self) -> String {
        format!(
            "schema_version = {}\n\
             plugin_id = \"{}\"\n\
             plugin_version = \"{}\"\n\
             api_version = {}\n\
             \n\
             [entry]\n\
             runtime = \"{}\"\n\
             library = \"{}\"\n\
             \n\
             [capabilities]\n\
             network = {}\n\
             audio_capture = {}\n\
             audio_output = {}\n\
             filesystem_read = {}\n\
             filesystem_write = {}\n\
             \n\
             [sandbox]\n\
             profile = \"{}\"\n\
             \n\
             [dependencies]\n\
             native = {}\n\
             wasm = {}\n\
             \n\
             [headless_render]\n\
             default_fps = {}\n\
             deterministic_seed = {}\n\
             gpu_optional = {}\n",
            self.schema_version,
            esc(&self.plugin_id),
            esc(&self.plugin_version),
            self.api_version,
            esc(&self.entry.runtime),
            esc(&self.entry.library),
            self.capabilities.network,
            self.capabilities.audio_capture,
            self.capabilities.audio_output,
            str_list(&self.capabilities.filesystem_read),
            str_list(&self.capabilities.filesystem_write),
            esc(&self.sandbox.profile),
            str_list(&self.dependencies.native),
            str_list(&self.dependencies.wasm),
            self.headless_render.default_fps,
            self.headless_render.deterministic_seed,
            self.headless_render.gpu_optional,
        )
    }
}
