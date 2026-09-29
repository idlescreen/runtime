// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn default_profile() -> String {
    "minimal".to_string()
}

impl Default for Sandbox {
    fn default() -> Self {
        Self {
            profile: default_profile(),
        }
    }
}

impl Default for HeadlessRender {
    fn default() -> Self {
        Self {
            default_fps: 60,
            deterministic_seed: true,
            gpu_optional: true,
        }
    }
}

// ---- TOML-value decoding (was serde) ----

pub(crate) fn missing(key: &str) -> String {
    format!("missing field `{key}`")
}

pub(crate) fn invalid(key: &str, want: &str) -> String {
    format!("invalid type for `{key}`: expected {want}")
}

pub(crate) fn req_str(t: &Value, key: &str) -> Result<String, String> {
    match t.get(key) {
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| invalid(key, "string")),
        None => Err(missing(key)),
    }
}

pub(crate) fn req_u32(t: &Value, key: &str) -> Result<u32, String> {
    match t.get(key) {
        Some(v) => v
            .as_int()
            .and_then(|i| u32::try_from(i).ok())
            .ok_or_else(|| invalid(key, "unsigned integer")),
        None => Err(missing(key)),
    }
}

pub(crate) fn opt_str(t: &Value, key: &str, dflt: String) -> Result<String, String> {
    match t.get(key) {
        None => Ok(dflt),
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| invalid(key, "string")),
    }
}

pub(crate) fn opt_bool(t: &Value, key: &str, dflt: bool) -> Result<bool, String> {
    match t.get(key) {
        None => Ok(dflt),
        Some(v) => v.as_bool().ok_or_else(|| invalid(key, "boolean")),
    }
}

pub(crate) fn opt_u32(t: &Value, key: &str, dflt: u32) -> Result<u32, String> {
    match t.get(key) {
        None => Ok(dflt),
        Some(v) => v
            .as_int()
            .and_then(|i| u32::try_from(i).ok())
            .ok_or_else(|| invalid(key, "unsigned integer")),
    }
}

pub(crate) fn opt_str_list(t: &Value, key: &str) -> Result<Vec<String>, String> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(v) => v
            .as_array()
            .ok_or_else(|| invalid(key, "array"))?
            .iter()
            .map(|e| {
                e.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| invalid(key, "array of strings"))
            })
            .collect(),
    }
}
