//! The media renderer: draws RGBA images and YUV video frames on a wgpu
//! surface with fit/zoom/pan support.

use bytemuck::{Pod, Zeroable};
use hotview_core::{ColorMatrix, ColorRange, MediaFrame, PlanarFrame, YuvInfo};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BufferBindingType, BufferDescriptor,
    BufferUsages, Device, Extent3d, FilterMode, Origin3d, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages,
    StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, LoadOp, Operations, PipelineLayoutDescriptor,
    ColorTargetState, ColorWrites, FragmentState, MultisampleState, PipelineCompilationOptions,
    PrimitiveState, VertexState,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    /// `(scale_x, scale_y, offset_x, offset_y)` in NDC.
    transform: [f32; 4],
    /// YUV -> RGB rows: `rgb = row * (y, u, v) + row.w`.
    coeff0: [f32; 4],
    coeff1: [f32; 4],
    coeff2: [f32; 4],
}

impl Uniforms {
    fn new(transform: [f32; 4], yuv: YuvInfo) -> Self {
        let rows = yuv_to_rgb_rows(yuv);
        Self {
            transform,
            coeff0: rows[0],
            coeff1: rows[1],
            coeff2: rows[2],
        }
    }
}

/// Coefficients converted to normalised float space (textures return 0..1).
fn yuv_to_rgb_rows(info: YuvInfo) -> [[f32; 4]; 3] {
    let (cy, y_off) = match info.range {
        ColorRange::Limited => (1.164_383_6_f32, 16.0_f32 / 255.0),
        ColorRange::Full => (1.0_f32, 0.0_f32),
    };
    let (crv, cgu, cgv, cbu) = match (info.matrix, info.range) {
        (ColorMatrix::Bt601, ColorRange::Limited) => (1.596_026_9, -0.391_762_24, -0.812_967_8, 2.017_232_3),
        (ColorMatrix::Bt601, ColorRange::Full) => (1.402, -0.344_136, -0.714_136, 1.772),
        (ColorMatrix::Bt709, ColorRange::Limited) => (1.792_741_1, -0.213_248_53, -0.532_909_5, 2.112_402_3),
        (ColorMatrix::Bt709, ColorRange::Full) => (1.574_8, -0.187_324_38, -0.468_124_05, 1.855_6),
        (ColorMatrix::Bt2020, ColorRange::Limited) => (1.678_67, -0.187_326_51, -0.650_441_5, 2.141_768),
        (ColorMatrix::Bt2020, ColorRange::Full) => (1.4746, -0.164_553_23, -0.571_353_1, 1.881_4),
    };
    let half = 128.0 / 255.0;
    [
        [cy, 0.0, crv, -cy * y_off - crv * half],
        [cy, cgu, cgv, -cy * y_off - (cgu + cgv) * half],
        [cy, cbu, 0.0, -cy * y_off - cbu * half],
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameKind {
    None,
    Rgba,
    YuvPlanar,
    YuvSemi,
}

/// Renderer for one media surface.
pub struct MediaRenderer {
    uniform_buffer: wgpu::Buffer,
    bind_group_layout: BindGroupLayout,
    bind_group: BindGroup,
    sampler: Sampler,
    pipeline_rgba: RenderPipeline,
    pipeline_yuv_planar: RenderPipeline,
    pipeline_yuv_semi: RenderPipeline,
    /// Y plane, or the RGBA texture.
    plane0: Option<Texture>,
    /// U plane (planar) or interleaved UV (semi-planar).
    plane1: Option<Texture>,
    /// V plane (planar only).
    plane2: Option<Texture>,
    dummy: Texture,
    kind: FrameKind,
    size: (u32, u32),
    uniforms: Uniforms,
}

impl MediaRenderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("hotview-media-shader"),
            source: ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("hotview-media-bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
            ],
        });

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("hotview-media-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let make_pipeline = |label: &str, fs: &str| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                fragment: Some(FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: PipelineCompilationOptions::default(),
                    targets: &[Some(ColorTargetState {
                        format,
                        // RGBA image textures use straight alpha. Blend into a
                        // premultiplied target so transparent RGB (often blue
                        // in PNGs) never leaks through to the surface.
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let pipeline_rgba = make_pipeline("hotview-media-rgba", "fs_rgba");
        let pipeline_yuv_planar = make_pipeline("hotview-media-yuv-planar", "fs_yuv_planar");
        let pipeline_yuv_semi = make_pipeline("hotview-media-yuv-semi", "fs_yuv_semi");

        let uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("hotview-media-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("hotview-media-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let dummy = device.create_texture(&TextureDescriptor {
            label: Some("hotview-media-dummy"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let dummy_view = dummy.create_view(&TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("hotview-media-bg"),
            layout: &bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&dummy_view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&dummy_view),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(&dummy_view),
                },
            ],
        });

        Self {
            uniform_buffer,
            bind_group_layout,
            bind_group,
            sampler,
            pipeline_rgba,
            pipeline_yuv_planar,
            pipeline_yuv_semi,
            plane0: None,
            plane1: None,
            plane2: None,
            dummy,
            kind: FrameKind::None,
            size: (0, 0),
            uniforms: Uniforms::new([1.0, 1.0, 0.0, 0.0], YuvInfo::default()),
        }
    }

    pub fn has_frame(&self) -> bool {
        self.kind != FrameKind::None
    }

    pub fn frame_size(&self) -> (u32, u32) {
        self.size
    }

    /// Upload a frame, recreating textures when the size or layout changes.
    pub fn set_frame(&mut self, device: &Device, queue: &Queue, frame: &MediaFrame) {
        match frame {
            MediaFrame::Rgba(rgba) => {
                let size = (rgba.width, rgba.height);
                let texture = ensure_texture(
                    device,
                    &mut self.plane0,
                    size,
                    TextureFormat::Rgba8Unorm,
                    "hotview-rgba",
                    self.size == size && self.kind == FrameKind::Rgba,
                );
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: TextureAspect::All,
                    },
                    &rgba.data,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(rgba.width * 4),
                        rows_per_image: Some(rgba.height),
                    },
                    Extent3d {
                        width: rgba.width,
                        height: rgba.height,
                        depth_or_array_layers: 1,
                    },
                );
                let changed = self.kind != FrameKind::Rgba || self.size != size;
                self.kind = FrameKind::Rgba;
                self.size = size;
                if changed {
                    self.rebuild_bind_group(device);
                }
            }
            MediaFrame::Planar(planar) => {
                self.upload_planar(device, queue, planar);
            }
        }
    }

    fn upload_planar(&mut self, device: &Device, queue: &Queue, frame: &PlanarFrame) {
        let y_size = (frame.width, frame.height);
        let changed_kind = self.kind != FrameKind::YuvPlanar && self.kind != FrameKind::YuvSemi;

        ensure_texture(
            device,
            &mut self.plane0,
            y_size,
            TextureFormat::R8Unorm,
            "hotview-y",
            self.size == y_size && !changed_kind,
        );
        write_plane(
            queue,
            self.plane0.as_ref().expect("y texture"),
            &frame.y.data,
            frame.y.stride,
            frame.width,
            frame.height,
        );

        let (kind, chroma_w, chroma_h) = match &frame.chroma {
            hotview_core::ChromaLayout::Planar { u, v } => {
                let cw = frame.width.div_ceil(2);
                let ch = frame.height.div_ceil(2);
                ensure_texture(
                    device,
                    &mut self.plane1,
                    (cw, ch),
                    TextureFormat::R8Unorm,
                    "hotview-u",
                    self.size == y_size && !changed_kind,
                );
                write_plane(
                    queue,
                    self.plane1.as_ref().expect("u texture"),
                    &u.data,
                    u.stride,
                    cw,
                    ch,
                );
                ensure_texture(
                    device,
                    &mut self.plane2,
                    (cw, ch),
                    TextureFormat::R8Unorm,
                    "hotview-v",
                    self.size == y_size && !changed_kind,
                );
                write_plane(
                    queue,
                    self.plane2.as_ref().expect("v texture"),
                    &v.data,
                    v.stride,
                    cw,
                    ch,
                );
                (FrameKind::YuvPlanar, cw, ch)
            }
            hotview_core::ChromaLayout::SemiPlanar { uv, vu } => {
                let cw = frame.width.div_ceil(2);
                let ch = frame.height.div_ceil(2);
                let data: std::borrow::Cow<'_, [u8]> = if *vu {
                    // NV21: swap U and V so the shader always sees NV12.
                    let mut swapped = uv.data.clone();
                    for pair in swapped.chunks_exact_mut(2) {
                        pair.swap(0, 1);
                    }
                    std::borrow::Cow::Owned(swapped)
                } else {
                    std::borrow::Cow::Borrowed(&uv.data)
                };
                ensure_texture(
                    device,
                    &mut self.plane1,
                    (cw, ch),
                    TextureFormat::Rg8Unorm,
                    "hotview-uv",
                    self.size == y_size && !changed_kind,
                );
                write_plane(
                    queue,
                    self.plane1.as_ref().expect("uv texture"),
                    &data,
                    uv.stride,
                    cw,
                    ch,
                );
                (FrameKind::YuvSemi, cw, ch)
            }
        };
        let _ = chroma_w;
        let _ = chroma_h;
        let changed = changed_kind || self.size != y_size;
        self.kind = kind;
        self.size = y_size;
        if changed {
            self.rebuild_bind_group(device);
        }
    }

    fn rebuild_bind_group(&mut self, device: &Device) {
        let view = |texture: &Option<Texture>| {
            texture
                .as_ref()
                .map(|t| t.create_view(&TextureViewDescriptor::default()))
        };
        let v0 = view(&self.plane0);
        let v1 = view(&self.plane1);
        let v2 = view(&self.plane2);
        let dummy_view = self.dummy.create_view(&TextureViewDescriptor::default());
        let t0 = v0.as_ref().unwrap_or(&dummy_view);
        let t1 = v1.as_ref().unwrap_or(&dummy_view);
        let t2 = v2.as_ref().unwrap_or(&dummy_view);

        self.bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("hotview-media-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(t0),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(t1),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::TextureView(t2),
                },
            ],
        });
    }

    /// Draw the current frame into `view`, using the given NDC transform.
    pub fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        view: &TextureView,
        transform: [f32; 4],
        yuv: YuvInfo,
    ) {
        self.uniforms = Uniforms::new(transform, yuv);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&self.uniforms));

        let pipeline = match self.kind {
            FrameKind::Rgba => &self.pipeline_rgba,
            FrameKind::YuvPlanar => &self.pipeline_yuv_planar,
            FrameKind::YuvSemi => &self.pipeline_yuv_semi,
            FrameKind::None => return,
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hotview-media-encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("hotview-media-pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}

fn texture_entry(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn ensure_texture<'a>(
    device: &Device,
    slot: &'a mut Option<Texture>,
    size: (u32, u32),
    format: TextureFormat,
    label: &str,
    _reuse_hint: bool,
) -> &'a Texture {
    let need_new = match slot {
        Some(texture) => {
            texture.width() != size.0 || texture.height() != size.1 || texture.format() != format
        }
        None => true,
    };
    if need_new {
        *slot = Some(device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        }));
    }
    slot.as_ref().expect("texture")
}

/// Upload one plane honouring the source stride.
fn write_plane(
    queue: &Queue,
    texture: &Texture,
    data: &[u8],
    stride: usize,
    width: u32,
    height: u32,
) {
    let width = width.max(1);
    let height = height.max(1);
    let stride = stride.max(width as usize);
    let needed = stride * height as usize;
    if data.len() < needed {
        return;
    }
    queue.write_texture(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        data,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(stride as u32),
            rows_per_image: Some(height),
        },
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Compute the NDC transform for "contain" (or "cover") fitting plus user
/// zoom/pan.
///
/// * `surface` – target size in pixels.
/// * `media` – source size in pixels.
/// * `user_scale` – 1.0 means fit, values above zoom in.
/// * `pan_px` – translation in surface pixels, positive x right / y down.
/// * `fill` – cover the surface instead of fitting inside it.
pub fn fit_transform(
    surface: (u32, u32),
    media: (u32, u32),
    user_scale: f32,
    pan_px: [f32; 2],
    fill: bool,
) -> [f32; 4] {
    if surface.0 == 0 || surface.1 == 0 || media.0 == 0 || media.1 == 0 {
        return [1.0, 1.0, 0.0, 0.0];
    }
    let sw = surface.0 as f32;
    let sh = surface.1 as f32;
    let mw = media.0 as f32;
    let mh = media.1 as f32;

    let fit = if fill {
        (sw / mw).max(sh / mh)
    } else {
        (sw / mw).min(sh / mh)
    };
    let scale = (fit * user_scale).max(1e-5);
    let display = (mw * scale, mh * scale);

    let max_pan_x = ((display.0 - sw) / 2.0).max(0.0);
    let max_pan_y = ((display.1 - sh) / 2.0).max(0.0);
    let pan_x = pan_px[0].clamp(-max_pan_x, max_pan_x);
    let pan_y = pan_px[1].clamp(-max_pan_y, max_pan_y);

    [
        display.0 / sw,
        display.1 / sh,
        pan_x / sw * 2.0,
        -pan_y / sh * 2.0,
    ]
}
