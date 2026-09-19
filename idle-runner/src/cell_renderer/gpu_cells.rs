// SPDX-License-Identifier: MIT

use super::gpu_init::GpuCell;
use idle_api::TerminalCell;
use std::collections::HashMap;

/// Build GPU cell records into a reused scratch buffer (no per-frame alloc when sized).
pub fn build_gpu_cells_into(
    grid: &[TerminalCell],
    grid_cols: usize,
    col_start: usize,
    row_start: usize,
    cols: usize,
    rows: usize,
    atlas_index: &HashMap<char, u32>,
    out: &mut Vec<GpuCell>,
) {
    let needed = cols.saturating_mul(rows);
    out.clear();
    if out.capacity() < needed {
        out.reserve(needed - out.capacity());
    }

    for row in 0..rows {
        for col in 0..cols {
            let index = (row_start + row) * grid_cols + (col_start + col);
            if let Some(cell) = grid.get(index) {
                let bg_color =
                    ((cell.bg.0 as u32) << 16) | ((cell.bg.1 as u32) << 8) | (cell.bg.2 as u32);
                let fg_color =
                    ((cell.fg.0 as u32) << 16) | ((cell.fg.1 as u32) << 8) | (cell.fg.2 as u32);
                let char_idx = if cell.ch == ' ' {
                    0xFFFFFFFF
                } else {
                    atlas_index.get(&cell.ch).copied().unwrap_or(0xFFFFFFFF)
                };
                out.push(GpuCell {
                    bg_color,
                    fg_color,
                    char_idx,
                    bold: u32::from(cell.bold),
                });
            } else {
                out.push(GpuCell {
                    bg_color: 0,
                    fg_color: 0xFFFFFF,
                    char_idx: 0xFFFFFFFF,
                    bold: 0,
                });
            }
        }
    }
}

pub fn copy_staging_to_out(
    staging_buffer: &wgpu::Buffer,
    device: &wgpu::Device,
    content_w: u32,
    content_h: u32,
    unpadded: u32,
    padded: u32,
    out: &mut Vec<u8>,
) {
    let buffer_slice = staging_buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |v| {
        let _ = sender.send(v);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });

    if let Ok(Ok(())) = receiver.recv() {
        let data = buffer_slice.get_mapped_range();
        let byte_len = (content_w * content_h * 4) as usize;
        out.resize(byte_len, 0);
        for row in 0..content_h {
            let src_start = (row * padded) as usize;
            let src_end = src_start + unpadded as usize;
            let dst_start = (row * unpadded) as usize;
            let dst_end = dst_start + unpadded as usize;
            if src_end <= data.len() && dst_end <= out.len() {
                out[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
            }
        }
        drop(data);
        staging_buffer.unmap();
    } else {
        idle_log::error!("Failed to map staging buffer for wgpu cell renderer");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idle_api::TerminalCell;

    #[test]
    fn build_gpu_cells_reuses_capacity() {
        let grid = [
            TerminalCell {
                ch: 'A',
                fg: (255, 0, 0),
                bg: (0, 0, 0),
                bold: false,
            },
            TerminalCell {
                ch: ' ',
                fg: (0, 0, 0),
                bg: (0, 0, 0),
                bold: false,
            },
        ];
        let mut index = HashMap::new();
        index.insert('A', 1u32);
        let mut buf = Vec::with_capacity(8);
        build_gpu_cells_into(&grid, 2, 0, 0, 2, 1, &index, &mut buf);
        assert_eq!(buf.len(), 2);
        assert_eq!(buf[0].char_idx, 1);
        assert_eq!(buf[1].char_idx, 0xFFFFFFFF);
        let cap = buf.capacity();
        build_gpu_cells_into(&grid, 2, 0, 0, 2, 1, &index, &mut buf);
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.capacity(), cap);
    }
}
