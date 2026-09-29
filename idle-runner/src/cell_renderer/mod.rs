// SPDX-License-Identifier: MIT

#![allow(clippy::too_many_arguments)]

//! Rasterizes [`idle_api::TerminalCell`] grids into BGRA pixel buffers.

mod atlas;
mod font;
mod geom;
mod gpu_cells;
mod gpu_init;
mod gpu_render;
mod gpu_render_setup;
mod pixels;

use std::collections::HashMap;
use std::sync::Arc;

use fontdue::{Font, Metrics};
use idle_api::TerminalCell;

use pixels::{blit_bitmap, dim_rect, fill_rect};

pub use font::{FONT_CANDIDATES, font_available, resolve_font_path};

const FONT_SIZE: f32 = 16.0;

struct CachedGlyph {
    metrics: Metrics,
    bitmap: Arc<[u8]>,
}

/// Rasterizes [`TerminalCell`] grids into ARGB8888 pixel buffers.
pub struct CellRenderer {
    font: Font,
    pub(crate) cell_width: usize,
    pub(crate) cell_height: usize,
    glyph_cache: HashMap<char, CachedGlyph>,
    pub(crate) atlas_chars: Vec<char>,
    /// O(1) char → atlas slot for the GPU cell upload hot path.
    pub(crate) atlas_index: HashMap<char, u32>,
    pub(crate) atlas_image: Vec<u8>,
    pub(crate) atlas_cols: usize,
    pub(crate) atlas_rows: usize,
    pub(crate) atlas_dirty: bool,
    gpu_renderer: Option<gpu_init::GpuCellRenderer>,
}

impl CellRenderer {
    /// CPU-only rasterizer. The wgpu probe maps ~400MB of driver libraries
    /// transiently at construction, so callers opt in explicitly via
    /// [`Self::new_with_gpu`] only when the raster load justifies it.
    pub fn new() -> Result<Self, String> {
        let font_bytes = font::load_monospace_font()?;
        let font = Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(|error| format!("failed to parse font: {error}"))?;

        let (metrics, _) = font.rasterize('M', FONT_SIZE);
        let cell_width = metrics.width.max(8);
        let cell_height = metrics.height.max(14);

        let mut renderer = Self {
            font,
            cell_width,
            cell_height,
            glyph_cache: HashMap::new(),
            atlas_chars: Vec::new(),
            atlas_index: HashMap::new(),
            atlas_image: Vec::new(),
            atlas_cols: 32,
            atlas_rows: 32,
            atlas_dirty: true,
            gpu_renderer: None,
        };
        renderer.prepopulate_atlas();
        Ok(renderer)
    }

    /// CPU rasterizer + wgpu cell path when probing succeeds. Skipped
    /// entirely under `IDLE_DISABLE_CELL_GPU` (zero probe cost).
    pub fn new_with_gpu() -> Result<Self, String> {
        let mut renderer = Self::new()?;
        if std::env::var_os("IDLE_DISABLE_CELL_GPU").is_some() {
            idle_log::info!("IDLE_DISABLE_CELL_GPU set — CPU cell rasterizer");
            return Ok(renderer);
        }
        renderer.gpu_renderer = match gpu_init::GpuCellRenderer::new() {
            Ok(gpu) => {
                idle_log::info!("wgpu cell renderer initialized successfully");
                Some(gpu)
            }
            Err(error) => {
                idle_log::warn!(
                    "wgpu cell renderer initialization failed, falling back to CPU: {error}"
                );
                None
            }
        };
        Ok(renderer)
    }

    /// Whether the wgpu cell path initialized (false → CPU rasterizer).
    pub fn gpu_active(&self) -> bool {
        self.gpu_renderer.is_some()
    }

    fn glyph_for(&mut self, ch: char) -> (Metrics, Arc<[u8]>) {
        if let Some(glyph) = self.glyph_cache.get(&ch) {
            return (glyph.metrics, Arc::clone(&glyph.bitmap));
        }

        let (metrics, bitmap) = self.font.rasterize(ch, FONT_SIZE);
        let bitmap = Arc::from(bitmap);
        self.glyph_cache.insert(
            ch,
            CachedGlyph {
                metrics,
                bitmap: Arc::clone(&bitmap),
            },
        );
        (metrics, bitmap)
    }

    pub fn render_content_viewport_into(
        &mut self,
        grid: &[TerminalCell],
        grid_cols: usize,
        col_start: usize,
        row_start: usize,
        cols: usize,
        rows: usize,
        scanlines: bool,
        out: &mut Vec<u8>,
    ) {
        if self.gpu_renderer.is_some() {
            for row in 0..rows {
                for col in 0..cols {
                    let grid_row = row_start + row;
                    let grid_col = col_start + col;
                    let index = grid_row * grid_cols + grid_col;
                    if let Some(cell) = grid.get(index)
                        && cell.ch != ' '
                    {
                        self.get_or_insert_atlas_char(cell.ch);
                    }
                }
            }

            self.rebuild_atlas_image();

            let cell_w = self.cell_width;
            let cell_h = self.cell_height;
            let atlas_cols = self.atlas_cols;
            let atlas_rows = self.atlas_rows;
            let atlas_dirty = self.atlas_dirty;

            if let Some(gpu) = self.gpu_renderer.as_mut() {
                gpu.render(
                    grid,
                    grid_cols,
                    col_start,
                    row_start,
                    cols,
                    rows,
                    scanlines,
                    cell_w,
                    cell_h,
                    atlas_cols,
                    atlas_rows,
                    &self.atlas_image,
                    atlas_dirty,
                    &self.atlas_index,
                    out,
                );
                self.reset_atlas_dirty();
                return;
            }
        }

        let content_w = self.content_width(cols);
        let content_h = self.content_height(rows);
        let byte_len = (content_w * content_h * 4) as usize;
        // No pre-clear: every pixel belongs to a cell whose bg fill_rect
        // overwrites it — zeroing 2MB here would be dead work.
        out.resize(byte_len, 0);

        for row in 0..rows {
            for col in 0..cols {
                let grid_row = row_start + row;
                let grid_col = col_start + col;
                let index = grid_row * grid_cols + grid_col;
                let Some(cell) = grid.get(index) else {
                    continue;
                };

                let x0 = col * self.cell_width;
                let y0 = row * self.cell_height;
                fill_rect(
                    out,
                    content_w,
                    content_h,
                    x0,
                    y0,
                    self.cell_width,
                    self.cell_height,
                    cell.bg,
                );

                if cell.ch != ' ' {
                    let (metrics, bitmap) = self.glyph_for(cell.ch);
                    blit_bitmap(
                        out,
                        content_w,
                        content_h,
                        x0,
                        y0.saturating_add(metrics.ymin.max(0) as usize),
                        &bitmap,
                        metrics.width,
                        metrics.height,
                        cell.fg,
                    );
                    if cell.bold {
                        blit_bitmap(
                            out,
                            content_w,
                            content_h,
                            x0 + 1,
                            y0.saturating_add(metrics.ymin.max(0) as usize),
                            &bitmap,
                            metrics.width,
                            metrics.height,
                            cell.fg,
                        );
                    }
                }

                if scanlines && row % 2 == 1 {
                    dim_rect(
                        out,
                        content_w,
                        content_h,
                        x0,
                        y0,
                        self.cell_width,
                        self.cell_height,
                    );
                }
            }
        }
    }
}
