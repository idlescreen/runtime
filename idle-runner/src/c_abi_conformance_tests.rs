// SPDX-License-Identifier: MIT

//! C-ABI conformance tests: compile a real C plugin against
//! `idle-api/include/idle_saver.h`, load it through `resolve_entry`, and
//! drive it via the Screensaver trait — proving a non-Rust `.so` satisfies
//! the plugin contract the host enforces.

use super::plugin_session::PluginGuard;
use super::plugin_session::entry::resolve_entry;
use crate::dylib::Library;
use std::path::PathBuf;
use std::process::Command;

const FIXTURE: &str = r#"
#include "idle_saver.h"
#include <string.h>

static int live_ctx = 0;

static void *fx_create(void) { live_ctx = 1; return &live_ctx; }
static void fx_destroy(void *ctx) { (void)ctx; live_ctx = 0; }
static void fx_update(void *ctx, double dt, uint32_t c, uint32_t r) {
    (void)ctx; (void)c; (void)r;
    if (dt <= 0.0) __builtin_trap();
}
static void fx_draw(void *ctx, IdleCell *cells, uint32_t cols, uint32_t rows) {
    (void)ctx;
    for (uint32_t i = 0; i < cols * rows; i++) {
        cells[i].ch = 0x2588u; /* █ */
        cells[i].fg[0] = 255; cells[i].fg[1] = 64; cells[i].fg[2] = 0;
        cells[i].bold = 1;
    }
}
static uint8_t fx_scanlines(void *ctx) { (void)ctx; return 1; }
static uint32_t fx_spots(void *ctx, IdleGpuSpotlight *out, uint32_t cap) {
    (void)ctx;
    if (cap < 1 || !out) return 0;
    out[0].origin_x_ratio = 0.5f; out[0].color_r = 1.0f;
    return 1;
}

static const IdleSaverOps OPS = {
    .abi_version = IDLE_API_VERSION,
    .create = fx_create,
    .destroy = fx_destroy,
    .init = NULL,
    .update = fx_update,
    .update_frame_time = NULL,
    .draw = fx_draw,
    .has_scanlines = fx_scanlines,
    .spotlights = fx_spots,
};

uint32_t idle_api_version(void) { return IDLE_API_VERSION; }
const IdleSaverOps *idle_saver_ops(void) { return &OPS; }
"#;

/// Compile `src` to a shared library; returns the .so path, or None when no
/// C toolchain is available (test skips rather than fails the suite).
fn compile_fixture(dir: &crate::test_util::TmpDir, name: &str, src: &str) -> Option<PathBuf> {
    let header_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../idle-api/include");
    let src_path = dir.path().join(format!("{name}.c"));
    let so_path = dir.path().join(format!("lib{name}.so"));
    std::fs::write(&src_path, src).unwrap();

    let status = Command::new("cc")
        .args([
            "-shared",
            "-fPIC",
            "-Wall",
            "-Werror",
            "-I",
            header_dir.to_str().unwrap(),
            src_path.to_str().unwrap(),
            "-o",
            so_path.to_str().unwrap(),
        ])
        .status()
        .ok()?;
    if status.success() && so_path.exists() {
        Some(so_path)
    } else {
        None
    }
}

#[test]
fn c_plugin_loads_and_drives_ops_table() {
    let dir = crate::test_util::tempdir().unwrap();
    let Some(so) = compile_fixture(&dir, "fxsaver", FIXTURE) else {
        eprintln!("no C toolchain — skipping");
        return;
    };

    unsafe {
        let lib = Library::new(&so).unwrap();
        let (ptr, destroy) = resolve_entry(&lib).expect("resolve_entry failed");
        let mut guard = PluginGuard {
            ptr,
            destroy,
            _lib: lib,
        };

        let saver = guard.saver_mut();
        saver.init(4, 2);
        saver.update(std::time::Duration::from_millis(16), 4, 2);
        assert!(saver.has_scanlines());

        let mut grid = vec![idle_api::TerminalCell::default(); 8];
        saver.draw(&mut grid, 4, 2);
        assert!(
            grid.iter()
                .all(|c| c.ch == '█' && c.fg == (255, 64, 0) && c.bold),
            "cells not painted by C plugin"
        );

        let spots = saver.spotlights();
        assert_eq!(spots.len(), 1);
        assert!((spots[0].color_r - 1.0).abs() < f32::EPSILON);
    } // guard drop → ops.destroy → live_ctx = 0 in the plugin
}

#[test]
fn c_plugin_wrong_abi_version_rejected() {
    const BAD: &str = r#"
#include "idle_saver.h"
static void *c(void) { return (void*)1; }
static void d(void *x) { (void)x; }
static void u(void *x, double dt, uint32_t a, uint32_t b) {(void)x;(void)dt;(void)a;(void)b;}
static void w(void *x, IdleCell *g, uint32_t a, uint32_t b) {(void)x;(void)g;(void)a;(void)b;}
static const IdleSaverOps OPS = {
    .abi_version = 999u, .create = c, .destroy = d, .update = u, .draw = w,
};
uint32_t idle_api_version(void) { return IDLE_API_VERSION; }
const IdleSaverOps *idle_saver_ops(void) { return &OPS; }
"#;
    let dir = crate::test_util::tempdir().unwrap();
    let Some(so) = compile_fixture(&dir, "fxbad", BAD) else {
        eprintln!("no C toolchain — skipping");
        return;
    };
    unsafe {
        let lib = Library::new(&so).unwrap();
        let result = resolve_entry(&lib);
        assert!(
            matches!(
                result,
                Err(crate::launcher::PluginError::ApiVersionMismatch { found: 999, .. })
            ),
            "expected ApiVersionMismatch(999), got {result:?}"
        );
    }
}
