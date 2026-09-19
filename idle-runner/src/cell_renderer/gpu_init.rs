// SPDX-License-Identifier: MIT

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Uniforms {
    pub cols: u32,
    pub rows: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub atlas_cols: u32,
    pub atlas_rows: u32,
    pub scanlines: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct GpuCell {
    pub bg_color: u32,
    pub fg_color: u32,
    pub char_idx: u32,
    pub bold: u32,
}

// Field order is the drop order (Rust drops fields in declaration order):
// device-owned resources first, `device`/`queue` last. Dropping the device
// before its buffers/textures/pipeline risks driver teardown races — this
// struct is dropped per presentation session, not just at process exit.
pub struct GpuCellRenderer {
    pub bind_group: Option<wgpu::BindGroup>,
    pub atlas_texture: Option<wgpu::Texture>,
    pub texture: Option<wgpu::Texture>,
    pub staging_buffer: Option<wgpu::Buffer>,
    pub uniform_buffer: Option<wgpu::Buffer>,
    pub cells_buffer: Option<wgpu::Buffer>,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub atlas_sampler: wgpu::Sampler,
    pub queue: wgpu::Queue,
    pub device: wgpu::Device,
    pub target_width: u32,
    pub target_height: u32,
    pub atlas_width: usize,
    pub atlas_height: usize,
    /// Reused across frames to avoid allocating GpuCell vectors every draw.
    pub cells_scratch: Vec<GpuCell>,
}

fn block_on_future<F: std::future::Future>(future: F) -> F::Output {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| spin_block_on(future))
    } else {
        spin_block_on(future)
    }
}

/// `futures_lite::future::block_on` equivalent: drive a future to completion
/// with a waker that unparks the calling thread (what `pollster`/`futures_lite`
/// do internally — wgpu futures wake through this when GPU work completes).
fn spin_block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn waker(thread: std::thread::Thread) -> Waker {
        let data = Arc::into_raw(Arc::new(thread)) as *const ();
        unsafe fn clone(d: *const ()) -> RawWaker {
            let arc = std::mem::ManuallyDrop::new(unsafe {
                Arc::from_raw(d as *const std::thread::Thread)
            });
            RawWaker::new(Arc::into_raw(Arc::clone(&arc)) as *const (), &VTABLE)
        }
        unsafe fn wake(d: *const ()) {
            unsafe { Arc::from_raw(d as *const std::thread::Thread) }.unpark();
        }
        unsafe fn wake_by_ref(d: *const ()) {
            std::mem::ManuallyDrop::new(unsafe { Arc::from_raw(d as *const std::thread::Thread) })
                .unpark();
        }
        unsafe fn drop(d: *const ()) {
            std::mem::drop(unsafe { Arc::from_raw(d as *const std::thread::Thread) });
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        unsafe { Waker::from_raw(RawWaker::new(data, &VTABLE)) }
    }

    let waker = waker(std::thread::current());
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::park(),
        }
    }
}

impl GpuCellRenderer {
    pub fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let adapter = block_on_future(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| format!("No GPU adapter found: {e}"))?;

        let (device, queue) = block_on_future(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("idle-runner headless device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|e| format!("Failed to create wgpu device: {e}"))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cell shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader.wgsl"
            ))),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cell bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cell pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            ..Default::default()
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cell pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            atlas_sampler,
            target_width: 0,
            target_height: 0,
            texture: None,
            staging_buffer: None,
            uniform_buffer: None,
            cells_buffer: None,
            bind_group: None,
            atlas_texture: None,
            atlas_width: 0,
            atlas_height: 0,
            cells_scratch: Vec::new(),
        })
    }

    pub fn ensure_buffer(
        device: &wgpu::Device,
        current: &mut Option<wgpu::Buffer>,
        label: &str,
        size: u64,
        usage: wgpu::BufferUsages,
    ) -> (wgpu::Buffer, bool) {
        if let Some(buf) = current.as_ref()
            && buf.size() >= size
        {
            return (buf.clone(), false);
        }
        let new_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        });
        *current = Some(new_buf.clone());
        (new_buf, true)
    }

    pub fn ensure_texture(
        device: &wgpu::Device,
        current: &mut Option<wgpu::Texture>,
        label: &str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> (wgpu::Texture, bool) {
        if let Some(tex) = current.as_ref()
            && tex.width() == width
            && tex.height() == height
        {
            return (tex.clone(), false);
        }
        let new_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        *current = Some(new_tex.clone());
        (new_tex, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use std::time::Duration;

    #[test]
    fn spin_block_on_ready_future_returns_value() {
        assert_eq!(spin_block_on(async { 42 }), 42);
        assert_eq!(spin_block_on(std::future::ready("ok")), "ok");
    }

    #[test]
    fn spin_block_on_pending_future_completes_on_wake() {
        // A future that is Pending once, then Ready after a spawned thread
        // wakes the parking thread — proves the RawWaker unparks us.
        struct Once {
            fired: Arc<AtomicBool>,
        }
        impl std::future::Future for Once {
            type Output = u32;
            fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
                if self.fired.swap(true, Ordering::SeqCst) {
                    return Poll::Ready(7);
                }
                let waker = cx.waker().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(20));
                    waker.wake();
                });
                Poll::Pending
            }
        }
        let fired = Arc::new(AtomicBool::new(false));
        let out = spin_block_on(Once {
            fired: Arc::clone(&fired),
        });
        assert_eq!(out, 7);
    }

    #[test]
    fn waker_clone_and_wake_by_ref_are_sound() {
        // Poll a future that clones its waker and calls wake_by_ref:
        // clone must add a ref (not alias) and wake_by_ref must not
        // consume the original — exercised by repeated parking.
        struct MultiWake {
            polls: Arc<AtomicUsize>,
        }
        impl std::future::Future for MultiWake {
            type Output = ();
            fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                let n = self.polls.fetch_add(1, Ordering::SeqCst);
                if n >= 2 {
                    return Poll::Ready(());
                }
                let w1 = cx.waker().clone();
                let w2 = w1.clone();
                std::thread::spawn(move || {
                    w2.wake_by_ref();
                    w1.wake_by_ref();
                    w1.wake();
                });
                Poll::Pending
            }
        }
        let polls = Arc::new(AtomicUsize::new(0));
        spin_block_on(MultiWake {
            polls: Arc::clone(&polls),
        });
        assert!(polls.load(Ordering::SeqCst) >= 3);
    }

    #[test]
    fn block_on_future_works_outside_tokio() {
        assert_eq!(block_on_future(async { "plain" }), "plain");
    }

    #[test]
    fn block_on_future_works_inside_tokio() {
        // block_in_place requires a multi-thread runtime.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let out = rt.block_on(async { block_on_future(async { 9u32 }) });
        assert_eq!(out, 9);
    }
}
