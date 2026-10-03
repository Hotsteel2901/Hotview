//! GPU context shared by every surface renderer on a device.

use std::sync::Arc;

use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle};
use wgpu::{
    Adapter, Backends, Device, DeviceDescriptor, Instance, InstanceDescriptor, PowerPreference,
    Queue, RequestAdapterOptions, Surface, SurfaceTargetUnsafe,
};

/// The device-wide wgpu objects. Create once, share everywhere.
pub struct GpuContext {
    pub instance: Instance,
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
}

impl GpuContext {
    /// Create a context using Vulkan (Android/desktop), with GLES as a fallback.
    pub fn new() -> Result<Arc<Self>, String> {
        let mut descriptor = InstanceDescriptor::new_without_display_handle();
        descriptor.backends = Backends::VULKAN | Backends::GL;
        let instance = Instance::new(descriptor);
        Self::with_instance(instance)
    }

    pub fn with_instance(instance: Instance) -> Result<Arc<Self>, String> {
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|err| format!("no compatible GPU adapter: {err}"))?;

        let info = adapter.get_info();
        log::info!(
            "GPU adapter: {} ({:?}, {:?}, driver {})",
            info.name,
            info.backend,
            info.device_type,
            info.driver
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("hotview-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|err| format!("could not create GPU device: {err}"))?;

        Ok(Arc::new(Self {
            instance,
            adapter,
            device,
            queue,
        }))
    }

    /// Pick a swap chain format that needs no colour space conversion: we feed
    /// the GPU sRGB-encoded pixels directly (images) or produce sRGB-encoded
    /// pixels in the YUV shader, so a non-sRGB target keeps everything 1:1.
    pub fn preferred_surface_format(&self) -> wgpu::TextureFormat {
        wgpu::TextureFormat::Bgra8Unorm
    }
}

/// Create a wgpu surface for an `ANativeWindow` pointer.
///
/// # Safety
/// `window` must be a valid `ANativeWindow*` that outlives the returned surface.
pub unsafe fn surface_from_android_window(
    instance: &Instance,
    window: *mut std::ffi::c_void,
) -> Result<Surface<'static>, String> {
    let window = std::ptr::NonNull::new(window).ok_or("null ANativeWindow")?;
    let raw_window_handle = RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(window));
    let raw_display_handle = RawDisplayHandle::Android(AndroidDisplayHandle::new());

    unsafe {
        instance
            .create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display_handle),
                raw_window_handle,
            })
            .map_err(|err| format!("could not create wgpu surface: {err}"))
    }
}
