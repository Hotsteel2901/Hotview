//! hotview-render: the wgpu renderer shared by the Android viewer (and usable
//! anywhere else that can hand us a surface).

pub mod gpu;
pub mod renderer;

pub use gpu::{surface_from_android_window, GpuContext};
pub use renderer::{fit_transform, MediaRenderer};

pub use raw_window_handle;
pub use wgpu;
