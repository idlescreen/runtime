// SPDX-License-Identifier: MIT

use idle_api::TerminalCell;
use idle_runner::cell_renderer::CellRenderer;
use idle_upscaler::FrameUpscaler;

/// Raster a grid viewport into `pixel_buf`, optionally via content upscale.
#[allow(clippy::too_many_arguments)]
pub(crate) fn raster_viewport_into(
    renderer: &mut CellRenderer,
    upscaler: &mut FrameUpscaler,
    grid: &[TerminalCell],
    hardware_scaling: bool,
    using_gpu_upscale: bool,
    content_buf: &mut Vec<u8>,
    pixel_buf: &mut Vec<u8>,
    col_start: usize,
    row_start: usize,
    cols: usize,
    rows: usize,
    grid_cols: usize,
    width: u32,
    height: u32,
    scanlines: bool,
) {
    if hardware_scaling && !using_gpu_upscale {
        renderer.render_content_viewport_into(
            grid, grid_cols, col_start, row_start, cols, rows, scanlines, pixel_buf,
        );
        return;
    }

    let content_w = renderer.content_width(cols);
    let content_h = renderer.content_height(rows);
    renderer.render_content_viewport_into(
        grid,
        grid_cols,
        col_start,
        row_start,
        cols,
        rows,
        scanlines,
        content_buf,
    );

    upscaler.upscale_stretch_into(content_buf, content_w, content_h, width, height, pixel_buf);
}
