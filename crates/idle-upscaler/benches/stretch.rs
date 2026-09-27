// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Bench harness for `idle-upscaler`.
//!
//! Parametrized over the realistic source/destination grid sizes the
//! runtime actually ships:
//!
//! - **Source** (`src_w × src_h`) = the simulation grid (smaller than
//!   the output; the upscale is the whole point of this crate).
//!   Common values: 320×180, 640×360, 960×540, 1280×720.
//! - **Destination** (`dst_w × dst_h`) = the Wayland output. Common
//!   values: 1920×1080 (FHD), 2560×1440 (QHD), 3840×2160 (4K UHD).
//! - **Filter** = Nearest (point sample) vs Linear (bilinear).
//! - **Path** = Stretch (full-screen, distort aspect) vs Letterbox
//!   (preserve aspect, black bars).
//!
//! The harness wraps every input in `std::hint::black_box` so the
//! compiler can't constant-fold the source bytes or hoist the
//! destination `out` allocation; a code-gen quirk that reads all of
//! `src` and claims "0 cycles" is exactly the failure mode the plan
//! called out.
//!
//! Per-frame cycles (cycles/sample) is reported via criterion's
//! default throughput metric. Wall-clock is intentionally avoided:
//! CI runners are noisy and the absolute cycles/sample is what
//! matters for the Tier-1 / Tier-2 / Tier-3 gating deltas.
//!
//! Run with:
//!
//! ```bash
//! cargo bench --bench stretch
//! # or one bench:
//! cargo bench --bench stretch -- "stretch_nearest_640x360_to_1920x1080"
//! ```

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use idle_upscaler::{FilterMode, FrameUpscaler};

/// Common source (simulation grid) sizes — what the savers actually produce.
const SRC_GRIDS: &[(u32, u32)] = &[(320, 180), (640, 360), (960, 540), (1280, 720)];

/// Common destination (Wayland output) sizes.
const DST_GRIDS: &[(u32, u32)] = &[(1920, 1080), (2560, 1440), (3840, 2160)];

/// Build a deterministic-but-non-uniform source BGRA buffer. The pattern
/// is a per-pixel gradient + a few constant-color "objects" so the
/// upscale loop has actual work to do (constant zero would be optimized
/// away even through `black_box`). Determinism keeps bench-to-bench
/// variance low.
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

fn bench_stretch(c: &mut Criterion) {
    let mut group = c.benchmark_group("stretch");
    // 3-second measurement window per sample; bench is small enough that
    // we can afford the precision. Warm-up is criterion's default 3s.
    group.measurement_time(Duration::from_secs(3));
    group.warm_up_time(Duration::from_secs(1));

    for &(src_w, src_h) in SRC_GRIDS {
        for &(dst_w, dst_h) in DST_GRIDS {
            // Skip source > destination — the stretch path becomes a
            // downscale, which the runtime doesn't ship.
            if src_w > dst_w || src_h > dst_h {
                continue;
            }
            let src = make_src(src_w, src_h);
            let bytes_out = (dst_w as u64) * (dst_h as u64) * 4;
            group.throughput(Throughput::Bytes(bytes_out));

            for &filter in &[FilterMode::Nearest, FilterMode::Linear] {
                let label = format!(
                    "{}_{}x{}_to_{}x{}",
                    match filter {
                        FilterMode::Nearest => "nearest",
                        FilterMode::Linear => "linear",
                    },
                    src_w,
                    src_h,
                    dst_w,
                    dst_h
                );
                let mut upscaler = FrameUpscaler::new(filter);
                let mut out: Vec<u8> = Vec::with_capacity(bytes_out as usize);

                group.bench_with_input(
                    BenchmarkId::from_parameter(&label),
                    &(&src, src_w, src_h, dst_w, dst_h),
                    |b, &(src, sw, sh, dw, dh)| {
                        b.iter(|| {
                            upscaler.upscale_stretch_into(
                                black_box(src),
                                black_box(sw),
                                black_box(sh),
                                black_box(dw),
                                black_box(dh),
                                black_box(&mut out),
                            );
                            black_box(out.as_mut_ptr());
                            black_box(out.len());
                        });
                    },
                );
            }
        }
    }
    group.finish();
}

fn bench_letterbox(c: &mut Criterion) {
    let mut group = c.benchmark_group("letterbox");
    group.measurement_time(Duration::from_secs(3));
    group.warm_up_time(Duration::from_secs(1));

    for &(src_w, src_h) in SRC_GRIDS {
        for &(dst_w, dst_h) in DST_GRIDS {
            if src_w > dst_w || src_h > dst_h {
                continue;
            }
            let src = make_src(src_w, src_h);
            let bytes_out = (dst_w as u64) * (dst_h as u64) * 4;
            group.throughput(Throughput::Bytes(bytes_out));

            for &filter in &[FilterMode::Nearest, FilterMode::Linear] {
                let label = format!(
                    "{}_{}x{}_to_{}x{}",
                    match filter {
                        FilterMode::Nearest => "nearest",
                        FilterMode::Linear => "linear",
                    },
                    src_w,
                    src_h,
                    dst_w,
                    dst_h
                );
                let mut upscaler = FrameUpscaler::new(filter);
                let mut out: Vec<u8> = Vec::with_capacity(bytes_out as usize);

                group.bench_with_input(
                    BenchmarkId::from_parameter(&label),
                    &(&src, src_w, src_h, dst_w, dst_h),
                    |b, &(src, sw, sh, dw, dh)| {
                        b.iter(|| {
                            upscaler.upscale_letterbox_into(
                                black_box(src),
                                black_box(sw),
                                black_box(sh),
                                black_box(dw),
                                black_box(dh),
                                black_box(&mut out),
                            );
                            black_box(out.as_mut_ptr());
                            black_box(out.len());
                        });
                    },
                );
            }
        }
    }
    group.finish();
}

criterion_group!(benches, bench_stretch, bench_letterbox);
criterion_main!(benches);
