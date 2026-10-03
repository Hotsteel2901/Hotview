//! Tiny diagnostic tool for the image pipeline.
//!
//! ```bash
//! cargo run --example image_probe --features ffmpeg -- path/to/photo.avif
//! ```

use std::path::PathBuf;

use hotview_core::decode_file_scaled;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .expect("usage: image_probe <file>")
        .into();

    let frame = decode_file_scaled(&path, 4096)?;
    println!(
        "decoded {} ({}x{}), {} bytes of RGBA",
        path.display(),
        frame.width,
        frame.height,
        frame.data.len()
    );
    Ok(())
}
