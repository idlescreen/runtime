// SPDX-License-Identifier: MIT

//! `CAbiSaver` — adapts a foreign plugin's [`IdleSaverOps`] vtable to the
//! host [`Screensaver`] trait.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::time::Duration;

use crate::{GpuSpotlight, Screensaver, TerminalCell};

use super::{IdleCell, IdleSaverOps};

/// Adapts an [`IdleSaverOps`] table to the [`Screensaver`] trait.
///
/// Single-threaded contract: the host calls trait methods from one thread,
/// so the staging buffers live in `UnsafeCell`. The `ops` reference must stay
/// valid for the saver's lifetime — the host keeps the library loaded until
/// after `Drop` runs.
pub struct CAbiSaver {
    ctx: *mut c_void,
    ops: &'static IdleSaverOps,
    cell_buf: UnsafeCell<Vec<IdleCell>>,
    spot_buf: UnsafeCell<Vec<GpuSpotlight>>,
}

/// Upper bound on returned spotlights — matches what any current saver emits.
const SPOTLIGHT_CAP: u32 = 64;

impl CAbiSaver {
    /// Call `ops.create()` and wrap the context. Returns `None` if the plugin
    /// reports allocation failure.
    ///
    /// # Safety
    /// `ops` must point at a valid table that outlives the returned saver
    /// (i.e. the backing library stays loaded).
    pub unsafe fn new(ops: &'static IdleSaverOps) -> Option<Self> {
        let ctx = unsafe { (ops.create)() };
        if ctx.is_null() {
            return None;
        }
        Some(Self {
            ctx,
            ops,
            cell_buf: UnsafeCell::new(Vec::new()),
            spot_buf: UnsafeCell::new(Vec::new()),
        })
    }
}

impl Drop for CAbiSaver {
    fn drop(&mut self) {
        unsafe { (self.ops.destroy)(self.ctx) }
    }
}

// The host drives plugin calls from a single thread; the raw ctx pointer is
// only ever used under that contract.
unsafe impl Send for CAbiSaver {}

impl Screensaver for CAbiSaver {
    fn init(&mut self, cols: usize, rows: usize) {
        if let Some(f) = self.ops.init {
            unsafe { f(self.ctx, cols as u32, rows as u32) }
        }
    }

    fn update(&mut self, dt: Duration, cols: usize, rows: usize) {
        unsafe { (self.ops.update)(self.ctx, dt.as_secs_f64(), cols as u32, rows as u32) }
    }

    fn update_frame_time(&mut self, dt: Duration) {
        if let Some(f) = self.ops.update_frame_time {
            unsafe { f(self.ctx, dt.as_secs_f64()) }
        }
    }

    fn draw(&self, grid: &mut [TerminalCell], cols: usize, rows: usize) {
        let n = cols.saturating_mul(rows).min(grid.len());
        unsafe {
            let buf = &mut *self.cell_buf.get();
            buf.clear();
            buf.resize(n, IdleCell::default());
            (self.ops.draw)(self.ctx, buf.as_mut_ptr(), cols as u32, rows as u32);
            for (dst, src) in grid.iter_mut().zip(buf.iter()) {
                *dst = src.to_terminal_cell();
            }
        }
    }

    fn has_scanlines(&self) -> bool {
        match self.ops.has_scanlines {
            Some(f) => unsafe { f(self.ctx) != 0 },
            None => false,
        }
    }

    fn spotlights(&self) -> &[GpuSpotlight] {
        let Some(f) = self.ops.spotlights else {
            return &[];
        };
        unsafe {
            let buf = &mut *self.spot_buf.get();
            buf.clear();
            buf.resize(SPOTLIGHT_CAP as usize, GpuSpotlight::default());
            let n = f(self.ctx, buf.as_mut_ptr(), SPOTLIGHT_CAP).min(SPOTLIGHT_CAP) as usize;
            &buf[..n]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_cell_to_terminal_cell_roundtrip() {
        let c = IdleCell {
            ch: '🔥' as u32,
            fg: [255, 100, 0],
            bg: [1, 2, 3],
            bold: 1,
        };
        let t = c.to_terminal_cell();
        assert_eq!(t.ch, '🔥');
        assert_eq!(t.fg, (255, 100, 0));
        assert!(t.bold);
    }

    #[test]
    fn invalid_scalar_becomes_space() {
        let c = IdleCell {
            ch: 0xD800, // surrogate — not a scalar
            ..IdleCell::default()
        };
        assert_eq!(c.to_terminal_cell().ch, ' ');
    }

    // A minimal fake plugin exercising the ops path end-to-end.
    static CTX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    unsafe extern "C" fn fake_create() -> *mut c_void {
        CTX.store(0xCAFE, std::sync::atomic::Ordering::Relaxed);
        0xCAFEusize as *mut c_void
    }
    unsafe extern "C" fn fake_destroy(ctx: *mut c_void) {
        assert_eq!(ctx as usize, 0xCAFE);
        CTX.store(0, std::sync::atomic::Ordering::Relaxed);
    }
    unsafe extern "C" fn fake_update(_c: *mut c_void, dt: f64, _w: u32, _h: u32) {
        assert!(dt > 0.0);
    }
    unsafe extern "C" fn fake_draw(_c: *mut c_void, cells: *mut IdleCell, w: u32, h: u32) {
        let n = (w * h) as usize;
        for i in 0..n {
            unsafe {
                *cells.add(i) = IdleCell {
                    ch: 'x' as u32,
                    fg: [9, 9, 9],
                    ..IdleCell::default()
                };
            }
        }
    }

    #[test]
    fn c_abi_saver_drives_ops_table() {
        static OPS: IdleSaverOps = IdleSaverOps {
            abi_version: crate::API_VERSION,
            create: fake_create,
            destroy: fake_destroy,
            init: None,
            update: fake_update,
            update_frame_time: None,
            draw: fake_draw,
            has_scanlines: None,
            spotlights: None,
        };
        let mut saver = unsafe { CAbiSaver::new(&OPS) }.expect("create");
        saver.update(Duration::from_millis(16), 4, 2);
        let mut grid = vec![TerminalCell::default(); 8];
        saver.draw(&mut grid, 4, 2);
        assert!(grid.iter().all(|c| c.ch == 'x' && c.fg == (9, 9, 9)));
        drop(saver);
        assert_eq!(CTX.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
}
