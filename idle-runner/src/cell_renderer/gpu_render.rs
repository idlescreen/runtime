// SPDX-License-Identifier: MIT

use super::gpu_init::{GpuCell, GpuCellRenderer, Uniforms};

/// Byte view of a `#[repr(C)]` value/slice for `queue.write_buffer`
/// (bytemuck replacement — the structs are plain-Copy PODs).
fn as_u8_slice<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}
fn as_u8_bytes<T>(v: &T) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            std::ptr::from_ref::<T>(v).cast::<u8>(),
            std::mem::size_of::<T>(),
        )
    }
}
use idle_api::TerminalCell;
use std::collections::HashMap;

impl GpuCellRenderer {
    pub fn render(
        &mut self,
        grid: &[TerminalCell],
        grid_cols: usize,
        col_start: usize,
        row_start: usize,
        cols: usize,
        rows: usize,
        scanlines: bool,
        cell_width: usize,
        cell_height: usize,
        atlas_cols: usize,
        atlas_rows: usize,
        atlas_image: &[u8],
        atlas_dirty: bool,
        atlas_index: &HashMap<char, u32>,
        out: &mut Vec<u8>,
    ) {
        let Some(targets) = self.prepare_targets(cols, rows, cell_width, cell_height) else {
            return;
        };

        let cells_size = (cols * rows * std::mem::size_of::<GpuCell>()) as u64;
        let (cells_buf, c_re) = Self::ensure_buffer(
            &self.device,
            &mut self.cells_buffer,
            "cells",
            cells_size,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );

        let (uni_buf, u_re) = Self::ensure_buffer(
            &self.device,
            &mut self.uniform_buffer,
            "uniforms",
            std::mem::size_of::<Uniforms>() as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );

        let a_re = self.prepare_atlas(
            atlas_dirty,
            atlas_cols,
            atlas_rows,
            cell_width,
            cell_height,
            atlas_image,
        );

        let recreate_bind = targets.recreate_bg || c_re || u_re || a_re;
        self.ensure_bind_group(recreate_bind, &uni_buf, &cells_buf);

        let uniforms = Uniforms {
            cols: cols as u32,
            rows: rows as u32,
            cell_width: cell_width as u32,
            cell_height: cell_height as u32,
            atlas_cols: atlas_cols as u32,
            atlas_rows: atlas_rows as u32,
            scanlines: u32::from(scanlines),
            padding: 0,
        };
        self.queue.write_buffer(&uni_buf, 0, as_u8_bytes(&uniforms));

        super::gpu_cells::build_gpu_cells_into(
            grid,
            grid_cols,
            col_start,
            row_start,
            cols,
            rows,
            atlas_index,
            &mut self.cells_scratch,
        );
        self.queue
            .write_buffer(&cells_buf, 0, as_u8_slice(&self.cells_scratch));

        self.encode_draw_and_copy(
            cols,
            rows,
            targets.content_w,
            targets.content_h,
            targets.unpadded,
            targets.padded,
            out,
        );
    }

    fn encode_draw_and_copy(
        &mut self,
        cols: usize,
        rows: usize,
        content_w: u32,
        content_h: u32,
        unpadded: u32,
        padded: u32,
        out: &mut Vec<u8>,
    ) {
        let Some(target_tex) = self.texture.as_ref() else {
            return;
        };
        let Some(bind_gp) = self.bind_group.as_ref() else {
            return;
        };
        let Some(staging_buf) = self.staging_buffer.as_ref() else {
            return;
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render"),
            });
        {
            let target_view = target_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, bind_gp, &[]);
            render_pass.draw(0..6, 0..(cols * rows) as u32);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: target_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: staging_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: content_w,
                height: content_h,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some(ref buf) = self.staging_buffer {
            super::gpu_cells::copy_staging_to_out(
                buf,
                &self.device,
                content_w,
                content_h,
                unpadded,
                padded,
                out,
            );
        }
    }
}
