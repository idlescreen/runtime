//! Blank screensaver example.
//!
//! Renders a blank/black canvas.
//!
//! Build with: `cargo build -p idle-api --example blank_saver`

use idle_api::{Screensaver, TerminalCell};
use std::time::Duration;

struct BlankSaver;

impl Screensaver for BlankSaver {
    fn init(&mut self, _cols: usize, _rows: usize) {}

    fn update(&mut self, _dt: Duration, _cols: usize, _rows: usize) {}

    fn draw(&self, grid: &mut [TerminalCell], _cols: usize, _rows: usize) {
        for cell in grid.iter_mut() {
            cell.bg = (0, 0, 0);
            cell.fg = (0, 0, 0);
            cell.ch = ' ';
            cell.bold = false;
        }
    }

    fn has_scanlines(&self) -> bool {
        false
    }

    fn spotlights(&self) -> &[idle_api::GpuSpotlight] {
        &[]
    }
}

fn main() {
    let _saver = BlankSaver;
    println!("BlankSaver screensaver example compiled and ready.");
}
