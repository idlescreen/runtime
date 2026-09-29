//! Single character cell for grid-based terminal renderers.
//!
//! Plugins draw into a flat `[TerminalCell]` slice addressed in row-major order.
//! Foreground and background are RGB tuples; bold doubles the glyph horizontally
//! when rasterized by the host [`idle_runner::cell_renderer`].

/// A single cell in a character-grid renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TerminalCell {
    pub ch: char,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    pub bold: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: (248, 248, 242),
            bg: (0, 0, 0),
            bold: false,
        }
    }
}

// Grid plugins treat unset cells as space with default colors.
