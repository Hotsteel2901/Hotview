//! Headless rendering checks for the wgpu media renderer.
//!
//! The desktop app draws with egui, so this is the only automated exercise of
//! `MediaRenderer` / `shader.wgsl`. It renders into an offscreen texture and
//! reads the pixels back, proving the pipelines actually draw.

use hotview_core::{
    ChromaLayout, ColorMatrix, ColorRange, MediaFrame, Plane, PlanarFrame, RgbaFrame, YuvInfo,
};
use hotview_render::{GpuContext, MediaRenderer};

const W: u32 = 32;
const H: u32 = 24;

fn padded_row(width: u32) -> u32 {
    let raw = width * 4;
    raw.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

fn context() -> Option<std::sync::Arc<GpuContext>> {
    match GpuContext::new() {
        Ok(context) => Some(context),
        Err(err) => {
            eprintln!("skipping headless render test: no GPU adapter ({err})");
            None
        }
    }
}

fn render_and_read(ctx: &GpuContext, frame: &MediaFrame, transform: [f32; 4]) -> Vec<u8> {
    let device = &ctx.device;
    let queue = &ctx.queue;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hotview-test-target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let mut renderer = MediaRenderer::new(device, wgpu::TextureFormat::Rgba8Unorm);
    renderer.set_frame(device, queue, frame);
    renderer.render(device, queue, &view, transform, YuvInfo::default());

    let row = padded_row(W);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hotview-test-readback"),
        size: (row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().expect("map callback").expect("map ok");

    let mapped = slice.get_mapped_range().expect("mapped range");
    let mut pixels = vec![0u8; (W * H * 4) as usize];
    for y in 0..H as usize {
        let src = y * row as usize;
        let dst = y * (W * 4) as usize;
        pixels[dst..dst + (W * 4) as usize]
            .copy_from_slice(&mapped[src..src + (W * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    pixels
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * W + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

fn frame_with(pixels: &[[u8; 4]; 4]) -> MediaFrame {
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let quadrant = match (x >= W / 2, y >= H / 2) {
                (false, false) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (true, true) => 3,
            };
            data.extend_from_slice(&pixels[quadrant]);
        }
    }
    MediaFrame::Rgba(RgbaFrame::new(W, H, data, None))
}

#[test]
fn rgba_frame_is_drawn() {
    let Some(ctx) = context() else { return };
    let frame = frame_with(&[
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    ]);
    let pixels = render_and_read(&ctx, &frame, [1.0, 1.0, 0.0, 0.0]);

    let red = pixel(&pixels, W / 4, H / 4);
    let green = pixel(&pixels, W * 3 / 4, H / 4);
    let blue = pixel(&pixels, W / 4, H * 3 / 4);
    let white = pixel(&pixels, W * 3 / 4, H * 3 / 4);

    assert!(red[0] > 200 && red[1] < 60, "expected red, got {red:?}");
    assert!(
        green[1] > 200 && green[0] < 60,
        "expected green, got {green:?}"
    );
    assert!(
        blue[2] > 200 && blue[0] < 60,
        "expected blue, got {blue:?}"
    );
    assert!(
        white.iter().take(3).all(|value| *value > 200),
        "expected white, got {white:?}"
    );
}

#[test]
fn rgba_alpha_hides_transparent_rgb_and_preserves_partial_alpha() {
    let Some(ctx) = context() else { return };
    let transparent = frame_with(&[[0, 0, 255, 0]; 4]);
    let transparent_pixels = render_and_read(&ctx, &transparent, [1.0, 1.0, 0.0, 0.0]);
    let transparent_center = pixel(&transparent_pixels, W / 2, H / 2);
    assert_eq!(transparent_center, [0, 0, 0, 0]);

    let translucent = frame_with(&[[255, 0, 0, 128]; 4]);
    let translucent_pixels = render_and_read(&ctx, &translucent, [1.0, 1.0, 0.0, 0.0]);
    let translucent_center = pixel(&translucent_pixels, W / 2, H / 2);
    assert!(
        (127..=128).contains(&translucent_center[0])
            && translucent_center[1] == 0
            && translucent_center[2] == 0
            && (127..=128).contains(&translucent_center[3]),
        "expected premultiplied half-alpha red, got {translucent_center:?}"
    );
}

#[test]
fn nv12_frame_is_drawn() {
    let Some(ctx) = context() else { return };

    // Limited-range white: Y=235, U=V=128.
    let y = vec![235u8; (W * H) as usize];
    let uv = vec![128u8; (W * H / 2) as usize];
    let frame = MediaFrame::Planar(PlanarFrame {
        width: W,
        height: H,
        y: Plane::new(y, W as usize),
        chroma: ChromaLayout::SemiPlanar {
            uv: Plane::new(uv, W as usize),
            vu: false,
        },
        info: YuvInfo {
            matrix: ColorMatrix::Bt709,
            range: ColorRange::Limited,
        },
        pts_us: None,
    });
    let pixels = render_and_read(&ctx, &frame, [1.0, 1.0, 0.0, 0.0]);

    let center = pixel(&pixels, W / 2, H / 2);
    assert!(
        center.iter().take(3).all(|value| *value > 200),
        "expected white NV12 frame, got {center:?}"
    );
}

#[test]
fn fit_transform_keeps_aspect_ratio() {
    // Portrait media inside a landscape surface: full height, centred width.
    let transform =
        hotview_render::fit_transform((2000, 1000), (500, 1000), 1.0, [0.0, 0.0], false);
    assert!((transform[1] - 1.0).abs() < 1e-6);
    // 500 of 2000 px wide -> the quad covers a quarter of the NDC width.
    assert!((transform[0] - 0.25).abs() < 1e-6);

    // Cover mode fills the surface instead of leaving bars.
    let cover = hotview_render::fit_transform((2000, 1000), (500, 1000), 1.0, [0.0, 0.0], true);
    assert!((cover[1] - 4.0).abs() < 1e-6);

    // Zooming never divides by zero.
    let zoomed =
        hotview_render::fit_transform((1000, 1000), (500, 500), 3.0, [0.0, 0.0], false);
    assert!((zoomed[0] - 3.0).abs() < 1e-6);
}
