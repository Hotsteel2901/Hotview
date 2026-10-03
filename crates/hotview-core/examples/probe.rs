//! Tiny diagnostic tool for the decode pipeline.
//!
//! ```bash
//! cargo run --example probe --features ffmpeg,audio -- path/to/media.mp4
//! cargo run --example probe --features ffmpeg -- path/to/photo.avif
//! ```

use std::path::PathBuf;

use hotview_core::video::ffmpeg::{FfmpegAudioDecoder, FfmpegVideoDecoder};
use hotview_core::video::{AudioDecoder, DecodeOutcome, VideoDecoder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .expect("usage: probe <file>")
        .into();

    match FfmpegVideoDecoder::open(&path) {
        Ok(mut video) => {
            let info = video.info().clone();
            println!(
                "video: {}x{} @ {:.2} fps, {:.2}s, codec {}, audio {}",
                info.width,
                info.height,
                info.fps.unwrap_or(0.0),
                info.duration_us.unwrap_or(0) as f64 / 1_000_000.0,
                info.codec,
                info.has_audio
            );
            match video.next_frame()? {
                DecodeOutcome::Frame(frame) => println!(
                    "first frame: {}x{} pts={:?}",
                    frame.dimensions().0,
                    frame.dimensions().1,
                    frame.pts_us()
                ),
                other => println!("first frame: {other:?}"),
            }
        }
        Err(err) => println!("no video stream: {err}"),
    }

    #[cfg(feature = "audio")]
    {
        match FfmpegAudioDecoder::open(&path)? {
            Some(mut audio) => {
                println!("audio: {} Hz, {} ch", audio.sample_rate(), audio.channels());
                match audio.next_chunk()? {
                    Some(chunk) => println!("first audio chunk: {} samples", chunk.len()),
                    None => println!("audio: no samples"),
                }
            }
            None => println!("audio: none"),
        }
    }

    Ok(())
}
