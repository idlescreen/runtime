// SPDX-License-Identifier: MIT

//! 5×5 block font glyph tables split by character class for maintainability.
//!
//! Lookup order: A–M, N–Z, digits, then punctuation/symbols. Unknown glyphs fall
//! back to a rounded box in [`symbols`].
//!
//! Patterns use Unicode full block (`U+2588`) glyphs so plugins need no font atlas.

mod alpha_am;
mod alpha_nz;
mod digits;
mod symbols;

/// 5x5 block font patterns (█ = on). Used to render live logo_text + kernel
/// so the centered "text in the middle of the screen" is always the actual OS.
pub fn get_5x5_pattern(ch: char) -> Option<[&'static str; 5]> {
    let u = ch.to_ascii_uppercase();
    alpha_am::pattern(u)
        .or_else(|| alpha_nz::pattern(u))
        .or_else(|| digits::pattern(u))
        .or_else(|| symbols::pattern(u))
}

// Pattern lookup is O(1) per class with no heap allocation.
// Lowercase input is normalized before lookup.
