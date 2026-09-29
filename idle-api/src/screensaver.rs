use std::time::Duration;

use crate::terminal_cell::TerminalCell;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuSpotlight {
    pub origin_x_ratio: f32,
    pub color_r: f32,
    pub color_g: f32,
    pub color_b: f32,
    pub angle: f32,
    pub spread: f32,
    pub speed: f32,
    pub _pad: f32,
}

pub trait Screensaver: ScreensaverState {
    fn init(&mut self, _cols: usize, _rows: usize) {}
    fn update(&mut self, dt: Duration, cols: usize, rows: usize);
    fn update_frame_time(&mut self, _dt: Duration) {}
    fn draw(&self, grid: &mut [TerminalCell], cols: usize, rows: usize);
    fn has_scanlines(&self) -> bool {
        false
    }
    fn spotlights(&self) -> &[GpuSpotlight] {
        &[]
    }
}

/// FFI-safe wrapper around the Screensaver trait object.
pub struct ScreensaverInstance {
    pub inner: Box<dyn Screensaver>,
}

pub trait ScreensaverState {
    fn active(&self) -> bool;
    fn set_active(&mut self, active: bool);
    fn focused(&self) -> bool;
    fn set_focused(&mut self, focused: bool);
}

impl<T: Screensaver + ?Sized> ScreensaverState for T {
    fn active(&self) -> bool {
        true
    }
    fn set_active(&mut self, _active: bool) {}
    fn focused(&self) -> bool {
        true
    }
    fn set_focused(&mut self, _focused: bool) {}
}
