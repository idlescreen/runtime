// SPDX-License-Identifier: MIT

//! GPU resource setup helpers for the cell renderer.

use super::gpu_init::GpuCellRenderer;

pub(super) struct RenderTargets {
    pub content_w: u32,
    pub content_h: u32,
    pub unpadded: u32,
    pub padded: u32,
    pub recreate_bg: bool,
}

impl GpuCellRenderer {
    pub(super) fn prepare_targets(
        &mut self,
        cols: usize,
        rows: usize,
        cell_width: usize,
        cell_height: usize,
    ) -> Option<RenderTargets> {
        let (content_w, content_h) = ((cols * cell_width) as u32, (rows * cell_height) as u32);
        if content_w == 0 || content_h == 0 {
            return None;
        }

        let unpadded = content_w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded + (align - unpadded % align) % align;

        let mut recreate_bg = false;
        if self.target_width != content_w
            || self.target_height != content_h
            || self.texture.is_none()
        {
            self.target_width = content_w;
            self.target_height = content_h;
            Self::ensure_texture(
                &self.device,
                &mut self.texture,
                "cell render target",
                content_w,
                content_h,
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            self.staging_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("staging"),
                size: (padded * content_h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            recreate_bg = true;
        }

        Some(RenderTargets {
            content_w,
            content_h,
            unpadded,
            padded,
            recreate_bg,
        })
    }

    pub(super) fn prepare_atlas(
        &mut self,
        atlas_dirty: bool,
        atlas_cols: usize,
        atlas_rows: usize,
        cell_width: usize,
        cell_height: usize,
        atlas_image: &[u8],
    ) -> bool {
        let (atlas_w, atlas_h) = (atlas_cols * cell_width, atlas_rows * cell_height);
        let mut a_re = false;
        if atlas_dirty
            || self.atlas_texture.is_none()
            || self.atlas_width != atlas_w
            || self.atlas_height != atlas_h
        {
            self.atlas_width = atlas_w;
            self.atlas_height = atlas_h;
            Self::ensure_texture(
                &self.device,
                &mut self.atlas_texture,
                "atlas",
                atlas_w as u32,
                atlas_h as u32,
                wgpu::TextureFormat::R8Unorm,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            );
            a_re = true;
        }

        if (atlas_dirty || a_re)
            && let Some(ref atlas_tex) = self.atlas_texture
        {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: atlas_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                atlas_image,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(atlas_w as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: atlas_w as u32,
                    height: atlas_h as u32,
                    depth_or_array_layers: 1,
                },
            );
        }

        a_re
    }

    pub(super) fn ensure_bind_group(
        &mut self,
        recreate: bool,
        uni_buf: &wgpu::Buffer,
        cells_buf: &wgpu::Buffer,
    ) {
        if !recreate && self.bind_group.is_some() {
            return;
        }
        let Some(atlas) = self.atlas_texture.as_ref() else {
            return;
        };
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        self.bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uni_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cells_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.atlas_sampler),
                },
            ],
        }));
    }
}
