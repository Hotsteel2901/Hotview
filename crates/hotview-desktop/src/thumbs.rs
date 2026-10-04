//! Background thumbnail loader: a small worker pool that decodes images and
//! first video frames off the UI thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver, Sender};
use hotview_core::video::ffmpeg::first_frame_scaled;
use hotview_core::{thumbnail_file, RgbaFrame};

/// Longest edge of a generated thumbnail.
pub const THUMB_SIZE: u32 = 420;

pub struct ThumbnailLoader {
    jobs: Sender<(u64, PathBuf)>,
    results: Receiver<(u64, PathBuf, Result<RgbaFrame, String>)>,
    pending: HashSet<PathBuf>,
    generation: Arc<AtomicU64>,
}

impl ThumbnailLoader {
    pub fn new(workers: usize) -> Self {
        let (jobs, job_rx) = unbounded::<(u64, PathBuf)>();
        let (results_tx, results) = unbounded();
        let generation = Arc::new(AtomicU64::new(1));

        for index in 0..workers.max(1) {
            let job_rx = job_rx.clone();
            let results_tx = results_tx.clone();
            let generation = generation.clone();
            std::thread::Builder::new()
                .name(format!("hotview-thumb-{index}"))
                .spawn(move || {
                    while let Ok((job_gen, path)) = job_rx.recv() {
                        if job_gen != generation.load(Ordering::Relaxed) {
                            continue;
                        }
                        // A decoder may panic on corrupt input; keep the pool alive.
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                            || load_thumbnail(&path),
                        ))
                        .unwrap_or_else(|_| Err("decoder crashed on this file".to_string()));
                        if job_gen != generation.load(Ordering::Relaxed) {
                            continue;
                        }
                        if results_tx.send((job_gen, path, result)).is_err() {
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
            generation,
        }
    }

    /// Cancel all queued thumbnail jobs from the previous folder.
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.pending.clear();
        while self.results.try_recv().is_ok() {}
    }

    /// Queue a thumbnail if it is not already in flight.
    pub fn request(&mut self, path: &Path) {
        if self.pending.contains(path) || self.pending.len() > 4096 {
            return;
        }
        let current_gen = self.generation.load(Ordering::Relaxed);
        self.pending.insert(path.to_path_buf());
        if self.jobs.send((current_gen, path.to_path_buf())).is_err() {
            self.pending.remove(path);
        }
    }

    /// Collect finished thumbnails (non-blocking).
    pub fn poll(&mut self) -> Vec<(PathBuf, Result<RgbaFrame, String>)> {
        let current_gen = self.generation.load(Ordering::Relaxed);
        let mut done = Vec::new();
        while let Ok((job_gen, path, result)) = self.results.try_recv() {
            if job_gen == current_gen {
                self.pending.remove(&path);
                done.push((path, result));
            }
        }
        done
    }

    pub fn pending_is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

fn load_thumbnail(path: &Path) -> Result<RgbaFrame, String> {
    if hotview_core::is_video_path(path) {
        first_frame_scaled(path, THUMB_SIZE).map_err(|err| err.to_string())
    } else {
        thumbnail_file(path, THUMB_SIZE).map_err(|err| err.to_string())
    }
}
