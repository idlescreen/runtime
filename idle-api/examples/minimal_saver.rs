//! Minimal Trance screensaver example.
//!
//! Renders a solid color that shifts hue over time.
//!
//! Build with: `cargo build -p idle-api --example minimal_saver`

use idle_api::{LcgRng, Screensaver, TerminalCell, hsl_to_rgb, rgb_to_hsl};
use std::time::Duration;

struct HueShifter {
    hue: f32,
}

impl Screensaver for HueShifter {
    fn init(&mut self, _cols: usize, _rows: usize) {
        self.hue = 0.0;
    }

    fn update(&mut self, dt: Duration, _cols: usize, _rows: usize) {
        let dt_secs = dt.as_secs_f32();
        self.hue = (self.hue + dt_secs * 0.1) % 1.0;
    }

    fn draw(&self, grid: &mut [TerminalCell], _cols: usize, _rows: usize) {
        let (r, g, b) = hsl_to_rgb(self.hue, 1.0, 0.5);
        for cell in grid.iter_mut() {
            cell.bg = (r, g, b);
            cell.fg = (r, g, b);
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
    let _saver = HueShifter { hue: 0.0 };
    let mut rng = LcgRng::new_random();
    let _ = rng.next_u64();
    let _ = rgb_to_hsl(255, 0, 0);
    println!("HueShifter screensaver compiled and ready.");
}
