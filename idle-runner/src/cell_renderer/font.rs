// SPDX-License-Identifier: MIT

use std::fs;
use std::path::Path;

pub const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/liberation-mono-fonts/LiberationMono-Regular.ttf",
];

/// Returns the first installed monospace font used for cell rasterization.
pub fn resolve_font_path() -> Option<&'static str> {
    FONT_CANDIDATES
        .iter()
        .find(|path| Path::new(path).is_file())
        .copied()
}

/// Whether a supported monospace font is installed on the system.
pub fn font_available() -> bool {
    resolve_font_path().is_some()
}

pub(crate) fn load_monospace_font() -> Result<Vec<u8>, String> {
    let path = resolve_font_path().ok_or_else(|| {
        "no monospace font found; install the fonts-dejavu-core package".to_string()
    })?;
    fs::read(path).map_err(|error| format!("failed to read {path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_candidates_list_nonempty() {
        assert!(!FONT_CANDIDATES.is_empty());
    }

    #[test]
    fn font_candidates_all_absolute_paths() {
        for path in FONT_CANDIDATES {
            assert!(
                Path::new(path).is_absolute(),
                "candidate {path} is not absolute"
            );
            assert!(
                path.ends_with(".ttf"),
                "candidate {path} is not a .ttf file"
            );
        }
    }

    #[test]
    fn font_available_returns_bool() {
        let _ = font_available();
    }

    #[test]
    fn resolve_font_path_matches_availability() {
        assert_eq!(resolve_font_path().is_some(), font_available());
    }
}
