// SPDX-License-Identifier: MIT

use super::CellRenderer;
use super::pixels::letterbox_into;
use idle_api::TerminalCell;

impl CellRenderer {
    pub fn cell_width(&self) -> usize {
        self.cell_width
    }

    pub fn cell_height(&self) -> usize {
        self.cell_height
    }

    pub fn grid_for_pixels(&self, width: u32, height: u32) -> (usize, usize) {
        self.grid_for_pixels_scaled(width, height, 1.0)
    }

    pub fn grid_for_pixels_scaled(&self, width: u32, height: u32, scale: f32) -> (usize, usize) {
        let cols = (width as usize / self.cell_width).max(1);
        let rows = (height as usize / self.cell_height).max(1);
        let scale = scale.clamp(0.25, 1.0);
        (
            ((cols as f32 * scale).floor() as usize).max(1),
            ((rows as f32 * scale).floor() as usize).max(1),
        )
    }

    pub fn content_width(&self, cols: usize) -> u32 {
        cols.saturating_mul(self.cell_width).min(u32::MAX as usize) as u32
    }

    pub fn content_height(&self, rows: usize) -> u32 {
        rows.saturating_mul(self.cell_height).min(u32::MAX as usize) as u32
    }

    pub fn render(
        &mut self,
        grid: &[TerminalCell],
        cols: usize,
        rows: usize,
        width: u32,
        height: u32,
        scanlines: bool,
    ) -> Vec<u8> {
        let content_w = self.content_width(cols);
        let content_h = self.content_height(rows);
        let mut content = Vec::new();
        self.render_content_viewport_into(grid, cols, 0, 0, cols, rows, scanlines, &mut content);
        let offset_x = width.saturating_sub(content_w) as usize / 2;
        let offset_y = height.saturating_sub(content_h) as usize / 2;
        let needed = (width as usize)
            .checked_mul(height as usize)
            .and_then(|p| p.checked_mul(4))
            .unwrap_or(0);
        let mut framed = vec![0u8; needed];
        letterbox_into(
            &content,
            content_w,
            content_h,
            width,
            height,
            offset_x,
            offset_y,
            &mut framed,
        );
        framed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_renderer_new_and_render() {
        if !crate::cell_renderer::font_available() {
            return;
        }
        let mut r = CellRenderer::new().unwrap();
        assert!(r.cell_width() > 0);
        assert!(r.cell_height() > 0);

        let grid = vec![
            TerminalCell {
                ch: 'A',
                fg: (255, 0, 0),
                bg: (0, 0, 255),
                bold: true,
            },
            TerminalCell {
                ch: ' ',
                fg: (255, 255, 255),
                bg: (0, 0, 0),
                bold: false,
            },
        ];

        let mut out = Vec::new();
        r.render_content_viewport_into(&grid, 2, 0, 0, 2, 1, false, &mut out);

        let expected_len = r.cell_width() * 2 * r.cell_height() * 4;
        assert_eq!(out.len(), expected_len);
    }
}
