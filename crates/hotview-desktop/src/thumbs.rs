//! Background thumbnail loader: a small worker pool that decodes images and
//! first video frames off the UI thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crossbeam_channel::{unbounded, Receiver, Sender};
use hotview_core::video::ffmpeg::FfmpegVideoDecoder;
use hotview_core::video::DecodeOutcome;
use hotview_core::{thumbnail, thumbnail_file, RgbaFrame, VideoDecoder};

/// Longest edge of a generated thumbnail.
pub const THUMB_SIZE: u32 = 420;

pub struct ThumbnailLoader {
    jobs: Sender<PathBuf>,
    results: Receiver<(PathBuf, Result<RgbaFrame, String>)>,
    pending: HashSet<PathBuf>,
}

impl ThumbnailLoader {
    pub fn new(workers: usize) -> Self {
        let (jobs, job_rx) = unbounded::<PathBuf>();
        let (results_tx, results) = unbounded();
        for index in 0..workers.max(1) {
            let job_rx = job_rx.clone();
            let results_tx = results_tx.clone();
            std::thread::Builder::new()
                .name(format!("hotview-thumb-{index}"))
                .spawn(move || {
                    while let Ok(path) = job_rx.recv() {
                        // A decoder may panic on corrupt input; keep the pool alive.
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                            || load_thumbnail(&path),
                        ))
                        .unwrap_or_else(|_| Err("decoder crashed on this file".to_string()));
                        if results_tx.send((path, result)).is_err() {
                            break;
                        }
                    }
                })
                .ok();
        }
        Self {
            jobs,
            results,
            pending: HashSet::new(),
        }
    }

    /// Queue a thumbnail if it is not already in flight.
    pub fn request(&mut self, path: &Path) {
        if self.pending.contains(path) || self.pending.len() > 4096 {
            return;
        }
        self.pending.insert(path.to_path_buf());
        if self.jobs.send(path.to_path_buf()).is_err() {
            self.pending.remove(path);
        }
    }

    /// Collect finished thumbnails (non-blocking).
    pub fn poll(&mut self) -> Vec<(PathBuf, Result<RgbaFrame, String>)> {
        let mut done = Vec::new();
        while let Ok((path, result)) = self.results.try_recv() {
            self.pending.remove(&path);
            done.push((path, result));
        }
        done
    }

    pub fn pending_is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

fn load_thumbnail(path: &Path) -> Result<RgbaFrame, String> {
    if hotview_core::is_video_path(path) {
        let mut decoder = FfmpegVideoDecoder::open(path).map_err(|err| err.to_string())?;
        for _ in 0..120 {
            match decoder.next_frame() {
                Ok(DecodeOutcome::Frame(frame)) => {
                    return thumbnail(frame.to_rgba(), THUMB_SIZE).map_err(|err| err.to_string())
                }
                Ok(DecodeOutcome::Pending) => continue,
                Ok(DecodeOutcome::Eos) => return Err("video has no frames".into()),
                Err(err) => return Err(err.to_string()),
            }
        }
        Err("no decodable frame near the start".into())
    } else {
        thumbnail_file(path, THUMB_SIZE).map_err(|err| err.to_string())
    }
}
