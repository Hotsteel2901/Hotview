//! Frame representation shared by every decoder.
//!
//! Desktop FFmpeg decoding hands us packed RGBA, Android MediaCodec hands us
//! YUV 4:2:0 planes. Both are represented here so that the renderer (and the
//! thumbnail code) can treat them uniformly.

use rayon::prelude::*;

/// Colour matrix used by a YUV frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMatrix {
    Bt601,
    Bt709,
    Bt2020,
}

/// Whether YUV samples use studio (limited) or full range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorRange {
    Limited,
    Full,
}

/// Colour description of a [`PlanarFrame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YuvInfo {
    pub matrix: ColorMatrix,
    pub range: ColorRange,
}

impl Default for YuvInfo {
    fn default() -> Self {
        Self {
            matrix: ColorMatrix::Bt709,
            range: ColorRange::Limited,
        }
    }
}

impl YuvInfo {
    /// Pick a sensible matrix for a video resolution when the container does
    /// not tell us anything better.
    pub fn guess_for_size(width: u32, height: u32) -> Self {
        let _ = width;
        Self {
            matrix: if height >= 720 {
                ColorMatrix::Bt709
            } else {
                ColorMatrix::Bt601
            },
            range: ColorRange::Limited,
        }
    }
}

/// A tightly packed RGBA8 image, one byte per channel, row major.
#[derive(Clone, Debug)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes.
    pub data: Vec<u8>,
    /// Presentation timestamp in microseconds, when known.
    pub pts_us: Option<i64>,
}

impl RgbaFrame {
    pub fn new(width: u32, height: u32, data: Vec<u8>, pts_us: Option<i64>) -> Self {
        debug_assert_eq!(data.len(), width as usize * height as usize * 4);
        Self {
            width,
            height,
            data,
            pts_us,
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// A single colour plane.
#[derive(Clone, Debug)]
pub struct Plane {
    pub data: Vec<u8>,
    /// Distance in bytes between two rows.
    pub stride: usize,
}

impl Plane {
    pub fn new(data: Vec<u8>, stride: usize) -> Self {
        Self { data, stride }
    }
}

/// How chroma samples are laid out.
#[derive(Clone, Debug)]
pub enum ChromaLayout {
    /// I420: separate U and V planes, half resolution in both directions.
    Planar { u: Plane, v: Plane },
    /// NV12 (`vu == false`) or NV21 (`vu == true`): interleaved UV plane.
    SemiPlanar { uv: Plane, vu: bool },
}

/// A YUV 4:2:0 frame (I420 or NV12/NV21) with tightly described planes.
#[derive(Clone, Debug)]
pub struct PlanarFrame {
    pub width: u32,
    pub height: u32,
    pub y: Plane,
    pub chroma: ChromaLayout,
    pub info: YuvInfo,
    pub pts_us: Option<i64>,
}

struct Coeffs {
    cy: i32,
    y_off: i32,
    crv: i32,
    cgu: i32,
    cgv: i32,
    cbu: i32,
}

fn coeffs(info: YuvInfo) -> Coeffs {
    let (cy, y_off) = match info.range {
        ColorRange::Limited => (298, 16),
        ColorRange::Full => (256, 0),
    };
    match (info.matrix, info.range) {
        (ColorMatrix::Bt601, ColorRange::Limited) => Coeffs {
            cy,
            y_off,
            crv: 409,
            cgu: -100,
            cgv: -208,
            cbu: 516,
        },
        (ColorMatrix::Bt601, ColorRange::Full) => Coeffs {
            cy,
            y_off,
            crv: 359,
            cgu: -88,
            cgv: -183,
            cbu: 454,
        },
        (ColorMatrix::Bt709, ColorRange::Limited) => Coeffs {
            cy,
            y_off,
            crv: 459,
            cgu: -55,
            cgv: -136,
            cbu: 541,
        },
        (ColorMatrix::Bt709, ColorRange::Full) => Coeffs {
            cy,
            y_off,
            crv: 403,
            cgu: -48,
            cgv: -120,
            cbu: 475,
        },
        (ColorMatrix::Bt2020, ColorRange::Limited) => Coeffs {
            cy,
            y_off,
            crv: 430,
            cgu: -48,
            cgv: -167,
            cbu: 548,
        },
        (ColorMatrix::Bt2020, ColorRange::Full) => Coeffs {
            cy,
            y_off,
            crv: 378,
            cgu: -42,
            cgv: -146,
            cbu: 482,
        },
    }
}

#[inline(always)]
fn clamp8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

#[inline(always)]
fn yuv_to_rgb(c: &Coeffs, y: u8, u: u8, v: u8) -> [u8; 3] {
    let yy = y as i32 - c.y_off;
    let uu = u as i32 - 128;
    let vv = v as i32 - 128;
    [
        clamp8((c.cy * yy + c.crv * vv + 128) >> 8),
        clamp8((c.cy * yy + c.cgu * uu + c.cgv * vv + 128) >> 8),
        clamp8((c.cy * yy + c.cbu * uu + 128) >> 8),
    ]
}

impl PlanarFrame {
    /// Convert to RGBA. Parallelised over rows with rayon; a 4K frame takes a
    /// couple of milliseconds on a modern phone.
    pub fn to_rgba(&self) -> RgbaFrame {
        let width = self.width as usize;
        let height = self.height as usize;
        let mut out = vec![0u8; width * height * 4];
        let c = coeffs(self.info);
        let y_stride = self.y.stride;

        match &self.chroma {
            ChromaLayout::Planar { u, v } => {
                let (u_stride, v_stride) = (u.stride, v.stride);
                out.par_chunks_mut(width * 4)
                    .enumerate()
                    .for_each(|(row, dst)| {
                        let y_row = &self.y.data[row * y_stride..row * y_stride + width];
                        let crow = row / 2;
                        let u_row = &u.data[crow * u_stride..];
                        let v_row = &v.data[crow * v_stride..];
                        for x in 0..width {
                            let [r, g, b] = yuv_to_rgb(&c, y_row[x], u_row[x / 2], v_row[x / 2]);
                            dst[x * 4] = r;
                            dst[x * 4 + 1] = g;
                            dst[x * 4 + 2] = b;
                            dst[x * 4 + 3] = 255;
                        }
                    });
            }
            ChromaLayout::SemiPlanar { uv, vu } => {
                let uv_stride = uv.stride;
                let vu = *vu;
                out.par_chunks_mut(width * 4)
                    .enumerate()
                    .for_each(|(row, dst)| {
                        let y_row = &self.y.data[row * y_stride..row * y_stride + width];
                        let uv_row = &uv.data[(row / 2) * uv_stride..];
                        for x in 0..width {
                            let ci = (x / 2) * 2;
                            let (u, v) = if vu {
                                (uv_row[ci + 1], uv_row[ci])
                            } else {
                                (uv_row[ci], uv_row[ci + 1])
                            };
                            let [r, g, b] = yuv_to_rgb(&c, y_row[x], u, v);
                            dst[x * 4] = r;
                            dst[x * 4 + 1] = g;
                            dst[x * 4 + 2] = b;
                            dst[x * 4 + 3] = 255;
                        }
                    });
            }
        }

        RgbaFrame {
            width: self.width,
            height: self.height,
            data: out,
            pts_us: self.pts_us,
        }
    }
}

/// Any decoded frame.
#[derive(Clone, Debug)]
pub enum MediaFrame {
    Rgba(RgbaFrame),
    Planar(PlanarFrame),
}

impl MediaFrame {
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            MediaFrame::Rgba(f) => f.dimensions(),
            MediaFrame::Planar(f) => (f.width, f.height),
        }
    }

    pub fn pts_us(&self) -> Option<i64> {
        match self {
            MediaFrame::Rgba(f) => f.pts_us,
            MediaFrame::Planar(f) => f.pts_us,
        }
    }

    pub fn to_rgba(&self) -> RgbaFrame {
        match self {
            MediaFrame::Rgba(f) => f.clone(),
            MediaFrame::Planar(f) => f.to_rgba(),
        }
    }
}
