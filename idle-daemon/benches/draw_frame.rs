// SPDX-License-Identifier: MIT

//! Bench harness for the runtime daemon's per-frame cell-raster path.
//!
//! Targets [`idle_runner::cell_renderer::CellRenderer::render_content_viewport_into`]
//! — the per-cell BGRA fill + glyph blit that runs every frame on the
//! screensaver's CPU render path. This is the function whose output then
//! feeds [`idle_upscaler::FrameUpscaler::upscale_stretch_into`] (covered
//! by `idle-upscaler/benches/stretch.rs`).
//!
//! Parametrized over realistic terminal grid densities:
//!
//! - **Grid** = `cols × rows` cells (the saver simulation grid).
//!   Common values: 80×24 (VT100 default), 160×48, 240×68 (large font
//!   scaled up), 320×80 (very dense).
//! - **Density** = fraction of cells that contain a non-space glyph
//!   (the saver usually fills ~60-80% of the grid).
//!
//! The harness wraps every input in `std::hint::black_box` so the
//! compiler can't constant-fold a known-empty grid and report "0
//! cycles". See plan §9 ("bench harness correctness risk").
//!
//! The font is loaded by `idle_runner::CellRenderer::new()` from a
//! system-installed monospace (`fonts-dejavu-core` or
//! `fonts-liberation-mono` on the CI runner). If no font is found,
//! the bench prints a one-line skip notice and exits 0 — bench harness
//! failure should never block CI.
//!
//! Run with:
//!
//! ```bash
//! cargo bench --bench draw_frame
//! # one bench:
//!
//! cargo bench --bench draw_frame -- "240x68_density_0.7"
//! ```

use std::hint::black_box;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use idle_api::TerminalCell;
use idle_daemon::bench_exports::{WaitOutcome, apply_fade_in, consume_events, new_bench_handle};
use idle_runner::cell_renderer::CellRenderer;

/// Realistic terminal grid sizes — `(cols, rows)` — for the screen
/// saver's per-frame render. CellRenderer content resolution is
/// `cell_width × cols` × `cell_height × rows` BGRA pixels.
const GRIDS: &[(usize, usize)] = &[(80, 24), (160, 48), (240, 68), (320, 80)];

/// Fraction of grid cells that contain a non-space glyph.
const DENSITIES: &[f32] = &[0.3, 0.7, 0.95];

/// Build a deterministic `TerminalCell` grid with a tunable non-space
/// density. We avoid the `ch = ' '` fast path so the bench exercises
/// the real `blit_bitmap` work.
fn make_grid(cols: usize, rows: usize, density: f32) -> Vec<TerminalCell> {
    let total = cols * rows;
    let mut grid = Vec::with_capacity(total);
    // Deterministic fill: every cell with `(row * 31 + col * 17) % 100
    // < density * 100` is a glyph; the rest is space. Density 0.3 → 30%
    // of cells are glyphs, etc.
    for row in 0..rows {
        for col in 0..cols {
            let v = ((row * 31 + col * 17) % 100) as f32;
            let is_glyph = v < density * 100.0;
            grid.push(TerminalCell {
                ch: if is_glyph {
                    (b'A' + (col % 26) as u8) as char
                } else {
                    ' '
                },
                fg: (200, 200, 200),
                bg: (16, 16, 24),
                bold: col % 7 == 0,
            });
        }
    }
    grid
}

fn bench_render(c: &mut Criterion) {
    let mut renderer = match CellRenderer::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "draw_frame bench: skipping — CellRenderer::new() failed: {e}. \
                 Install fonts-dejavu-core or fonts-liberation-mono on the CI runner."
            );
            return;
        }
    };

    let mut group = c.benchmark_group("render_content_viewport");
    group.measurement_time(std::time::Duration::from_secs(3));
    group.warm_up_time(std::time::Duration::from_secs(1));

    for &(cols, rows) in GRIDS {
        for &density in DENSITIES {
            let grid = make_grid(cols, rows, density);
            let label = format!("{cols}x{rows}_density_{density:.2}");
            // Bytes written: content resolution in BGRA pixels.
            // CellRenderer picks cell dims from the rasterized 'M' glyph,
            // so we let `content_width(cols) * content_height(rows) * 4`
            // be the throughput proxy.
            let cw = renderer.content_width(cols) as u64;
            let ch = renderer.content_height(rows) as u64;
            group.throughput(Throughput::Bytes(cw * ch * 4));

            // Pre-allocate the output buffer once. `render_content_viewport_into`
            // calls `out.resize(byte_len, 0)` and fills every byte, so the
            // bench steady-state is "no allocation". Allocating fresh per
            // iteration would conflate allocator perf with render perf.
            let out_capacity = (cw * ch * 4) as usize;
            let mut out: Vec<u8> = Vec::with_capacity(out_capacity);

            group.bench_with_input(
                BenchmarkId::from_parameter(&label),
                &(&grid, cols, rows),
                |b, &(grid, cols, rows)| {
                    b.iter(|| {
                        renderer.render_content_viewport_into(
                            black_box(grid),
                            black_box(cols),
                            black_box(0),
                            black_box(0),
                            black_box(cols),
                            black_box(rows),
                            black_box(false),
                            black_box(&mut out),
                        );
                        black_box(out.as_mut_ptr());
                        black_box(out.len());
                    });
                },
            );
        }
    }
    group.finish();
}

/// T1 · `apply_fade_in` — the per-frame fade composite. Runs over the
/// whole BGRA buffer once per frame for the first 500ms of a transition,
/// so this is a real hot-path cost, not a one-off.
fn bench_apply_fade_in(c: &mut Criterion) {
    // Full 1920×1080 BGRA frame — the size the overlay actually pushes.
    let len = 1920 * 1080 * 4;
    let src: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    let mut pixels = src.clone();

    let mut g = c.benchmark_group("apply_fade_in");
    g.throughput(Throughput::Bytes(len as u64));
    for (label, ms) in [
        ("mult_0_clears", 0u64),
        ("mult_128", 250),
        ("mult_255", 498),
    ] {
        g.bench_function(label, |b| {
            b.iter(|| {
                pixels.copy_from_slice(&src);
                apply_fade_in(black_box(&mut pixels), Duration::from_millis(ms));
                black_box(pixels.as_mut_ptr());
            });
        });
    }
    // Past the fade window the function early-returns; cheap, but it
    // runs on every frame of a long screensaver, so it stays measured.
    g.bench_function("past_500ms_noop", |b| {
        b.iter(|| apply_fade_in(black_box(&mut pixels), Duration::from_secs(30)));
    });
    g.finish();
}

/// T2 · `consume_events` — inotify buffer parse. Allocation- and
/// branch-sensitive: the runtime calls it on every power-supply event.
fn bench_consume_events(c: &mut Criterion) {
    // 64 whole inotify records, each with a short name — one read().
    let mut buf = Vec::new();
    for i in 0..64u32 {
        let name = format!("ACAD-{:02}", i % 100);
        buf.extend_from_slice(&i.to_ne_bytes());
        buf.extend_from_slice(&0x0000_0102u32.to_ne_bytes()); // IN_MODIFY | IN_CREATE
        buf.extend_from_slice(&0u32.to_ne_bytes());
        buf.extend_from_slice(&name.len().to_ne_bytes());
        buf.extend_from_slice(name.as_bytes());
    }

    let mut g = c.benchmark_group("consume_events");
    g.throughput(Throughput::Bytes(buf.len() as u64));
    g.bench_function("64_records", |b| {
        b.iter(|| black_box(consume_events(black_box(&buf))));
    });
    g.finish();
}

/// T2 · `power_watcher` — the consumer side of the power watcher. This
/// is the lock/condvar path: one `parking_lot` lock per call plus the
/// timed wait. A 1ms cap keeps the bench quick; the real runtime uses a
/// 1s heartbeat, and the cost being measured is the lock/condvar
/// turnover, not the sleep.
fn bench_power_watcher(c: &mut Criterion) {
    let handle = new_bench_handle();
    let stop = AtomicBool::new(false);

    let mut g = c.benchmark_group("power_watcher");
    g.bench_function("wait_for_heartbeat_1ms_timeout", |b| {
        b.iter(|| {
            let out = handle.wait_for_heartbeat(&stop, Duration::from_millis(1));
            black_box(matches!(out, WaitOutcome::Heartbeat));
        });
    });
    g.bench_function("cached_is_on_battery_atomic_load", |b| {
        b.iter(|| black_box(handle.cached_is_on_battery()));
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_render,
    bench_apply_fade_in,
    bench_consume_events,
    bench_power_watcher,
);
criterion_main!(benches);
