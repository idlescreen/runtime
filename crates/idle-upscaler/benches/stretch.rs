// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Bench harness for `idle-upscaler`. Backs the T1 and T2 pages under
//! `src/cpu/`.
//!
//! Two kinds of measurement live here. **Whole path**:
//! [`FrameUpscaler::upscale_stretch_into`] and
//! [`FrameUpscaler::upscale_letterbox_into`] over the full source ×
//! destination grid matrix the runtime ships. **Per page**: the inner
//! loops those two call, measured directly so a regression is
//! attributable to one page rather than to "the upscaler".
//!
//! Inputs are wrapped in `std::hint::black_box` so the compiler can't
//! constant-fold the source bytes or hoist the `out` allocation.
//!
//! ```bash
//! cargo bench --bench stretch
//! cargo bench --bench stretch -- "bilinear_avx2"
//! ```

use std::hint::black_box as bb;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

#[cfg(target_arch = "x86_64")]
use idle_upscaler::bench_exports::bilinear_row_avx2;
use idle_upscaler::bench_exports::{
    StretchCache, bilinear_row, stretch_byte_rows, stretch_u32_rows,
};
use idle_upscaler::{FilterMode, FrameUpscaler};

/// Common source (simulation grid) sizes — what the savers actually produce.
const SRC_GRIDS: &[(u32, u32)] = &[(320, 180), (640, 360), (960, 540), (1280, 720)];

/// Common destination (Wayland output) sizes.
const DST_GRIDS: &[(u32, u32)] = &[(1920, 1080), (2560, 1440), (3840, 2160)];

/// The one upscale every per-page bench uses: the 320×180 saver grid the
/// runtime ships, scaled to a 1920×1080 output. A single pair keeps the
/// per-page numbers comparable to each other and to the whole-path runs.
const PAIR: (u32, u32, u32, u32) = (320, 180, 1920, 1080);

/// Both whole-path entry points share this signature, so one driver
/// covers Stretch and Letterbox and they cannot drift apart.
type UpscaleFn = fn(&mut FrameUpscaler, &[u8], u32, u32, u32, u32, &mut Vec<u8>);

/// Deterministic-but-non-uniform source BGRA: a per-pixel gradient, so
/// the upscale loop has real work (constant zero folds away even
/// through `black_box`) and repeat runs are comparable.
fn make_src(w: u32, h: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    let mut i = 0;
    for y in 0..h {
        for x in 0..w {
            // BGRA
            buf[i] = ((x * 255 / w.max(1)) & 0xFF) as u8; // B
            buf[i + 1] = ((y * 255 / h.max(1)) & 0xFF) as u8; // G
            buf[i + 2] = (((x.wrapping_add(y)) & 0xFF) ^ 0xA5) as u8; // R
            buf[i + 3] = 0xFF; // A
            i += 4;
        }
    }
    buf
}

/// [`make_src`] reinterpreted as one `u32` per pixel, so
/// `stretch_u32_rows` is measured on the aligned path it ships on.
fn make_src_u32(w: u32, h: u32) -> Vec<u32> {
    let src = make_src(w, h);
    src.as_chunks::<4>()
        .0
        .iter()
        .map(|p| u32::from_ne_bytes(*p))
        .collect()
}

/// Shared driver for both whole-path benches. `group` doubles as the
/// criterion group name, which is the `bench:` target the
/// `upscale_stretch_into` and `letterbox` labels name.
fn bench_path(c: &mut Criterion, group: &str) {
    let op: UpscaleFn = if group == "stretch" {
        FrameUpscaler::upscale_stretch_into
    } else {
        FrameUpscaler::upscale_letterbox_into
    };
    let mut g = c.benchmark_group(group);
    g.measurement_time(Duration::from_secs(3));
    g.warm_up_time(Duration::from_secs(1));

    for &(src_w, src_h) in SRC_GRIDS {
        for &(dst_w, dst_h) in DST_GRIDS {
            // Skip source > destination — that is a downscale, which the
            // runtime does not ship.
            if src_w > dst_w || src_h > dst_h {
                continue;
            }
            let src = make_src(src_w, src_h);
            let bytes_out = u64::from(dst_w) * u64::from(dst_h) * 4;
            g.throughput(Throughput::Bytes(bytes_out));
            for &filter in &[FilterMode::Nearest, FilterMode::Linear] {
                let f = match filter {
                    FilterMode::Nearest => "nearest",
                    FilterMode::Linear => "linear",
                };
                let label = format!("{f}_{src_w}x{src_h}_to_{dst_w}x{dst_h}");
                let mut upscaler = FrameUpscaler::new(filter);
                let mut out: Vec<u8> = Vec::with_capacity(bytes_out as usize);
                g.bench_with_input(
                    BenchmarkId::from_parameter(&label),
                    &(&src, src_w, src_h, dst_w, dst_h),
                    |b, &(src, sw, sh, dw, dh)| {
                        b.iter(|| {
                            op(&mut upscaler, bb(src), sw, sh, dw, dh, bb(&mut out));
                            bb(out.as_mut_ptr());
                            bb(out.len());
                        });
                    },
                );
            }
        }
    }
    g.finish();
}

/// T1 · `upscale_stretch_into` — full-screen stretch, aspect distorted.
fn bench_stretch(c: &mut Criterion) {
    bench_path(c, "stretch");
}

/// T1 · `letterbox` — aspect preserved, black bars top and bottom.
fn bench_letterbox(c: &mut Criterion) {
    bench_path(c, "letterbox");
}

/// T1 · `stretch_u32_rows` — the aligned row fill: where an upscale
/// frame spends its time.
fn bench_stretch_u32_rows(c: &mut Criterion) {
    let (sw, sh, dw, dh) = PAIR;
    let src = make_src_u32(sw, sh);
    let mut dst = vec![0u32; (dw as usize) * (dh as usize)];
    let mut cache = StretchCache::new();
    cache.ensure(sw, dw); // caller-side, as `upscale_stretch_into` does
    let mut g = c.benchmark_group("stretch_u32_rows");
    g.throughput(Throughput::Elements(u64::from(dw) * u64::from(dh)));
    g.bench_function("320x180_to_1920x1080", |b| {
        b.iter(|| {
            let (sw, sh, dw, dh) = bb((sw, sh, dw, dh));
            stretch_u32_rows(bb(&src), bb(&mut dst), sw, sh, dw, dh, bb(&cache));
        });
    });
    g.finish();
}

/// T1 · `stretch_byte_rows` — the unaligned fallback the u32 path skips.
fn bench_stretch_byte_rows(c: &mut Criterion) {
    let (sw, sh, dw, dh) = PAIR;
    let src = make_src(sw, sh);
    let needed = (dw as usize) * (dh as usize) * 4;
    let mut dst = vec![0u8; needed];
    let mut cache = StretchCache::new();
    cache.ensure(sw, dw);
    let mut g = c.benchmark_group("stretch_byte_rows");
    g.throughput(Throughput::Bytes(needed as u64));
    g.bench_function("320x180_to_1920x1080", |b| {
        b.iter(|| {
            let (sw, sh, dw, dh) = bb((sw, sh, dw, dh));
            stretch_byte_rows(bb(&mut dst), bb(&src), sw, sh, dw, dh, needed, bb(&cache));
        });
    });
    g.finish();
}

/// T2 · `stretch_cache` — the column map is rebuilt only when
/// `(src_w, dst_w)` changes, so steady state is the no-op branch.
fn bench_stretch_cache(c: &mut Criterion) {
    let (sw, _, dw, _) = PAIR;
    let mut g = c.benchmark_group("stretch_cache");
    g.bench_function("ensure_cached_noop", |b| {
        let mut cache = StretchCache::new();
        cache.ensure(sw, dw);
        b.iter(|| cache.ensure(bb(sw), bb(dw)));
    });
    g.bench_function("ensure_rebuild", |b| {
        let mut cache = StretchCache::new();
        cache.ensure(sw, dw);
        // Alternate the width so every call misses and rebuilds x_map.
        let mut flip = false;
        b.iter(|| {
            flip = !flip;
            cache.ensure(bb(sw), bb(if flip { dw } else { dw + 1 }));
        });
    });
    g.finish();
}

/// T1 · `bilinear_row` — public scalar entry point, one call per four
/// horizontally-adjacent output pixels.
fn bench_bilinear_row(c: &mut Criterion) {
    let (sw, sh, _, _) = PAIR;
    let src = make_src(sw, sh);
    let mut out = [0u8; 16];
    let mut g = c.benchmark_group("bilinear_row");
    g.throughput(Throughput::Bytes(16));
    g.bench_function("4px_128_fractional", |b| {
        b.iter(|| bilinear_row(bb(&src), sw, sh, 0, 0, 1, 1, 128, 128, bb(&mut out)));
    });
    g.finish();
}

/// T1 · `bilinear_avx2` — the x86_64 body `bilinear_row` delegates to.
/// Guarded: the module is `#![cfg(target_arch = "x86_64")]`, so on any
/// other target this bench is absent rather than silently measuring the
/// scalar path under an AVX2 name.
#[cfg(target_arch = "x86_64")]
fn bench_bilinear_avx2(c: &mut Criterion) {
    let (sw, _, _, _) = PAIR;
    let src = make_src(sw, 2);
    let mut out = [0u8; 16];
    let mut g = c.benchmark_group("bilinear_avx2");
    g.throughput(Throughput::Bytes(16));
    g.bench_function("4px_128_fractional", |b| {
        b.iter(|| {
            // SAFETY: `src` is `sw * 2 * 4` bytes and the (0,0)-(1,1)
            // window at stride `sw * 4` is in bounds.
            unsafe { bilinear_row_avx2(bb(&src), sw, 0, 0, 1, 1, 128, 128, bb(&mut out)) }
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_stretch,
    bench_letterbox,
    bench_stretch_u32_rows,
    bench_stretch_byte_rows,
    bench_stretch_cache,
    bench_bilinear_row,
    bench_bilinear_avx2,
);
criterion_main!(benches);
