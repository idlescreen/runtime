// SPDX-License-Identifier: MIT

//! Viewport rasterization: turn the cell grid + content buffer into a frame
//! for the Wayland surface. Hardware-scaled short-circuit lives here so
//! [`PluginSession`] stays focused on plugin lifecycle.

use super::PluginSession;

impl PluginSession {
    pub fn render(
        &mut self,
        cols: usize,
        rows: usize,
        width: u32,
        height: u32,
    ) -> std::sync::Arc<Vec<u8>> {
        let scanlines = self.draw_frame(cols, rows);
        self.raster_viewport_internal(0, 0, cols, rows, cols, rows, width, height, scanlines);
        self.pixel_buf.clone()
    }

    pub fn raster_viewport(
        &mut self,
        col_start: usize,
        row_start: usize,
        cols: usize,
        rows: usize,
        grid_cols: usize,
        grid_rows: usize,
        width: u32,
        height: u32,
        scanlines: bool,
    ) -> std::sync::Arc<Vec<u8>> {
        self.raster_viewport_internal(
            col_start, row_start, cols, rows, grid_cols, grid_rows, width, height, scanlines,
        );
        self.pixel_buf.clone()
    }

    fn raster_viewport_internal(
        &mut self,
        col_start: usize,
        row_start: usize,
        cols: usize,
        rows: usize,
        grid_cols: usize,
        _grid_rows: usize,
        width: u32,
        height: u32,
        scanlines: bool,
    ) {
        let hardware_scaling = self.hardware_scaling && !self.using_gpu_upscale();
        let out_pixel_buf = std::sync::Arc::make_mut(&mut self.pixel_buf);
        if hardware_scaling {
            self.renderer.render_content_viewport_into(
                &self.grid,
                grid_cols,
                col_start,
                row_start,
                cols,
                rows,
                scanlines,
                out_pixel_buf,
            );
            return;
        }

        let content_w = self.renderer.content_width(cols);
        let content_h = self.renderer.content_height(rows);
        self.renderer.render_content_viewport_into(
            &self.grid,
            grid_cols,
            col_start,
            row_start,
            cols,
            rows,
            scanlines,
            &mut self.content_buf,
        );

        self.upscaler.upscale_stretch_into(
            &self.content_buf,
            content_w,
            content_h,
            width,
            height,
            out_pixel_buf,
        );
    }
}
