//! Fails the build when `shader.wgsl` stops parsing or validating.
//!
//! wgpu compiles WGSL at runtime, so without this test a shader mistake (like
//! a local variable shadowing the uniform) only shows up as a black screen on
//! a device.

#[test]
fn shader_is_valid_wgsl() {
    let module = naga::front::wgsl::parse_str(include_str!("../src/shader.wgsl"))
        .expect("shader.wgsl must parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("shader.wgsl must pass validation");
}
