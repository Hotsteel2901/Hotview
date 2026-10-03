//! Hotview desktop: a cross-platform image & video viewer with a Rust core.

mod app;
mod player;
mod thumbs;

use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hotview")
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Hotview",
        options,
        Box::new(move |cc| Ok(Box::new(app::HotviewApp::new(cc, paths)))),
    )
}
