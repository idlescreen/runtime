// SPDX-License-Identifier: MIT

// Frame-pipeline timing probe: measures the production render path
// (content raster → stretch upscale → submit-side memcpy) at real
// geometry (1920×1080, render_scale 0.5 → ~106×38 cell grid).
// Not a hard gate — prints timings; assert only on absurd regressions.

use std::time::Instant;

use idle_api::TerminalCell;

/// Fill a grid with a plausible busy-saver mix (non-space chars, varied
// fg/bg, some bold) so glyph blits actually run.
fn busy_grid(cols: usize, rows: usize) -> Vec<TerminalCell> {
    let chars = [' ', '*', '+', '#', '@', '.', ':', '~', 'x', 'o'];
    (0..cols * rows)
        .map(|i| {
            let ch = chars[i % chars.len()];
            TerminalCell {
                ch,
                fg: (
                    (i * 7 % 255) as u8,
                    (i * 13 % 255) as u8,
                    (i * 3 % 255) as u8,
                ),
                bg: ((i * 5 % 64) as u8, 0, 0),
                bold: i % 17 == 0,
            }
        })
        .collect()
}

#[test]
fn frame_pipeline_timing() {
    let mut renderer = crate::cell_renderer::CellRenderer::new().expect("renderer");
    let mut upscaler = idle_upscaler::FrameUpscaler::new(idle_upscaler::FilterMode::Nearest);

    // Production geometry: 1920×1080 output, render_scale 0.5.
    let (cols, rows) = renderer.grid_for_pixels_scaled(1920, 1080, 0.5);
    let grid = busy_grid(cols, rows);
    let content_w = renderer.content_width(cols);
    let content_h = renderer.content_height(rows);
    let mut content_buf = Vec::new();
    let mut pixel_buf = Vec::new();

    // Warm-up: glyph cache + atlas + buffer sizing.
    for _ in 0..5 {
        renderer.render_content_viewport_into(
            &grid,
            cols,
            0,
            0,
            cols,
            rows,
            false,
            &mut content_buf,
        );
        upscaler.upscale_stretch_into(
            &content_buf,
            content_w,
            content_h,
            1920,
            1080,
            &mut pixel_buf,
        );
    }

    let n = 120;
    let mut t_raster = Vec::with_capacity(n);
    let mut t_upscale = Vec::with_capacity(n);
    let mut t_copy = Vec::with_capacity(n);
    let mut shm = vec![0u8; 1920 * 1080 * 4];

    for _ in 0..n {
        let t0 = Instant::now();
        renderer.render_content_viewport_into(
            &grid,
            cols,
            0,
            0,
            cols,
            rows,
            false,
            &mut content_buf,
        );
        t_raster.push(t0.elapsed());

        let t1 = Instant::now();
        upscaler.upscale_stretch_into(
            &content_buf,
            content_w,
            content_h,
            1920,
            1080,
            &mut pixel_buf,
        );
        t_upscale.push(t1.elapsed());

        let t2 = Instant::now();
        shm.copy_from_slice(&pixel_buf);
        t_copy.push(t2.elapsed());
    }

    let us = |v: &[std::time::Duration]| {
        v.iter().map(|d| d.as_micros() as f64).sum::<f64>() / v.len() as f64 / 1000.0
    };
    let max = |v: &[std::time::Duration]| {
        v.iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .fold(0.0, f64::max)
    };

    // Correctness gate: stretch output must equal the reference
    // nearest-neighbor gather (sx = dx*src_w/dst_w, sy = dy*src_h/dst_h).
    let mut expected = vec![0u8; 1920 * 1080 * 4];
    for dy in 0..1080usize {
        let sy = dy * content_h as usize / 1080;
        for dx in 0..1920usize {
            let sx = dx * content_w as usize / 1920;
            let s = (sy * content_w as usize + sx) * 4;
            let d = (dy * 1920 + dx) * 4;
            expected[d..d + 4].copy_from_slice(&content_buf[s..s + 4]);
        }
    }
    if pixel_buf != expected {
        let idx = pixel_buf
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap();
        let px = idx / 4;
        eprintln!(
            "first diff at byte {idx} (px {},{}): got {:?} want {:?}",
            px % 1920,
            px / 1920,
            &pixel_buf[idx - (idx % 4)..idx - (idx % 4) + 4],
            &expected[idx - (idx % 4)..idx - (idx % 4) + 4],
        );
        assert_eq!(pixel_buf, expected, "stretch diverged from reference");
    }

    eprintln!("grid {cols}x{rows} content {content_w}x{content_h} -> 1920x1080");
    eprintln!(
        "raster : avg {:.2}ms max {:.2}ms",
        us(&t_raster),
        max(&t_raster)
    );
    eprintln!(
        "upscale: avg {:.2}ms max {:.2}ms",
        us(&t_upscale),
        max(&t_upscale)
    );
    eprintln!(
        "shmcpy : avg {:.2}ms max {:.2}ms",
        us(&t_copy),
        max(&t_copy)
    );
    let total = us(&t_raster) + us(&t_upscale) + us(&t_copy);
    eprintln!("TOTAL  : avg {:.2}ms (budget 16.6ms)", total);

    // Gross-regression gate only: pipeline must fit inside half a frame.
    // Meaningful only in optimized builds — debug codegen inflates timings
    // ~10x and would trip this on every CI run.
    #[cfg(not(debug_assertions))]
    assert!(total < 33.0, "frame pipeline {total:.2}ms exceeds 33ms");
}

fn vm_peak_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmPeak"))
                .map(str::to_string)
        })
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        .unwrap_or(0)
}

#[test]
fn cell_renderer_init_memory() {
    // CPU constructor must never probe wgpu — the ~400MB driver-mapping
    // transient is what made daemon MemoryPeak spike at presentation start.
    let before = vm_peak_kb();
    let cpu = crate::cell_renderer::CellRenderer::new().expect("cpu renderer");
    let after_cpu = vm_peak_kb();
    assert!(
        !cpu.gpu_active(),
        "CellRenderer::new must stay CPU-only (GPU is opt-in)"
    );
    assert!(
        after_cpu.saturating_sub(before) < 64 * 1024,
        "CellRenderer::new spiked VmPeak by {} KB — wgpu probe must not run here",
        after_cpu - before
    );
    eprintln!("CellRenderer::new: VmPeak delta {} KB", after_cpu - before);

    let gpu = crate::cell_renderer::CellRenderer::new_with_gpu().expect("gpu renderer");
    eprintln!(
        "CellRenderer::new_with_gpu: VmPeak {} KB -> {} KB (delta {} KB, gpu={})",
        after_cpu,
        vm_peak_kb(),
        vm_peak_kb().saturating_sub(after_cpu),
        gpu.gpu_active()
    );
}
