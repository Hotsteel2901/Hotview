struct Uniforms {
    transform: vec4<f32>,
    coeff0: vec4<f32>,
    coeff1: vec4<f32>,
    coeff2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uni: Uniforms;

@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex0: texture_2d<f32>;
@group(0) @binding(3) var tex1: texture_2d<f32>;
@group(0) @binding(4) var tex2: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let p = positions[vi];
    var out: VsOut;
    out.pos = vec4<f32>(
        p.x * uni.transform.x + uni.transform.z,
        p.y * uni.transform.y + uni.transform.w,
        0.0,
        1.0,
    );
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

@fragment
fn fs_rgba(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex0, samp, in.uv);
}

@fragment
fn fs_yuv_planar(in: VsOut) -> @location(0) vec4<f32> {
    let luma = textureSample(tex0, samp, in.uv).r;
    let chroma_u = textureSample(tex1, samp, in.uv).r;
    let chroma_v = textureSample(tex2, samp, in.uv).r;
    let c = vec3<f32>(luma, chroma_u, chroma_v);
    return vec4<f32>(
        dot(uni.coeff0.xyz, c) + uni.coeff0.w,
        dot(uni.coeff1.xyz, c) + uni.coeff1.w,
        dot(uni.coeff2.xyz, c) + uni.coeff2.w,
        1.0,
    );
}

@fragment
fn fs_yuv_semi(in: VsOut) -> @location(0) vec4<f32> {
    let luma = textureSample(tex0, samp, in.uv).r;
    let uv = textureSample(tex1, samp, in.uv).rg;
    let c = vec3<f32>(luma, uv.x, uv.y);
    return vec4<f32>(
        dot(uni.coeff0.xyz, c) + uni.coeff0.w,
        dot(uni.coeff1.xyz, c) + uni.coeff1.w,
        dot(uni.coeff2.xyz, c) + uni.coeff2.w,
        1.0,
    );
}
