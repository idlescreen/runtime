// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Stable C ABI for screensaver plugins written in non-Rust languages.
//!
//! Rust plugins implement [`Screensaver`] and export `create_screensaver`
//! returning a `Box<dyn Screensaver>` — a trait object no other language can
//! produce. This module defines the foreign-language contract instead: the
//! plugin exports one symbol
//!
//! ```c
//! const IdleSaverOps *idle_saver_ops(void);
//! ```
//!
//! and the host drives it through the function-pointer table. `CAbiSaver`
//! adapts that table to the [`Screensaver`] trait so the rest of the host is
//! language-agnostic. The full contract (including the symbol list and the
//! ownership rules) is spelled out in `docs/idle_saver.h` at the repo root.

use std::ffi::c_void;

use crate::{GpuSpotlight, TerminalCell};

/// C-ABI mirror of [`TerminalCell`]. `repr(C)` layout — the C header defines
/// the same struct byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdleCell {
    /// Unicode scalar value (`char32_t`). Invalid values render as space.
    pub ch: u32,
    /// Foreground RGB.
    pub fg: [u8; 3],
    /// Background RGB.
    pub bg: [u8; 3],
    /// 0 = normal, nonzero = bold (double-width glyph).
    pub bold: u8,
}

impl IdleCell {
    /// Convert to the host cell type, replacing an invalid scalar with space.
    pub fn to_terminal_cell(&self) -> TerminalCell {
        TerminalCell {
            ch: char::from_u32(self.ch).unwrap_or(' '),
            fg: (self.fg[0], self.fg[1], self.fg[2]),
            bg: (self.bg[0], self.bg[1], self.bg[2]),
            bold: self.bold != 0,
        }
    }
}

/// Function-pointer table a foreign-language plugin returns from
/// `idle_saver_ops()`. All pointers except `create`/`destroy`/`update`/`draw`
/// are optional (`NULL` = use the trait default).
///
/// The `ctx` produced by `create` is passed back to every callback; the host
/// never dereferences it. `draw`/`spotlights` buffers are host-owned and
/// valid only for the duration of the call — the plugin must not retain them.
#[repr(C)]
pub struct IdleSaverOps {
    /// Value of `API_VERSION` the plugin was built against.
    pub abi_version: u32,
    /// Allocate plugin state; return the opaque context. NULL = failure.
    pub create: unsafe extern "C" fn() -> *mut c_void,
    /// Free plugin state. Called exactly once on unload.
    pub destroy: unsafe extern "C" fn(*mut c_void),
    /// `(ctx, cols, rows)` — called after creation and on resize. Optional.
    pub init: Option<unsafe extern "C" fn(*mut c_void, u32, u32)>,
    /// `(ctx, dt_seconds, cols, rows)` — advance the simulation.
    pub update: unsafe extern "C" fn(*mut c_void, f64, u32, u32),
    /// `(ctx, dt_seconds)` — optional sub-frame timing hook.
    pub update_frame_time: Option<unsafe extern "C" fn(*mut c_void, f64)>,
    /// `(ctx, cells, cols, rows)` — paint the `cols*rows` buffer.
    pub draw: unsafe extern "C" fn(*mut c_void, *mut IdleCell, u32, u32),
    /// `(ctx)` — nonzero enables scanline postfx. Optional.
    pub has_scanlines: Option<unsafe extern "C" fn(*mut c_void) -> u8>,
    /// `(ctx, out, capacity)` — write up to `capacity` spotlights, return the
    /// count written. Optional.
    pub spotlights: Option<unsafe extern "C" fn(*mut c_void, *mut GpuSpotlight, u32) -> u32>,
}

/// Symbol a C-ABI plugin exports instead of `create_screensaver`.
pub const OPS_SYMBOL: &[u8] = b"idle_saver_ops";

mod saver;
pub use saver::CAbiSaver;
