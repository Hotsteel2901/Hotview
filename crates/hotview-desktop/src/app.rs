//! The eframe/egui application: gallery grid + full viewer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use egui::{
    Align2, Color32, ColorImage, Context, FontId, Key, Rect, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Vec2,
};
use hotview_core::{decode_file_scaled, is_media_path, is_video_path, RgbaFrame};

use crate::player::{Player, PlayerEvent};
use crate::thumbs::ThumbnailLoader;

const SLIDESHOW_INTERVAL: Duration = Duration::from_secs(5);
const MAX_VISIBLE_ITEMS: usize = 5000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Clone)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub kind: MediaKind,
    pub size: u64,
}

#[derive(PartialEq, Eq)]
enum Screen {
    Gallery,
    Viewer,
}

struct Viewer {
    index: usize,
    image: Option<(TextureHandle, (u32, u32))>,
    image_error: Option<String>,
    image_rx: Option<Receiver<Result<RgbaFrame, String>>>,
    player: Option<Player>,
    video_texture: Option<TextureHandle>,
    video_size: (u32, u32),
    playing: bool,
    position_us: i64,
    duration_us: i64,
    looping: bool,
    has_audio: bool,
    pending_frame: Option<RgbaFrame>,
    zoom: f32,
    pan: Vec2,
    fit_pending: bool,
}

impl Viewer {
    fn new() -> Self {
        Self {
            index: 0,
            image: None,
            image_error: None,
            image_rx: None,
            player: None,
            video_texture: None,
            video_size: (0, 0),
            playing: false,
            position_us: 0,
            duration_us: 0,
            looping: false,
            has_audio: false,
            pending_frame: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit_pending: true,
        }
    }
}

pub struct HotviewApp {
    ctx: Context,
    items: Vec<Item>,
    folder: Option<PathBuf>,
    screen: Screen,
    viewer: Viewer,
    thumbs: ThumbnailLoader,
    textures: HashMap<PathBuf, TextureHandle>,
    failures: HashMap<PathBuf, String>,
    toast: Option<(String, Instant)>,
    slideshow: Option<Instant>,
}

impl HotviewApp {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Vec<PathBuf>) -> Self {
        let workers = std::thread::available_parallelism()
            .map(|value| value.get().min(8))
            .unwrap_or(4);
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            items: Vec::new(),
            folder: None,
            screen: Screen::Gallery,
            viewer: Viewer::new(),
            thumbs: ThumbnailLoader::new(workers),
            textures: HashMap::new(),
            failures: HashMap::new(),
            toast: None,
            slideshow: None,
        };
        if !args.is_empty() {
            app.open_paths(args);
        }
        app
    }

    fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    // ---------------------------------------------------------------- opening

    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        let mut files = Vec::new();
        let mut explicit_file: Option<PathBuf> = None;
        let mut had_directory = false;
        for path in paths {
            if path.is_file() {
                explicit_file.get_or_insert_with(|| path.clone());
            }
            if path.is_dir() {
                had_directory = true;
                match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        self.folder = Some(path.clone());
                        for entry in entries.flatten() {
                            let candidate = entry.path();
                            if candidate.is_file() && is_media_path(&candidate) {
                                files.push(candidate);
                            }
                        }
                    }
                    Err(err) => self.toast(format!("Cannot read folder: {err}")),
                }
            } else if path.is_file() && is_media_path(&path) {
                if self.folder.is_none() {
                    self.folder = path.parent().map(Path::to_path_buf);
                }
                files.push(path);
            }
        }
        if files.is_empty() {
            if !self.items.is_empty() {
                return;
            }
            self.toast("No supported media found");
            return;
        }

        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        files.dedup();
        files.truncate(MAX_VISIBLE_ITEMS);

        self.items = files
            .into_iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let kind = if is_video_path(&path) {
                    MediaKind::Video
                } else {
                    MediaKind::Image
                };
                let size = path.metadata().map(|meta| meta.len()).unwrap_or(0);
                Item {
                    path,
                    name,
                    kind,
                    size,
                }
            })
            .collect();
        self.textures.clear();
        self.failures.clear();
        self.screen = Screen::Gallery;
        self.toast(format!("{} items", self.items.len()));

        // `hotview photo.jpg` opens that file straight away; folders stay in the
        // gallery.
        if !had_directory {
            if let Some(file) = explicit_file {
                if let Some(index) = self.items.iter().position(|item| item.path == file) {
                    self.open_viewer(index);
                    if self.items[index].kind == MediaKind::Video {
                        if let Some(player) = &self.viewer.player {
                            player.play();
                        }
                    }
                }
            }
        }
    }

    fn open_viewer(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.screen = Screen::Viewer;
        self.viewer = Viewer::new();
        self.viewer.index = index;
        self.start_viewer_item();
    }

    fn start_viewer_item(&mut self) {
        let Some(item) = self.items.get(self.viewer.index).cloned() else {
            return;
        };
        self.viewer.image = None;
        self.viewer.image_error = None;
        self.viewer.image_rx = None;
        self.viewer.player = None;
        self.viewer.video_texture = None;
        self.viewer.video_size = (0, 0);
        self.viewer.playing = false;
        self.viewer.position_us = 0;
        self.viewer.duration_us = 0;
        self.viewer.pending_frame = None;
        self.viewer.zoom = 1.0;
        self.viewer.pan = Vec2::ZERO;
        self.viewer.fit_pending = true;

        match item.kind {
            MediaKind::Image => {
                let path = item.path.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("hotview-image".into())
                    .spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            decode_file_scaled(&path, 8192).map_err(|err| err.to_string())
                        }))
                        .unwrap_or_else(|_| Err("decoder crashed on this file".to_string()));
                        let _ = tx.send(result);
                    })
                    .ok();
                self.viewer.image_rx = Some(rx);
            }
            MediaKind::Video => match Player::open(item.path.clone()) {
                Ok(player) => {
                    self.viewer.duration_us = player.duration_us;
                    self.viewer.has_audio = player.has_audio;
                    self.viewer.player = Some(player);
                }
                Err(err) => {
                    self.viewer.image_error = Some(err);
                }
            },
        }
    }

    fn step(&mut self, delta: i64) {
        if self.items.is_empty() {
            return;
        }
        let count = self.items.len() as i64;
        let next = (self.viewer.index as i64 + delta).rem_euclid(count) as usize;
        self.viewer.index = next;
        self.start_viewer_item();
        // Auto-play videos when moving with the keyboard/slideshow.
        if self.items[next].kind == MediaKind::Video {
            if let Some(player) = &self.viewer.player {
                player.play();
            }
        }
    }

    // ---------------------------------------------------------------- updates

    fn poll_thumbnails(&mut self) {
        for (path, result) in self.thumbs.poll() {
            match result {
                Ok(frame) => {
                    if self.textures.len() > 800 {
                        self.textures.clear();
                    }
                    let image = ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.data,
                    );
                    let texture = self
                        .ctx
                        .load_texture(path.to_string_lossy(), image, TextureOptions::LINEAR);
                    self.textures.insert(path, texture);
                }
                Err(err) => {
                    self.failures.insert(path, err);
                }
            }
        }
        if !self.thumbs.pending_is_empty() {
            self.ctx
                .request_repaint_after(Duration::from_millis(200));
        }
    }

    fn poll_viewer(&mut self) {
        // Image loading.
        if let Some(rx) = &self.viewer.image_rx {
            match rx.try_recv() {
                Ok(Ok(frame)) => {
                    let image = ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.data,
                    );
                    let texture =
                        self.ctx
                            .load_texture("viewer-image", image, TextureOptions::LINEAR);
                    self.viewer.image = Some((texture, (frame.width, frame.height)));
                    self.viewer.image_rx = None;
                    self.viewer.fit_pending = true;
                }
                Ok(Err(err)) => {
                    self.viewer.image_error = Some(err);
                    self.viewer.image_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.ctx.request_repaint_after(Duration::from_millis(50));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.viewer.image_rx = None;
                }
            }
        }

        // Video events.
        let mut events = Vec::new();
        if let Some(player) = &self.viewer.player {
            while let Ok(event) = player.events.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                PlayerEvent::Frame(frame) => {
                    self.viewer.pending_frame = Some((*frame).clone());
                }
                PlayerEvent::State {
                    playing,
                    position_us,
                    duration_us,
                    has_audio,
                } => {
                    self.viewer.playing = playing;
                    self.viewer.position_us = position_us;
                    self.viewer.duration_us = duration_us;
                    self.viewer.has_audio = has_audio;
                }
                PlayerEvent::Ended => {
                    self.viewer.playing = false;
                }
                PlayerEvent::Error(err) => {
                    self.viewer.image_error = Some(err);
                }
            }
        }
        if let Some(frame) = self.viewer.pending_frame.take() {
            self.checkerboard_into_texture(&frame);
        }
        if self.viewer.player.is_some() {
            self.ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn checkerboard_into_texture(&mut self, frame: &RgbaFrame) {
        let image = ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &frame.data,
        );
        match &mut self.viewer.video_texture {
            Some(texture) => texture.set(image, TextureOptions::LINEAR),
            None => {
                self.viewer.video_texture = Some(self.ctx.load_texture(
                    "viewer-video",
                    image,
                    TextureOptions::LINEAR,
                ));
            }
        }
        self.viewer.video_size = (frame.width, frame.height);
    }

    // ------------------------------------------------------------------- ui

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top(egui::Id::new("hotview-toolbar")).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Open files…").clicked() {
                    if let Some(files) = rfd::FileDialog::new()
                        .add_filter("Media", &media_extensions())
                        .pick_files()
                    {
                        self.open_paths(files);
                    }
                }
                if ui.button("Open folder…").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.open_paths(vec![folder]);
                    }
                }
                ui.separator();
                let in_viewer = self.screen == Screen::Viewer;
                if ui
                    .add_enabled(in_viewer, egui::Button::new("◀ Prev"))
                    .clicked()
                {
                    self.step(-1);
                }
                if ui
                    .add_enabled(in_viewer, egui::Button::new("Next ▶"))
                    .clicked()
                {
                    self.step(1);
                }
                if in_viewer {
                    if ui.button("Fit").clicked() {
                        self.viewer.fit_pending = true;
                    }
                    if ui.button("1:1").clicked() {
                        self.viewer.zoom = 1.0;
                        self.viewer.pan = Vec2::ZERO;
                    }
                    let slideshow = self.slideshow.is_some();
                    if ui
                        .selectable_label(slideshow, "Slideshow")
                        .clicked()
                    {
                        self.slideshow = if slideshow {
                            None
                        } else {
                            Some(Instant::now())
                        };
                    }
                }
                ui.separator();
                if !self.items.is_empty() {
                    ui.label(format!(
                        "{} / {}",
                        self.viewer.index + 1,
                        self.items.len()
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((message, at)) = &self.toast {
                        if at.elapsed() < Duration::from_secs(4) {
                            ui.colored_label(Color32::from_rgb(150, 200, 255), message);
                        }
                    }
                });
            });
            ui.add_space(4.0);
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom(egui::Id::new("hotview-status")).show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if let Some(item) = self.items.get(self.viewer.index) {
                    ui.label(&item.name);
                    ui.separator();
                    ui.label(human_size(item.size));
                }
                let is_video = self
                    .items
                    .get(self.viewer.index)
                    .map(|item| item.kind == MediaKind::Video)
                    .unwrap_or(false);
                if is_video && self.screen == Screen::Viewer {
                    ui.separator();
                    let label = if self.viewer.playing { "⏸" } else { "▶" };
                    if ui.button(label).clicked() {
                        if let Some(player) = &self.viewer.player {
                            if self.viewer.playing {
                                player.pause();
                            } else {
                                player.play();
                            }
                        }
                    }
                    let mut looping = self.viewer.looping;
                    if ui.checkbox(&mut looping, "Loop").changed() {
                        self.viewer.looping = looping;
                        if let Some(player) = &self.viewer.player {
                            player.set_looping(looping);
                        }
                    }
                    let duration = self.viewer.duration_us as f64 / 1e6;
                    let mut position = self.viewer.position_us as f64 / 1e6;
                    let slider = ui.add(
                        egui::Slider::new(&mut position, 0.0..=duration.max(0.001))
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    if slider.drag_stopped() {
                        if let Some(player) = &self.viewer.player {
                            player.seek((position * 1e6) as i64);
                        }
                    }
                    ui.label(format!(
                        "{} / {}",
                        format_time(self.viewer.position_us),
                        format_time(self.viewer.duration_us)
                    ));
                    if !self.viewer.has_audio {
                        ui.label("(no audio)");
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{}%", (self.viewer.zoom * 100.0).round() as i64));
                });
            });
            ui.add_space(3.0);
        });
    }

    fn gallery_ui(&mut self, ui: &mut egui::Ui) {
        if self.items.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("Drop a folder of photos here, or use “Open folder…”");
            });
            return;
        }

        let items = std::mem::take(&mut self.items);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let cell = 168.0;
                let columns = ((ui.available_width() + 8.0) / (cell + 8.0))
                    .floor()
                    .max(2.0) as usize;
                for row in items.chunks(columns) {
                    ui.horizontal(|ui| {
                        for item in row {
                            self.thumbnail_cell(ui, item, cell);
                        }
                    });
                    ui.add_space(6.0);
                }
            });
        self.items = items;
    }

    fn thumbnail_cell(&mut self, ui: &mut egui::Ui, item: &Item, size: f32) {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
        let painter = ui.painter().with_clip_rect(ui.clip_rect());
        painter.rect_filled(rect, 8.0, Color32::from_gray(34));

        if let Some(texture) = self.textures.get(&item.path) {
            painter.image(
                texture.id(),
                rect,
                cover_uv(rect, texture.size()),
                Color32::WHITE,
            );
        } else if let Some(error) = self.failures.get(&item.path) {
            let _ = error;
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "⚠",
                FontId::proportional(22.0),
                Color32::from_rgb(220, 140, 120),
            );
        } else {
            self.thumbs.request(&item.path);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "…",
                FontId::proportional(20.0),
                Color32::GRAY,
            );
        }

        if item.kind == MediaKind::Video {
            let badge = Rect::from_min_size(
                egui::pos2(rect.right() - 36.0, rect.top() + 8.0),
                Vec2::new(28.0, 18.0),
            );
            painter.rect_filled(badge, 4.0, Color32::from_black_alpha(170));
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                "▶",
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }

        if response.hovered() {
            painter.rect_stroke(
                rect,
                8.0,
                Stroke::new(2.0, Color32::from_rgb(120, 180, 255)),
                StrokeKind::Inside,
            );
        }
        if response.clicked() {
            if let Some(index) = self.items.iter().position(|candidate| candidate.path == item.path)
            {
                self.open_viewer(index);
            }
        }
    }

    fn viewer_ui(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
        let rect = response.rect;

        painter.rect_filled(rect, 0.0, Color32::from_rgb(10, 10, 12));

        let (texture_id, size) = if let Some((texture, size)) = &self.viewer.image {
            (Some(texture.id()), *size)
        } else if let Some(texture) = &self.viewer.video_texture {
            (Some(texture.id()), self.viewer.video_size)
        } else {
            (None, (0, 0))
        };

        let fit_scale = if size.0 > 0 && size.1 > 0 && rect.width() > 0.0 && rect.height() > 0.0 {
            (rect.width() / size.0 as f32).min(rect.height() / size.1 as f32)
        } else {
            1.0
        };
        if self.viewer.fit_pending {
            self.viewer.fit_pending = false;
            self.viewer.zoom = fit_scale;
            self.viewer.pan = Vec2::ZERO;
        }

        // Wheel zoom (around the cursor).
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01 {
                let cursor = response.hover_pos().unwrap_or(rect.center());
                let old_zoom = self.viewer.zoom.max(1e-4);
                let new_zoom = (old_zoom * (scroll * 0.002).exp())
                    .clamp(fit_scale * 0.05, fit_scale.max(1.0) * 40.0);
                let center = rect.center() + self.viewer.pan;
                self.viewer.pan = center + (cursor - center) * (new_zoom / old_zoom) - rect.center();
                self.viewer.zoom = new_zoom;
            }
        }
        if response.dragged() {
            self.viewer.pan += response.drag_delta();
        }

        let image_size = Vec2::new(size.0 as f32, size.1 as f32);
        let display = image_size * self.viewer.zoom;
        let max_x = ((display.x - rect.width()) / 2.0).max(0.0);
        let max_y = ((display.y - rect.height()) / 2.0).max(0.0);
        self.viewer.pan.x = self.viewer.pan.x.clamp(-max_x, max_x);
        self.viewer.pan.y = self.viewer.pan.y.clamp(-max_y, max_y);

        if let Some(texture_id) = texture_id {
            let image_rect = Rect::from_center_size(rect.center() + self.viewer.pan, display);
            painter.image(
                texture_id,
                image_rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else if let Some(error) = &self.viewer.image_error {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                error,
                FontId::proportional(15.0),
                Color32::from_rgb(235, 140, 130),
            );
        } else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Loading…",
                FontId::proportional(16.0),
                Color32::GRAY,
            );
        }

        // Click on the left/right third to navigate, like every gallery app.
        if response.clicked() {
            if let Some(position) = response.interact_pointer_pos() {
                let third = rect.width() / 3.0;
                if position.x - rect.left() < third {
                    self.step(-1);
                } else if rect.right() - position.x < third {
                    self.step(1);
                }
            }
        }
    }
}

impl eframe::App for HotviewApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Keyboard.
        let (escape, left, right, space, fit, one) = ctx.input(|i| {
            (
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::F),
                i.key_pressed(Key::Num1),
            )
        });

        // Drag & drop.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|file| {
                    let path = file.path();
                    (!path.as_os_str().is_empty()).then(|| path.to_path_buf())
                })
                .collect()
        });
        if !dropped.is_empty() {
            self.open_paths(dropped);
        }

        self.poll_thumbnails();
        self.poll_viewer();

        if self.screen == Screen::Viewer {
            if escape {
                self.screen = Screen::Gallery;
                self.viewer.player = None;
            } else if left {
                self.step(-1);
            } else if right {
                self.step(1);
            } else if space {
                if let Some(player) = &self.viewer.player {
                    if self.viewer.playing {
                        player.pause();
                    } else {
                        player.play();
                    }
                }
            } else if fit {
                self.viewer.fit_pending = true;
            } else if one {
                self.viewer.zoom = 1.0;
                self.viewer.pan = Vec2::ZERO;
            }

            if let Some(last) = self.slideshow {
                let is_image = self
                    .items
                    .get(self.viewer.index)
                    .map(|item| item.kind == MediaKind::Image)
                    .unwrap_or(false);
                if is_image && last.elapsed() >= SLIDESHOW_INTERVAL {
                    self.slideshow = Some(Instant::now());
                    self.step(1);
                }
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.toolbar(ui);
        self.status_bar(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| match self.screen {
                Screen::Gallery => self.gallery_ui(ui),
                Screen::Viewer => self.viewer_ui(ui),
            });
    }
}

fn cover_uv(rect: Rect, texture_size: [usize; 2]) -> Rect {
    let (tw, th) = (texture_size[0].max(1) as f32, texture_size[1].max(1) as f32);
    let scale = (rect.width() / tw).max(rect.height() / th);
    let (uw, uh) = (
        rect.width() / (tw * scale),
        rect.height() / (th * scale),
    );
    Rect::from_center_size(egui::pos2(0.5, 0.5), Vec2::new(uw, uh))
}

fn media_extensions() -> Vec<&'static str> {
    let mut extensions = hotview_core::IMAGE_EXTENSIONS.to_vec();
    extensions.extend_from_slice(hotview_core::VIDEO_EXTENSIONS);
    extensions
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_time(micros: i64) -> String {
    let total = (micros.max(0) / 1_000_000) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
