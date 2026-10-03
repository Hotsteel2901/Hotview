//! The eframe/egui application: gallery grid + full viewer.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use egui::{
    Align2, Color32, ColorImage, CornerRadius, Context, FontId, Id, Key, Margin, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Vec2,
    ViewportCommand,
};
use hotview_core::{
    decode_file_scaled, is_media_path, is_video_path, mime_from_path, probe_image_dimensions,
    RgbaFrame,
};

use crate::i18n::{Lang, LanguagePref, Strings, strings};
use crate::player::{Player, PlayerEvent};
use crate::settings::{
    DesktopSettings, SortBy, file_modified_secs, format_unix_date, natural_cmp, normalize_path,
    reveal_in_file_manager,
};
use crate::theme::{
    ACCENT_AMBER, ACCENT_COOL, ACCENT_CORAL, ThemeMode, ViewerBg, install_system_fonts,
    paint_checkerboard,
};
use crate::thumbs::ThumbnailLoader;

const MAX_VISIBLE_ITEMS: usize = 5000;
const MAX_CACHED_TEXTURES: usize = 1500;
const EVICT_BATCH: usize = 128;
const SPEEDS: [f32; 5] = [0.5, 1.0, 1.25, 1.5, 2.0];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaFilter {
    All,
    Photos,
    Videos,
}

#[derive(Clone)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub kind: MediaKind,
    pub size: u64,
    pub modified_secs: u64,
}

#[derive(PartialEq, Eq)]
enum Screen {
    Gallery,
    Viewer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ZoomMode {
    Contain,
    Cover,
    Custom,
}

struct DecodedImage {
    frame: RgbaFrame,
    original_dims: Option<(u32, u32)>,
}

struct Viewer {
    /// Index into `visible_indices` (and therefore `self.visible_item(self.viewer.index)`).
    index: usize,
    raw_image: Option<RgbaFrame>,
    original_dims: Option<(u32, u32)>,
    image: Option<(TextureHandle, (u32, u32))>,
    image_error: Option<String>,
    image_rx: Option<Receiver<Result<DecodedImage, String>>>,
    player: Option<Player>,
    video_texture: Option<TextureHandle>,
    video_size: (u32, u32),
    video_fps: Option<f64>,
    video_codec: Option<String>,
    video_rot: u8,
    video_flip_h: bool,
    video_flip_v: bool,
    playing: bool,
    position_us: i64,
    duration_us: i64,
    scrub_us: Option<i64>,
    looping: bool,
    has_audio: bool,
    speed: f32,
    pending_frame: Option<RgbaFrame>,
    zoom: f32,
    zoom_mode: ZoomMode,
    pan: Vec2,
}

impl Viewer {
    fn new(looping: bool) -> Self {
        Self {
            index: 0,
            raw_image: None,
            original_dims: None,
            image: None,
            image_error: None,
            image_rx: None,
            player: None,
            video_texture: None,
            video_size: (0, 0),
            video_fps: None,
            video_codec: None,
            video_rot: 0,
            video_flip_h: false,
            video_flip_v: false,
            playing: false,
            position_us: 0,
            duration_us: 0,
            scrub_us: None,
            looping,
            has_audio: false,
            speed: 1.0,
            pending_frame: None,
            zoom: 1.0,
            zoom_mode: ZoomMode::Contain,
            pan: Vec2::ZERO,
        }
    }
}

pub struct HotviewApp {
    ctx: Context,
    has_cjk_font: bool,
    settings: DesktopSettings,
    all_items: Vec<Item>,
    visible_indices: Vec<usize>,
    selected_gallery_index: usize,
    gallery_columns: usize,
    folder: Option<PathBuf>,
    search_query: String,
    focus_search: bool,
    filter: MediaFilter,
    screen: Screen,
    viewer: Viewer,
    thumbs: ThumbnailLoader,
    textures: HashMap<PathBuf, TextureHandle>,
    texture_order: VecDeque<PathBuf>,
    failures: HashMap<PathBuf, String>,
    toast: Option<(String, Instant)>,
    slideshow: Option<Instant>,
    is_fullscreen: bool,
    show_settings_modal: bool,
    show_shortcuts_modal: bool,
    last_title: String,
}

impl HotviewApp {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Vec<PathBuf>) -> Self {
        let has_cjk_font = install_system_fonts(&cc.egui_ctx);
        let settings = DesktopSettings::load();
        settings.theme.apply(&cc.egui_ctx);

        let workers = std::thread::available_parallelism()
            .map(|value| value.get().min(8))
            .unwrap_or(4);
        let looping = settings.loop_video;
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            has_cjk_font,
            settings,
            all_items: Vec::new(),
            visible_indices: Vec::new(),
            selected_gallery_index: 0,
            gallery_columns: 4,
            folder: None,
            search_query: String::new(),
            focus_search: false,
            filter: MediaFilter::All,
            screen: Screen::Gallery,
            viewer: Viewer::new(looping),
            thumbs: ThumbnailLoader::new(workers),
            textures: HashMap::new(),
            texture_order: VecDeque::new(),
            failures: HashMap::new(),
            toast: None,
            slideshow: None,
            is_fullscreen: false,
            show_settings_modal: false,
            show_shortcuts_modal: false,
            last_title: String::new(),
        };
        if !args.is_empty() {
            app.open_paths(args);
        }
        app
    }

    fn lang(&self) -> Lang {
        self.settings.language.resolve(self.has_cjk_font)
    }

    fn tr(&self) -> &'static Strings {
        strings(self.lang())
    }

    fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    fn visible_len(&self) -> usize {
        self.visible_indices.len()
    }

    fn visible_item(&self, visible_idx: usize) -> Option<&Item> {
        let raw_idx = *self.visible_indices.get(visible_idx)?;
        self.all_items.get(raw_idx)
    }

    fn update_window_title(&mut self) {
        let title = match self.screen {
            Screen::Viewer => {
                if let Some(item) = self.visible_item(self.viewer.index) {
                    format!(
                        "{} ({}/{}) — Hotview",
                        item.name,
                        self.viewer.index + 1,
                        self.visible_len()
                    )
                } else {
                    "Hotview".to_string()
                }
            }
            Screen::Gallery => {
                if let Some(folder) = &self.folder {
                    let folder_name = folder
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| folder.display().to_string());
                    if self.all_items.is_empty() {
                        format!("{folder_name} — Hotview")
                    } else {
                        format!("{folder_name} ({}) — Hotview", self.visible_len())
                    }
                } else {
                    "Hotview".to_string()
                }
            }
        };
        if title != self.last_title {
            self.last_title = title.clone();
            self.ctx.send_viewport_cmd(ViewportCommand::Title(title));
        }
    }

    // ---------------------------------------------------------------- opening

    fn open_paths(&mut self, raw_paths: Vec<PathBuf>) {
        let paths: Vec<PathBuf> = raw_paths.iter().map(|p| normalize_path(p)).collect();
        let mut files = Vec::new();
        let mut explicit_file: Option<PathBuf> = None;
        let mut had_directory = false;
        let mut target_folder: Option<PathBuf> = None;

        for path in &paths {
            if path.is_file() && is_media_path(path) {
                explicit_file.get_or_insert_with(|| path.clone());
            }
            if path.is_dir() {
                had_directory = true;
                target_folder = Some(path.clone());
                match std::fs::read_dir(path) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            let candidate = normalize_path(&entry.path());
                            if candidate.is_file() && is_media_path(&candidate) {
                                files.push(candidate);
                            }
                        }
                    }
                    Err(err) => self.toast(format!("Cannot read folder: {err}")),
                }
            } else if path.is_file() && is_media_path(path) {
                if target_folder.is_none() {
                    target_folder = path.parent().map(Path::to_path_buf);
                }
                files.push(path.clone());
            }
        }

        // When a single media file is opened (e.g. `hotview photo.jpg`, single
        // file dialog pick, or drag-and-drop), also load its sibling media files
        // from the parent directory so Prev/Next and Gallery work seamlessly.
        if !had_directory && files.len() == 1 {
            if let Some(parent) = &target_folder {
                if let Ok(entries) = std::fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let candidate = normalize_path(&entry.path());
                        if candidate.is_file() && is_media_path(&candidate) {
                            files.push(candidate);
                        }
                    }
                }
            }
        }

        if files.is_empty() {
            if !self.all_items.is_empty() {
                return;
            }
            self.toast(self.tr().no_supported_media);
            return;
        }

        // Always stop any active video playback / slideshow before switching lists.
        self.viewer.player = None;
        self.slideshow = None;

        if let Some(folder) = target_folder {
            self.folder = Some(folder.clone());
            self.settings.push_recent(folder);
        }

        files.sort_by(|a, b| {
            let na = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let nb = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
            natural_cmp(na, nb)
        });
        files.dedup();
        files.truncate(MAX_VISIBLE_ITEMS);

        self.all_items = files
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
                let modified_secs = file_modified_secs(&path);
                Item {
                    path,
                    name,
                    kind,
                    size,
                    modified_secs,
                }
            })
            .collect();

        self.thumbs.clear();
        self.textures.clear();
        self.texture_order.clear();
        self.failures.clear();
        self.search_query.clear();
        self.filter = MediaFilter::All;
        self.rebuild_visible_items(explicit_file.as_deref());

        self.screen = Screen::Gallery;
        self.toast(format!("{} items", self.visible_len()));

        // `hotview photo.jpg` opens that file straight away; folders stay in the
        // gallery.
        if !had_directory {
            if let Some(file) = explicit_file {
                if let Some(vis_idx) = self
                    .visible_indices
                    .iter()
                    .position(|&raw| self.all_items[raw].path == file)
                {
                    self.open_viewer(vis_idx);
                }
            }
        }
        self.update_window_title();
    }

    fn rebuild_visible_items(&mut self, preserve_path: Option<&Path>) {
        let preserve = preserve_path.map(Path::to_path_buf).or_else(|| {
            self.visible_item(self.viewer.index)
                .map(|item| item.path.clone())
        });

        let query = self.search_query.trim().to_lowercase();
        let mut indices: Vec<usize> = self
            .all_items
            .iter()
            .enumerate()
            .filter(|(_, item)| match self.filter {
                MediaFilter::All => true,
                MediaFilter::Photos => item.kind == MediaKind::Image,
                MediaFilter::Videos => item.kind == MediaKind::Video,
            })
            .filter(|(_, item)| {
                query.is_empty() || item.name.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();

        let sort_by = self.settings.sort_by;
        let sort_asc = self.settings.sort_asc;
        indices.sort_by(|&a, &b| {
            let ia = &self.all_items[a];
            let ib = &self.all_items[b];
            let cmp = match sort_by {
                SortBy::Name => natural_cmp(&ia.name, &ib.name),
                SortBy::Date => ia
                    .modified_secs
                    .cmp(&ib.modified_secs)
                    .then_with(|| natural_cmp(&ia.name, &ib.name)),
                SortBy::Size => ia
                    .size
                    .cmp(&ib.size)
                    .then_with(|| natural_cmp(&ia.name, &ib.name)),
                SortBy::Kind => {
                    let ka = matches!(ia.kind, MediaKind::Video) as u8;
                    let kb = matches!(ib.kind, MediaKind::Video) as u8;
                    ka.cmp(&kb).then_with(|| natural_cmp(&ia.name, &ib.name))
                }
            };
            if sort_asc { cmp } else { cmp.reverse() }
        });

        self.visible_indices = indices;

        if let Some(target) = preserve {
            if let Some(pos) = self
                .visible_indices
                .iter()
                .position(|&raw| self.all_items[raw].path == target)
            {
                self.viewer.index = pos;
                self.selected_gallery_index = pos;
                return;
            }
        }
        self.viewer.index = 0;
        self.selected_gallery_index = 0;
    }

    fn return_to_gallery(&mut self) {
        self.selected_gallery_index = self.viewer.index.min(self.visible_len().saturating_sub(1));
        self.screen = Screen::Gallery;
        self.viewer.player = None;
        self.slideshow = None;
        self.update_window_title();
    }

    fn open_viewer(&mut self, visible_index: usize) {
        if visible_index >= self.visible_len() {
            return;
        }
        self.selected_gallery_index = visible_index;
        self.screen = Screen::Viewer;
        self.viewer = Viewer::new(self.settings.loop_video);
        self.viewer.index = visible_index;
        self.start_viewer_item(self.settings.auto_play);
        self.update_window_title();
    }

    fn start_viewer_item(&mut self, auto_play_video: bool) {
        let Some(item) = self.visible_item(self.viewer.index).cloned() else {
            return;
        };
        self.viewer.raw_image = None;
        self.viewer.original_dims = None;
        self.viewer.image = None;
        self.viewer.image_error = None;
        self.viewer.image_rx = None;
        self.viewer.player = None;
        self.viewer.video_texture = None;
        self.viewer.video_size = (0, 0);
        self.viewer.video_fps = None;
        self.viewer.video_codec = None;
        self.viewer.video_rot = 0;
        self.viewer.video_flip_h = false;
        self.viewer.video_flip_v = false;
        self.viewer.playing = false;
        self.viewer.position_us = 0;
        self.viewer.duration_us = 0;
        self.viewer.scrub_us = None;
        self.viewer.pending_frame = None;
        self.viewer.zoom = 1.0;
        self.viewer.zoom_mode = ZoomMode::Contain;
        self.viewer.pan = Vec2::ZERO;

        match item.kind {
            MediaKind::Image => {
                let path = item.path.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("hotview-image".into())
                    .spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let original_dims = probe_image_dimensions(&path);
                            decode_file_scaled(&path, 8192)
                                .map(|frame| DecodedImage {
                                    frame,
                                    original_dims,
                                })
                                .map_err(|err| err.to_string())
                        }))
                        .unwrap_or_else(|_| Err("decoder crashed on this file".to_string()));
                        let _ = tx.send(result);
                    })
                    .ok();
                self.viewer.image_rx = Some(rx);
            }
            MediaKind::Video => match Player::open(item.path.clone()) {
                Ok(player) => {
                    self.viewer.video_size = (player.width, player.height);
                    self.viewer.original_dims = Some((player.width, player.height));
                    self.viewer.video_fps = player.fps;
                    self.viewer.video_codec = Some(player.codec.clone());
                    self.viewer.duration_us = player.duration_us;
                    self.viewer.has_audio = player.has_audio;
                    player.set_looping(self.viewer.looping);
                    player.set_volume(self.settings.volume);
                    player.set_muted(self.settings.muted);
                    player.set_speed(self.viewer.speed);
                    if auto_play_video {
                        player.play();
                    }
                    self.viewer.player = Some(player);
                }
                Err(err) => {
                    self.viewer.image_error = Some(err);
                }
            },
        }
    }

    fn step(&mut self, delta: i64) {
        let count = self.visible_len() as i64;
        if count <= 0 {
            return;
        }
        let next = (self.viewer.index as i64 + delta).rem_euclid(count) as usize;
        self.viewer.index = next;
        self.selected_gallery_index = next;
        self.start_viewer_item(self.settings.auto_play);
        self.update_window_title();
    }

    // ------------------------------------------------- transformations & copy

    fn rotate_viewer(&mut self, clockwise: bool) {
        if let Some(frame) = &self.viewer.raw_image {
            let rotated = if clockwise {
                frame.rotate_90_cw()
            } else {
                frame.rotate_90_ccw()
            };
            self.upload_viewer_image(rotated);
            self.viewer.zoom_mode = ZoomMode::Contain;
            self.viewer.pan = Vec2::ZERO;
        } else if self.viewer.player.is_some() {
            self.viewer.video_rot = if clockwise {
                (self.viewer.video_rot + 1) % 4
            } else {
                (self.viewer.video_rot + 3) % 4
            };
            self.viewer.zoom_mode = ZoomMode::Contain;
            self.viewer.pan = Vec2::ZERO;
        }
    }

    fn flip_viewer(&mut self, horizontal: bool) {
        if let Some(frame) = &self.viewer.raw_image {
            let flipped = if horizontal {
                frame.flip_horizontal()
            } else {
                frame.flip_vertical()
            };
            self.upload_viewer_image(flipped);
        } else if self.viewer.player.is_some() {
            if horizontal {
                self.viewer.video_flip_h = !self.viewer.video_flip_h;
            } else {
                self.viewer.video_flip_v = !self.viewer.video_flip_v;
            }
        }
    }

    fn upload_viewer_image(&mut self, frame: RgbaFrame) {
        let dims = (frame.width, frame.height);
        let image = ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &frame.data,
        );
        let texture = self
            .ctx
            .load_texture("viewer-image", image, TextureOptions::LINEAR);
        self.viewer.image = Some((texture, dims));
        self.viewer.raw_image = Some(frame);
    }

    fn copy_current_frame_to_clipboard(&mut self) {
        if let Some(frame) = &self.viewer.raw_image {
            let image = ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.data,
            );
            self.ctx.copy_image(image);
            self.toast(self.tr().copied_image);
        }
    }

    fn copy_current_path_to_clipboard(&mut self) {
        if let Some(item) = self.visible_item(self.viewer.index) {
            let path_str = item.path.display().to_string();
            self.ctx.copy_text(path_str);
            self.toast(self.tr().copied_path);
        }
    }

    fn toggle_fullscreen(&mut self) {
        self.is_fullscreen = !self.is_fullscreen;
        self.ctx
            .send_viewport_cmd(ViewportCommand::Fullscreen(self.is_fullscreen));
    }

    // ---------------------------------------------------------------- updates

    fn poll_thumbnails(&mut self) {
        for (path, result) in self.thumbs.poll() {
            match result {
                Ok(frame) => {
                    if self.textures.len() >= MAX_CACHED_TEXTURES {
                        for _ in 0..EVICT_BATCH {
                            if let Some(oldest) = self.texture_order.pop_front() {
                                self.textures.remove(&oldest);
                            } else {
                                break;
                            }
                        }
                    }
                    let image = ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.data,
                    );
                    let texture = self.ctx.load_texture(
                        path.to_string_lossy(),
                        image,
                        TextureOptions::LINEAR,
                    );
                    if !self.textures.contains_key(&path) {
                        self.texture_order.push_back(path.clone());
                    }
                    self.textures.insert(path, texture);
                }
                Err(err) => {
                    self.failures.insert(path, err);
                }
            }
        }
        if !self.thumbs.pending_is_empty() {
            self.ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    fn poll_viewer(&mut self) {
        // Image loading.
        if let Some(rx) = &self.viewer.image_rx {
            match rx.try_recv() {
                Ok(Ok(decoded)) => {
                    self.viewer.original_dims = decoded
                        .original_dims
                        .or(Some((decoded.frame.width, decoded.frame.height)));
                    self.upload_viewer_image(decoded.frame);
                    self.viewer.image_rx = None;
                    self.viewer.zoom_mode = ZoomMode::Contain;
                    self.viewer.pan = Vec2::ZERO;
                }
                Ok(Err(err)) => {
                    self.viewer.image_error = Some(err);
                    self.viewer.image_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.ctx.request_repaint_after(Duration::from_millis(30));
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
                    if self.viewer.scrub_us.is_none() {
                        self.viewer.position_us = position_us;
                    }
                    if duration_us > 0 {
                        self.viewer.duration_us = duration_us;
                    }
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
        if let Some(mut frame) = self.viewer.pending_frame.take() {
            frame = match self.viewer.video_rot % 4 {
                1 => frame.rotate_90_cw(),
                2 => frame.rotate_180(),
                3 => frame.rotate_90_ccw(),
                _ => frame,
            };
            if self.viewer.video_flip_h {
                frame = frame.flip_horizontal();
            }
            if self.viewer.video_flip_v {
                frame = frame.flip_vertical();
            }
            self.upload_video_frame(&frame);
        }
        if self.viewer.player.is_some() {
            self.ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn upload_video_frame(&mut self, frame: &RgbaFrame) {
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
        self.viewer.raw_image = Some(frame.clone());
    }

    // ------------------------------------------------------------------- ui

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let tr = self.tr();
        egui::Panel::top(Id::new("hotview-toolbar")).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // Brand badge / back to gallery
                if self.screen == Screen::Viewer {
                    if ui
                        .button(RichText::new(format!("⬅ {}", tr.back_to_gallery)).strong())
                        .on_hover_text("Esc / Backspace")
                        .clicked()
                    {
                        self.return_to_gallery();
                    }
                    ui.separator();
                } else {
                    ui.colored_label(ACCENT_AMBER, RichText::new("●").size(15.0));
                    ui.label(RichText::new("Hotview").strong().size(15.0));
                    ui.separator();
                }

                if ui
                    .button(format!("📄 {}", tr.open_files))
                    .on_hover_text("Ctrl+O")
                    .clicked()
                {
                    self.pick_files_dialog();
                }
                if ui
                    .button(format!("📁 {}", tr.open_folder))
                    .on_hover_text("Ctrl+Shift+O")
                    .clicked()
                {
                    self.pick_folder_dialog();
                }

                let in_viewer = self.screen == Screen::Viewer;
                if in_viewer {
                    ui.separator();
                    let has_items = self.visible_len() > 1;
                    if ui
                        .add_enabled(has_items, egui::Button::new(format!("◀ {}", tr.prev)))
                        .on_hover_text("←")
                        .clicked()
                    {
                        self.step(-1);
                    }
                    if !self.visible_indices.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "{} / {}",
                                self.viewer.index + 1,
                                self.visible_len()
                            ))
                            .strong(),
                        );
                    }
                    if ui
                        .add_enabled(has_items, egui::Button::new(format!("{} ▶", tr.next)))
                        .on_hover_text("→")
                        .clicked()
                    {
                        self.step(1);
                    }

                    ui.separator();
                    if ui
                        .selectable_label(self.viewer.zoom_mode == ZoomMode::Contain, tr.fit)
                        .on_hover_text("F / 0")
                        .clicked()
                    {
                        self.viewer.zoom_mode = ZoomMode::Contain;
                        self.viewer.pan = Vec2::ZERO;
                    }
                    if ui
                        .selectable_label(self.viewer.zoom_mode == ZoomMode::Cover, tr.fill)
                        .clicked()
                    {
                        self.viewer.zoom_mode = ZoomMode::Cover;
                        self.viewer.pan = Vec2::ZERO;
                    }
                    if ui
                        .selectable_label(
                            self.viewer.zoom_mode == ZoomMode::Custom
                                && (self.viewer.zoom - 1.0).abs() < 0.01,
                            tr.actual_size,
                        )
                        .on_hover_text("1")
                        .clicked()
                    {
                        self.viewer.zoom_mode = ZoomMode::Custom;
                        self.viewer.zoom = 1.0;
                        self.viewer.pan = Vec2::ZERO;
                    }
                    if ui.button("−").on_hover_text("Zoom out (-)").clicked() {
                        self.viewer.zoom_mode = ZoomMode::Custom;
                        self.viewer.zoom = (self.viewer.zoom / 1.25).clamp(0.02, 40.0);
                    }
                    if ui.button("+").on_hover_text("Zoom in (+)").clicked() {
                        self.viewer.zoom_mode = ZoomMode::Custom;
                        self.viewer.zoom = (self.viewer.zoom * 1.25).clamp(0.02, 40.0);
                    }

                    ui.separator();
                    if ui
                        .button("⟲")
                        .on_hover_text(format!("{} (Shift+R)", tr.rotate_ccw))
                        .clicked()
                    {
                        self.rotate_viewer(false);
                    }
                    if ui
                        .button("⟳")
                        .on_hover_text(format!("{} (R)", tr.rotate_cw))
                        .clicked()
                    {
                        self.rotate_viewer(true);
                    }
                    if ui
                        .button("⇄")
                        .on_hover_text(format!("{} (H)", tr.flip_h))
                        .clicked()
                    {
                        self.flip_viewer(true);
                    }
                    if ui
                        .button("⇅")
                        .on_hover_text(format!("{} (V)", tr.flip_v))
                        .clicked()
                    {
                        self.flip_viewer(false);
                    }

                    let bg_tip = match self.settings.viewer_bg {
                        ViewerBg::Dark => tr.bg_dark,
                        ViewerBg::Checkerboard => tr.bg_checker,
                        ViewerBg::Light => tr.bg_light,
                    };
                    if ui
                        .button("▦")
                        .on_hover_text(format!("{bg_tip} (B)"))
                        .clicked()
                    {
                        self.settings.viewer_bg = self.settings.viewer_bg.next();
                        self.settings.save();
                    }

                    ui.separator();
                    let slideshow = self.slideshow.is_some();
                    if ui
                        .selectable_label(slideshow, format!("▶ {}", tr.slideshow))
                        .on_hover_text("S")
                        .clicked()
                    {
                        self.slideshow = if slideshow {
                            None
                        } else {
                            Some(Instant::now())
                        };
                    }
                    if ui
                        .selectable_label(self.settings.show_filmstrip, tr.filmstrip)
                        .on_hover_text("T")
                        .clicked()
                    {
                        self.settings.show_filmstrip = !self.settings.show_filmstrip;
                        self.settings.save();
                    }
                    if ui
                        .selectable_label(self.settings.show_info, format!("ℹ {}", tr.info))
                        .on_hover_text("I")
                        .clicked()
                    {
                        self.settings.show_info = !self.settings.show_info;
                        self.settings.save();
                    }
                } else if !self.all_items.is_empty() {
                    // Gallery filter, search & sort controls
                    ui.separator();
                    let total_count = self.all_items.len();
                    let photo_count = self
                        .all_items
                        .iter()
                        .filter(|i| i.kind == MediaKind::Image)
                        .count();
                    let video_count = total_count.saturating_sub(photo_count);

                    if ui
                        .selectable_label(
                            self.filter == MediaFilter::All,
                            format!("{} ({total_count})", tr.filter_all),
                        )
                        .clicked()
                    {
                        self.filter = MediaFilter::All;
                        self.rebuild_visible_items(None);
                    }
                    if ui
                        .selectable_label(
                            self.filter == MediaFilter::Photos,
                            format!("{} ({photo_count})", tr.filter_photos),
                        )
                        .clicked()
                    {
                        self.filter = MediaFilter::Photos;
                        self.rebuild_visible_items(None);
                    }
                    if ui
                        .selectable_label(
                            self.filter == MediaFilter::Videos,
                            format!("{} ({video_count})", tr.filter_videos),
                        )
                        .clicked()
                    {
                        self.filter = MediaFilter::Videos;
                        self.rebuild_visible_items(None);
                    }

                    ui.separator();
                    let lang = self.lang();
                    for mode in SortBy::ALL {
                        let active = self.settings.sort_by == mode;
                        let arrow = if active {
                            if self.settings.sort_asc { " ↑" } else { " ↓" }
                        } else {
                            ""
                        };
                        if ui
                            .selectable_label(active, format!("{}{arrow}", mode.label(lang)))
                            .clicked()
                        {
                            if active {
                                self.settings.sort_asc = !self.settings.sort_asc;
                            } else {
                                self.settings.sort_by = mode;
                                self.settings.sort_asc = true;
                            }
                            self.settings.save();
                            self.rebuild_visible_items(None);
                        }
                    }

                    ui.separator();
                    let search_resp = ui.add(
                        TextEdit::singleline(&mut self.search_query)
                            .hint_text(format!("🔍 {}", tr.search_hint))
                            .desired_width(150.0),
                    );
                    if self.focus_search {
                        search_resp.request_focus();
                        self.focus_search = false;
                    }
                    if search_resp.changed() {
                        self.rebuild_visible_items(None);
                    }
                    if !self.search_query.is_empty() && ui.button("✕").clicked() {
                        self.search_query.clear();
                        self.rebuild_visible_items(None);
                    }
                }

                // Right-aligned global buttons + toast
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("⛶")
                        .on_hover_text(format!("{} (F11)", tr.fullscreen))
                        .clicked()
                    {
                        self.toggle_fullscreen();
                    }
                    if ui
                        .button("?")
                        .on_hover_text(format!("{} (F1)", tr.shortcuts))
                        .clicked()
                    {
                        self.show_shortcuts_modal = !self.show_shortcuts_modal;
                    }
                    if ui
                        .button("⚙")
                        .on_hover_text(tr.settings)
                        .clicked()
                    {
                        self.show_settings_modal = !self.show_settings_modal;
                    }
                    if ui
                        .button(self.settings.theme.icon())
                        .on_hover_text(tr.theme_label)
                        .clicked()
                    {
                        self.settings.theme = self.settings.theme.next();
                        self.settings.theme.apply(&self.ctx);
                        self.settings.save();
                    }

                    if self.screen == Screen::Gallery && !self.all_items.is_empty() {
                        let mut grid = self.settings.grid_size;
                        if ui
                            .add(
                                egui::Slider::new(&mut grid, 104.0..=280.0)
                                    .show_value(false)
                                    .trailing_fill(true),
                            )
                            .on_hover_text(tr.grid_size)
                            .changed()
                        {
                            self.settings.grid_size = grid;
                            self.settings.save();
                        }
                    }

                    if let Some((message, at)) = &self.toast {
                        if at.elapsed() < Duration::from_secs(4) {
                            ui.colored_label(ACCENT_COOL, message);
                        }
                    }
                });
            });
            ui.add_space(4.0);
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let tr = self.tr();
        egui::Panel::bottom(Id::new("hotview-status")).show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                let current_idx = match self.screen {
                    Screen::Viewer => self.viewer.index,
                    Screen::Gallery => self.selected_gallery_index,
                };
                if let Some(item) = self.visible_item(current_idx).cloned() {
                    ui.label(RichText::new(&item.name).strong());
                    ui.separator();
                    ui.label(human_size(item.size));
                    if self.screen == Screen::Viewer {
                        if let Some((w, h)) = self.viewer.original_dims {
                            if w > 0 && h > 0 {
                                ui.separator();
                                ui.label(format!("{w} × {h}"));
                            }
                        }
                    }
                } else if let Some(folder) = &self.folder {
                    ui.label(folder.display().to_string());
                }

                let is_video = self
                    .visible_item(self.viewer.index)
                    .map(|item| item.kind == MediaKind::Video)
                    .unwrap_or(false);
                if is_video && self.screen == Screen::Viewer {
                    ui.separator();
                    let play_label = if self.viewer.playing { "⏸" } else { "▶" };
                    if ui
                        .button(play_label)
                        .on_hover_text("Space")
                        .clicked()
                    {
                        if let Some(player) = &self.viewer.player {
                            if self.viewer.playing {
                                player.pause();
                            } else {
                                player.play();
                            }
                        }
                    }
                    if ui.button("⏪ 5s").on_hover_text("[ / J").clicked() {
                        self.seek_relative_us(-5_000_000);
                    }
                    if ui.button("5s ⏩").on_hover_text("] / L").clicked() {
                        self.seek_relative_us(5_000_000);
                    }

                    let duration = (self.viewer.duration_us.max(1) as f64) / 1e6;
                    let active_us = self.viewer.scrub_us.unwrap_or(self.viewer.position_us);
                    let mut position = (active_us as f64 / 1e6).clamp(0.0, duration);
                    let slider = ui.add(
                        egui::Slider::new(&mut position, 0.0..=duration)
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    if slider.changed() || slider.dragged() {
                        self.viewer.scrub_us = Some((position * 1e6) as i64);
                    }
                    if !slider.dragged() {
                        if let Some(target_us) = self.viewer.scrub_us.take() {
                            self.viewer.position_us = target_us;
                            if let Some(player) = &self.viewer.player {
                                player.seek(target_us);
                            }
                        }
                    }

                    let shown_us = self.viewer.scrub_us.unwrap_or(self.viewer.position_us);
                    ui.label(format!(
                        "{} / {}",
                        format_time(shown_us),
                        format_time(self.viewer.duration_us)
                    ));

                    let mut looping = self.viewer.looping;
                    if ui.checkbox(&mut looping, tr.loop_video).changed() {
                        self.viewer.looping = looping;
                        self.settings.loop_video = looping;
                        self.settings.save();
                        if let Some(player) = &self.viewer.player {
                            player.set_looping(looping);
                        }
                    }

                    // Speed cycle button
                    let speed_label = format!("{:.2}×", self.viewer.speed)
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string()
                        + if self.viewer.speed.fract() == 0.0 {
                            ".0×"
                        } else {
                            "×"
                        };
                    if ui
                        .button(speed_label)
                        .on_hover_text(tr.speed)
                        .clicked()
                    {
                        let next_idx = SPEEDS
                            .iter()
                            .position(|&s| (s - self.viewer.speed).abs() < 0.05)
                            .map(|i| (i + 1) % SPEEDS.len())
                            .unwrap_or(1);
                        self.viewer.speed = SPEEDS[next_idx];
                        if let Some(player) = &self.viewer.player {
                            player.set_speed(self.viewer.speed);
                        }
                    }

                    // Audio mute + volume slider
                    if self.viewer.has_audio {
                        let mute_icon = if self.settings.muted || self.settings.volume <= 0.001 {
                            "🔇"
                        } else if self.settings.volume < 0.5 {
                            "🔉"
                        } else {
                            "🔊"
                        };
                        if ui
                            .button(mute_icon)
                            .on_hover_text(format!(
                                "{} (M)",
                                if self.settings.muted {
                                    tr.unmute
                                } else {
                                    tr.mute
                                }
                            ))
                            .clicked()
                        {
                            self.settings.muted = !self.settings.muted;
                            self.settings.save();
                            if let Some(player) = &self.viewer.player {
                                player.set_muted(self.settings.muted);
                            }
                        }
                        let mut vol = if self.settings.muted {
                            0.0
                        } else {
                            self.settings.volume
                        };
                        let vol_slider = ui.add_sized(
                            [72.0, 18.0],
                            egui::Slider::new(&mut vol, 0.0..=1.0)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        if vol_slider.changed() {
                            self.settings.volume = vol;
                            self.settings.muted = vol <= 0.001;
                            if let Some(player) = &self.viewer.player {
                                player.set_volume(self.settings.volume);
                                player.set_muted(self.settings.muted);
                            }
                        }
                        if vol_slider.drag_stopped() {
                            self.settings.save();
                        }
                    } else {
                        ui.weak(format!("({})", tr.no_audio));
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.screen == Screen::Viewer {
                        ui.label(format!("{}%", (self.viewer.zoom * 100.0).round() as i64));
                    } else if !self.all_items.is_empty() {
                        ui.label(format!(
                            "{} / {}",
                            self.visible_len(),
                            self.all_items.len()
                        ));
                    }
                });
            });
            ui.add_space(3.0);
        });
    }

    fn seek_relative_us(&mut self, delta_us: i64) {
        if let Some(player) = &self.viewer.player {
            let max_us = self.viewer.duration_us.max(0);
            let next = if max_us > 0 {
                (self.viewer.position_us + delta_us).clamp(0, max_us)
            } else {
                (self.viewer.position_us + delta_us).max(0)
            };
            self.viewer.position_us = next;
            player.seek(next);
        }
    }

    fn pick_files_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Media", &media_extensions());
        if let Some(folder) = &self.folder {
            dialog = dialog.set_directory(folder);
        }
        if let Some(files) = dialog.pick_files() {
            self.open_paths(files);
        }
    }

    fn pick_folder_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(folder) = &self.folder {
            dialog = dialog.set_directory(folder);
        }
        if let Some(folder) = dialog.pick_folder() {
            self.open_paths(vec![folder]);
        }
    }

    // -------------------------------------------------------------- gallery

    fn gallery_ui(&mut self, ui: &mut egui::Ui) {
        let tr = self.tr();
        if self.all_items.is_empty() {
            self.empty_welcome_ui(ui);
            return;
        }
        if self.visible_indices.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(tr.no_matches).size(16.0));
            });
            return;
        }

        // Ctrl + Mouse wheel adjusts thumbnail size in Gallery.
        let ctrl_scroll = ui.input(|i| {
            if i.modifiers.ctrl || i.modifiers.command {
                i.smooth_scroll_delta.y
            } else {
                0.0
            }
        });
        if ctrl_scroll.abs() > 0.1 {
            self.settings.grid_size =
                (self.settings.grid_size + ctrl_scroll * 0.25).clamp(104.0, 280.0);
        }

        let cell = self.settings.grid_size;
        let visible = self.visible_indices.clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                let avail_w = (ui.available_width() - 16.0).max(cell);
                let columns = ((avail_w + 10.0) / (cell + 10.0)).floor().max(2.0) as usize;
                self.gallery_columns = columns;

                for (row_idx, chunk) in visible.chunks(columns).enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        for (col_idx, &raw_idx) in chunk.iter().enumerate() {
                            let vis_idx = row_idx * columns + col_idx;
                            if let Some(item) = self.all_items.get(raw_idx).cloned() {
                                self.thumbnail_cell(ui, &item, vis_idx, cell);
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
            });
    }

    fn empty_welcome_ui(&mut self, ui: &mut egui::Ui) {
        let tr = self.tr();
        let recents = self.settings.recent_folders.clone();
        let mut open_recent: Option<PathBuf> = None;
        let mut clear_recents = false;

        ui.vertical_centered(|ui| {
            let avail_h = ui.available_height();
            ui.add_space((avail_h * 0.18).max(24.0));

            egui::Frame::window(ui.style())
                .inner_margin(Margin::symmetric(36, 28))
                .corner_radius(CornerRadius::same(18))
                .show(ui, |ui| {
                    ui.set_max_width(480.0);
                    ui.vertical_centered(|ui| {
                        ui.colored_label(ACCENT_AMBER, RichText::new("◉").size(40.0));
                        ui.add_space(6.0);
                        ui.label(RichText::new("Hotview").size(24.0).strong());
                        ui.add_space(4.0);
                        ui.label(RichText::new(tr.empty_title).size(16.0));
                        ui.add_space(2.0);
                        ui.weak(tr.empty_subtitle);
                        ui.add_space(18.0);

                        ui.horizontal(|ui| {
                            let total_w = ui.available_width();
                            ui.add_space(((total_w - 260.0) * 0.5).max(0.0));
                            if ui
                                .add_sized(
                                    [124.0, 34.0],
                                    egui::Button::new(
                                        RichText::new(format!("📁 {}", tr.open_folder)).strong(),
                                    ),
                                )
                                .clicked()
                            {
                                self.pick_folder_dialog();
                            }
                            if ui
                                .add_sized(
                                    [124.0, 34.0],
                                    egui::Button::new(format!("📄 {}", tr.open_files)),
                                )
                                .clicked()
                            {
                                self.pick_files_dialog();
                            }
                        });

                        if !recents.is_empty() {
                            ui.add_space(18.0);
                            ui.separator();
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.weak(tr.recent_folders);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button(tr.clear_recent).clicked() {
                                            clear_recents = true;
                                        }
                                    },
                                );
                            });
                            ui.add_space(4.0);
                            for folder in &recents {
                                let label = folder.display().to_string();
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 26.0],
                                        egui::Button::new(format!("📂 {label}")),
                                    )
                                    .clicked()
                                {
                                    open_recent = Some(folder.clone());
                                }
                            }
                        }
                    });
                });
        });

        if clear_recents {
            self.settings.recent_folders.clear();
            self.settings.save();
        }
        if let Some(folder) = open_recent {
            self.open_paths(vec![folder]);
        }
    }

    fn thumbnail_cell(&mut self, ui: &mut egui::Ui, item: &Item, vis_idx: usize, size: f32) {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());

        // Viewport culling: only decode & paint thumbnails that are visible or near the viewport.
        let near_viewport = ui.clip_rect().expand(380.0).intersects(rect);
        if !near_viewport {
            return;
        }

        let painter = ui.painter().with_clip_rect(ui.clip_rect());
        let dark = ui.visuals().dark_mode;
        let card_bg = if dark {
            Color32::from_rgb(26, 26, 38)
        } else {
            Color32::from_rgb(232, 226, 216)
        };
        painter.rect_filled(rect, 10.0, card_bg);

        if let Some(texture) = self.textures.get(&item.path) {
            painter.image(
                texture.id(),
                rect,
                cover_uv(rect, texture.size()),
                Color32::WHITE,
            );
        } else if self.failures.contains_key(&item.path) {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "⚠",
                FontId::proportional(22.0),
                ACCENT_CORAL,
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
                Pos2::new(rect.right() - 36.0, rect.top() + 8.0),
                Vec2::new(28.0, 18.0),
            );
            painter.rect_filled(badge, 5.0, Color32::from_black_alpha(185));
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                "▶",
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }

        let selected = self.selected_gallery_index == vis_idx;
        if response.hovered() || selected {
            // Subtle bottom caption bar on hover/selection
            let cap_h = 24.0;
            let cap_rect = Rect::from_min_max(
                Pos2::new(rect.left(), rect.bottom() - cap_h),
                rect.max,
            );
            painter.rect_filled(cap_rect, 6.0, Color32::from_black_alpha(170));
            let short_name = truncate_middle(&item.name, (size / 7.5).max(10.0) as usize);
            painter.text(
                cap_rect.center(),
                Align2::CENTER_CENTER,
                short_name,
                FontId::proportional(11.5),
                Color32::WHITE,
            );

            let stroke_col = if response.hovered() {
                ACCENT_AMBER
            } else {
                ACCENT_COOL
            };
            painter.rect_stroke(
                rect,
                10.0,
                Stroke::new(2.0, stroke_col),
                StrokeKind::Inside,
            );
        }

        let tr = self.tr();
        let item_path = item.path.clone();
        response.context_menu(|ui| {
            if ui.button(format!("🔍 {}", tr.info)).clicked() {
                self.settings.show_info = true;
                self.open_viewer(vis_idx);
                ui.close();
            }
            if ui.button(format!("📂 {}", tr.reveal_in_folder)).clicked() {
                reveal_in_file_manager(&item_path);
                ui.close();
            }
            if ui.button(format!("📋 {}", tr.copy_path)).clicked() {
                self.ctx.copy_text(item_path.display().to_string());
                self.toast(tr.copied_path);
                ui.close();
            }
        });

        let response = response.on_hover_text(format!("{}\n{}", item.name, human_size(item.size)));
        if response.clicked() {
            self.open_viewer(vis_idx);
        }
    }

    // --------------------------------------------------------------- viewer

    fn viewer_ui(&mut self, ui: &mut egui::Ui) {
        if self.settings.show_info {
            self.info_panel_ui(ui);
        }
        if self.settings.show_filmstrip && self.visible_len() > 1 {
            self.filmstrip_ui(ui);
        }

        let tr = self.tr();
        let available = ui.available_size();
        let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
        let rect = response.rect;

        let bg_fill = match self.settings.viewer_bg {
            ViewerBg::Dark => Color32::from_rgb(8, 8, 12),
            ViewerBg::Checkerboard => Color32::from_rgb(14, 14, 20),
            ViewerBg::Light => Color32::from_rgb(244, 240, 234),
        };
        painter.rect_filled(rect, 0.0, bg_fill);

        let (texture_id, size) = if let Some((texture, size)) = &self.viewer.image {
            (Some(texture.id()), *size)
        } else if let Some(texture) = &self.viewer.video_texture {
            (Some(texture.id()), self.viewer.video_size)
        } else {
            (None, self.viewer.video_size)
        };

        let has_valid_size =
            size.0 > 0 && size.1 > 0 && rect.width() > 1.0 && rect.height() > 1.0;
        let fit_scale = if has_valid_size {
            (rect.width() / size.0 as f32).min(rect.height() / size.1 as f32)
        } else {
            1.0
        };
        let fill_scale = if has_valid_size {
            (rect.width() / size.0 as f32).max(rect.height() / size.1 as f32)
        } else {
            1.0
        };

        if has_valid_size {
            match self.viewer.zoom_mode {
                ZoomMode::Contain => {
                    self.viewer.zoom = fit_scale;
                    self.viewer.pan = Vec2::ZERO;
                }
                ZoomMode::Cover => {
                    self.viewer.zoom = fill_scale;
                    self.viewer.pan = Vec2::ZERO;
                }
                ZoomMode::Custom => {}
            }
        }

        // Wheel zoom (around the cursor).
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01 && has_valid_size {
                let cursor = response.hover_pos().unwrap_or(rect.center());
                let old_zoom = self.viewer.zoom.max(1e-4);
                let new_zoom = (old_zoom * (scroll * 0.002).exp())
                    .clamp(fit_scale * 0.05, fit_scale.max(1.0) * 40.0);
                let center = rect.center() + self.viewer.pan;
                self.viewer.pan =
                    center + (cursor - center) * (new_zoom / old_zoom) - rect.center();
                self.viewer.zoom = new_zoom;
                self.viewer.zoom_mode = ZoomMode::Custom;
            }
        }
        if response.dragged() {
            self.viewer.pan += response.drag_delta();
        }
        if response.middle_clicked() {
            self.viewer.zoom_mode = ZoomMode::Contain;
            self.viewer.pan = Vec2::ZERO;
        }

        let image_size = Vec2::new(size.0 as f32, size.1 as f32);
        let display = image_size * self.viewer.zoom;
        let max_x = ((display.x - rect.width()) / 2.0).max(0.0);
        let max_y = ((display.y - rect.height()) / 2.0).max(0.0);
        self.viewer.pan.x = self.viewer.pan.x.clamp(-max_x, max_x);
        self.viewer.pan.y = self.viewer.pan.y.clamp(-max_y, max_y);

        if let Some(texture_id) = texture_id {
            let image_rect = Rect::from_center_size(rect.center() + self.viewer.pan, display);
            if self.settings.viewer_bg == ViewerBg::Checkerboard {
                paint_checkerboard(&painter, image_rect.intersect(rect), true);
            }
            painter.image(
                texture_id,
                image_rect,
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else if let Some(error) = &self.viewer.image_error {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                error,
                FontId::proportional(15.0),
                ACCENT_CORAL,
            );
        } else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tr.loading,
                FontId::proportional(16.0),
                Color32::GRAY,
            );
        }

        // Floating Prev / Next edge navigation buttons when hovering in the viewer.
        let mut nav_clicked = false;
        if self.visible_len() > 1 && response.hovered() {
            let btn_size = Vec2::new(40.0, 64.0);
            let left_rect = Rect::from_center_size(
                Pos2::new(rect.left() + 28.0, rect.center().y),
                btn_size,
            );
            let right_rect = Rect::from_center_size(
                Pos2::new(rect.right() - 28.0, rect.center().y),
                btn_size,
            );
            let pointer_pos = response.hover_pos();
            let over_left = pointer_pos.map_or(false, |p| left_rect.contains(p));
            let over_right = pointer_pos.map_or(false, |p| right_rect.contains(p));

            for (btn_rect, over, symbol) in [
                (left_rect, over_left, "‹"),
                (right_rect, over_right, "›"),
            ] {
                let alpha = if over { 210 } else { 110 };
                painter.rect_filled(btn_rect, 12.0, Color32::from_black_alpha(alpha));
                if over {
                    painter.rect_stroke(
                        btn_rect,
                        12.0,
                        Stroke::new(1.5, ACCENT_AMBER),
                        StrokeKind::Inside,
                    );
                }
                painter.text(
                    btn_rect.center(),
                    Align2::CENTER_CENTER,
                    symbol,
                    FontId::proportional(28.0),
                    Color32::WHITE,
                );
            }

            if response.clicked() {
                if over_left {
                    self.step(-1);
                    nav_clicked = true;
                } else if over_right {
                    self.step(1);
                    nav_clicked = true;
                }
            }
        }

        // Double-click toggles Fit <-> 2x zoom around cursor.
        if !nav_clicked && response.double_clicked() && has_valid_size {
            if self.viewer.zoom_mode == ZoomMode::Contain
                && (self.viewer.zoom - fit_scale).abs() < 1e-3
            {
                let cursor = response.interact_pointer_pos().unwrap_or(rect.center());
                let old_zoom = self.viewer.zoom.max(1e-4);
                let target_zoom = (fit_scale * 2.25).max(1.0);
                let center = rect.center() + self.viewer.pan;
                self.viewer.pan =
                    center + (cursor - center) * (target_zoom / old_zoom) - rect.center();
                self.viewer.zoom = target_zoom;
                self.viewer.zoom_mode = ZoomMode::Custom;
            } else {
                self.viewer.zoom_mode = ZoomMode::Contain;
                self.viewer.pan = Vec2::ZERO;
            }
        } else if !nav_clicked && response.clicked() && self.viewer.player.is_some() {
            // Single click on video toggles play/pause.
            if let Some(player) = &self.viewer.player {
                if self.viewer.playing {
                    player.pause();
                } else {
                    player.play();
                }
            }
        }

        // Context menu in Viewer
        response.context_menu(|ui| {
            if ui.button(format!("📋 {}", tr.copy_image)).clicked() {
                self.copy_current_frame_to_clipboard();
                ui.close();
            }
            if ui.button(format!("📋 {}", tr.copy_path)).clicked() {
                self.copy_current_path_to_clipboard();
                ui.close();
            }
            if let Some(item) = self.visible_item(self.viewer.index).cloned() {
                if ui.button(format!("📂 {}", tr.reveal_in_folder)).clicked() {
                    reveal_in_file_manager(&item.path);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(format!("⟲ {}", tr.rotate_ccw)).clicked() {
                self.rotate_viewer(false);
                ui.close();
            }
            if ui.button(format!("⟳ {}", tr.rotate_cw)).clicked() {
                self.rotate_viewer(true);
                ui.close();
            }
            if ui.button(format!("ℹ {}", tr.info)).clicked() {
                self.settings.show_info = !self.settings.show_info;
                self.settings.save();
                ui.close();
            }
        });
    }

    fn filmstrip_ui(&mut self, ui: &mut egui::Ui) {
        let count = self.visible_len();
        if count == 0 {
            return;
        }
        let thumb_size = 54.0_f32;
        let window_radius = 12usize;
        let start = self.viewer.index.saturating_sub(window_radius);
        let end = (self.viewer.index + window_radius + 1).min(count);
        let mut clicked_index: Option<usize> = None;

        egui::Panel::bottom(Id::new("hotview-filmstrip")).show(ui, |ui| {
            ui.add_space(4.0);
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        for vis_idx in start..end {
                            let Some(item) = self.visible_item(vis_idx).cloned() else {
                                continue;
                            };
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::splat(thumb_size), Sense::click());
                            let painter = ui.painter().with_clip_rect(ui.clip_rect());
                            painter.rect_filled(rect, 6.0, Color32::from_gray(28));
                            if let Some(tex) = self.textures.get(&item.path) {
                                painter.image(
                                    tex.id(),
                                    rect,
                                    cover_uv(rect, tex.size()),
                                    Color32::WHITE,
                                );
                            } else {
                                self.thumbs.request(&item.path);
                            }
                            let is_active = vis_idx == self.viewer.index;
                            if is_active || resp.hovered() {
                                let col = if is_active { ACCENT_AMBER } else { ACCENT_COOL };
                                painter.rect_stroke(
                                    rect,
                                    6.0,
                                    Stroke::new(if is_active { 2.2 } else { 1.5 }, col),
                                    StrokeKind::Inside,
                                );
                            }
                            if resp.on_hover_text(&item.name).clicked() {
                                clicked_index = Some(vis_idx);
                            }
                        }
                    });
                });
            ui.add_space(4.0);
        });

        if let Some(idx) = clicked_index {
            if idx != self.viewer.index {
                self.viewer.index = idx;
                self.selected_gallery_index = idx;
                self.start_viewer_item(self.settings.auto_play);
                self.update_window_title();
            }
        }
    }

    fn info_panel_ui(&mut self, ui: &mut egui::Ui) {
        let tr = self.tr();
        let Some(item) = self.visible_item(self.viewer.index).cloned() else {
            return;
        };

        egui::Panel::right(Id::new("hotview-info-panel"))
            .default_size(270.0)
            .min_size(220.0)
            .max_size(360.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr.info_title).size(15.5).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            self.settings.show_info = false;
                            self.settings.save();
                        }
                    });
                });
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    info_row(ui, tr.info_name, &item.name);
                    info_row(
                        ui,
                        tr.info_type,
                        match item.kind {
                            MediaKind::Image => tr.info_image,
                            MediaKind::Video => tr.info_video,
                        },
                    );
                    if let Some((w, h)) = self.viewer.original_dims {
                        if w > 0 && h > 0 {
                            let mp = (w as f64 * h as f64) / 1_000_000.0;
                            let res = if mp >= 0.1 {
                                format!("{w} × {h} ({mp:.1} MP)")
                            } else {
                                format!("{w} × {h}")
                            };
                            info_row(ui, tr.info_resolution, &res);
                        }
                    }
                    info_row(
                        ui,
                        tr.info_size,
                        &format!("{} ({} B)", human_size(item.size), item.size),
                    );
                    if let Some(mime) = mime_from_path(&item.path) {
                        info_row(ui, tr.info_format, &mime);
                    }
                    if item.kind == MediaKind::Video {
                        if self.viewer.duration_us > 0 {
                            info_row(
                                ui,
                                tr.info_duration,
                                &format_time(self.viewer.duration_us),
                            );
                        }
                        if let Some(fps) = self.viewer.video_fps {
                            info_row(ui, tr.info_fps, &format!("{fps:.2} fps"));
                        }
                        if let Some(codec) = &self.viewer.video_codec {
                            info_row(ui, tr.info_codec, codec);
                        }
                    }
                    info_row(
                        ui,
                        tr.info_modified,
                        &format_unix_date(item.modified_secs),
                    );
                    info_row(ui, tr.info_path, &item.path.display().to_string());

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(6.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::new(format!("📋 {}", tr.copy_image)),
                        )
                        .clicked()
                    {
                        self.copy_current_frame_to_clipboard();
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::new(format!("📋 {}", tr.copy_path)),
                        )
                        .clicked()
                    {
                        self.copy_current_path_to_clipboard();
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::new(format!("📂 {}", tr.reveal_in_folder)),
                        )
                        .clicked()
                    {
                        reveal_in_file_manager(&item.path);
                    }
                });
            });
    }

    fn modals_ui(&mut self, ctx: &Context) {
        let tr = self.tr();
        let lang = self.lang();

        if self.show_settings_modal {
            let mut open = self.show_settings_modal;
            egui::Window::new(format!("⚙ {}", tr.settings))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .show(ctx, |ui| {
                    ui.label(RichText::new(tr.theme_label).strong());
                    ui.horizontal(|ui| {
                        for (mode, label) in [
                            (ThemeMode::System, tr.theme_system),
                            (ThemeMode::Dark, tr.theme_dark),
                            (ThemeMode::Light, tr.theme_light),
                        ] {
                            if ui
                                .selectable_label(
                                    self.settings.theme == mode,
                                    format!("{} {label}", mode.icon()),
                                )
                                .clicked()
                            {
                                self.settings.theme = mode;
                                self.settings.theme.apply(ctx);
                                self.settings.save();
                            }
                        }
                    });

                    ui.add_space(8.0);
                    ui.label(RichText::new(tr.lang_label).strong());
                    ui.horizontal_wrapped(|ui| {
                        for pref in LanguagePref::ALL {
                            if ui
                                .selectable_label(
                                    self.settings.language == pref,
                                    pref.label(lang),
                                )
                                .clicked()
                            {
                                self.settings.language = pref;
                                self.settings.save();
                            }
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    if ui
                        .checkbox(&mut self.settings.auto_play, tr.auto_play_label)
                        .changed()
                    {
                        self.settings.save();
                    }
                    if ui
                        .checkbox(&mut self.settings.loop_video, tr.loop_video)
                        .changed()
                    {
                        self.viewer.looping = self.settings.loop_video;
                        if let Some(player) = &self.viewer.player {
                            player.set_looping(self.settings.loop_video);
                        }
                        self.settings.save();
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(tr.slideshow_interval_label);
                        let mut secs = self.settings.slideshow_secs;
                        if ui
                            .add(egui::Slider::new(&mut secs, 2..=30))
                            .changed()
                        {
                            self.settings.slideshow_secs = secs;
                            self.settings.save();
                        }
                    });
                });
            self.show_settings_modal = open;
        }

        if self.show_shortcuts_modal {
            let mut open = self.show_shortcuts_modal;
            egui::Window::new(format!("⌨ {}", tr.shortcuts))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .show(ctx, |ui| {
                    egui::Grid::new("hotview-shortcuts-grid")
                        .num_columns(2)
                        .spacing([20.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for (key, desc) in [
                                ("← / →", "Prev / Next media"),
                                ("Space", "Play / Pause video (or open in Gallery)"),
                                ("Esc / Backspace", "Back to Gallery / Exit fullscreen"),
                                ("F / 0", tr.fit),
                                ("1", tr.actual_size),
                                ("+ / - / Wheel", "Zoom in / out"),
                                ("Double-click", "Toggle Fit / 2× zoom"),
                                ("R / Shift+R", tr.rotate_cw),
                                ("H / V", tr.flip_h),
                                ("B", "Cycle backdrop (Dark / Checker / Light)"),
                                ("I", tr.info),
                                ("T", tr.filmstrip),
                                ("S", tr.slideshow),
                                ("M", tr.mute),
                                ("[ / ]", "Seek -5s / +5s"),
                                ("Ctrl+C", tr.copy_image),
                                ("Ctrl+F", tr.search_hint),
                                ("Ctrl+O", tr.open_files),
                                ("F11", tr.fullscreen),
                            ] {
                                ui.code(key);
                                ui.label(desc);
                                ui.end_row();
                            }
                        });
                });
            self.show_shortcuts_modal = open;
        }

        // Drag & drop visual overlay
        let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering_files {
            let screen_rect = ctx.input(|i| i.viewport_rect());
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                Id::new("hotview-drop-overlay"),
            ));
            painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(180));
            let inner = screen_rect.shrink(36.0);
            painter.rect_stroke(
                inner,
                18.0,
                Stroke::new(2.5, ACCENT_AMBER),
                StrokeKind::Inside,
            );
            painter.text(
                inner.center() - Vec2::new(0.0, 14.0),
                Align2::CENTER_CENTER,
                tr.drop_overlay_title,
                FontId::proportional(24.0),
                Color32::WHITE,
            );
            painter.text(
                inner.center() + Vec2::new(0.0, 18.0),
                Align2::CENTER_CENTER,
                tr.drop_overlay_subtitle,
                FontId::proportional(15.0),
                ACCENT_COOL,
            );
        }
    }
}

impl eframe::App for HotviewApp {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let text_editing = ctx.text_edit_focused();

        // Global shortcuts (work in both Gallery and Viewer).
        let (f11, f1, ctrl_o, ctrl_shift_o, ctrl_f, ctrl_c) = ctx.input(|i| {
            let cmd = i.modifiers.ctrl || i.modifiers.command;
            (
                i.key_pressed(Key::F11) || (i.modifiers.alt && i.key_pressed(Key::Enter)),
                i.key_pressed(Key::F1)
                    || (!text_editing && i.key_pressed(Key::Questionmark)),
                cmd && !i.modifiers.shift && i.key_pressed(Key::O),
                cmd && i.modifiers.shift && i.key_pressed(Key::O),
                cmd && i.key_pressed(Key::F),
                cmd && i.key_pressed(Key::C),
            )
        });

        if f11 {
            self.toggle_fullscreen();
        }
        if f1 {
            self.show_shortcuts_modal = !self.show_shortcuts_modal;
        }
        if ctrl_o {
            self.pick_files_dialog();
        }
        if ctrl_shift_o {
            self.pick_folder_dialog();
        }
        if ctrl_f && self.screen == Screen::Gallery {
            self.focus_search = true;
        }
        if ctrl_c && self.screen == Screen::Viewer && !text_editing {
            self.copy_current_frame_to_clipboard();
        }

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

        if text_editing {
            return;
        }

        match self.screen {
            Screen::Viewer => {
                let (
                    escape,
                    left,
                    right,
                    space,
                    fit,
                    one,
                    plus,
                    minus,
                    rot_cw,
                    rot_ccw,
                    flip_h,
                    flip_v,
                    bg_cycle,
                    info_toggle,
                    strip_toggle,
                    slide_toggle,
                    mute_toggle,
                    seek_back,
                    seek_fwd,
                ) = ctx.input(|i| {
                    let no_cmd = !i.modifiers.ctrl && !i.modifiers.command;
                    (
                        i.key_pressed(Key::Escape) || i.key_pressed(Key::Backspace),
                        i.key_pressed(Key::ArrowLeft),
                        i.key_pressed(Key::ArrowRight),
                        i.key_pressed(Key::Space),
                        no_cmd && (i.key_pressed(Key::F) || i.key_pressed(Key::Num0)),
                        no_cmd && i.key_pressed(Key::Num1),
                        no_cmd && (i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)),
                        no_cmd && i.key_pressed(Key::Minus),
                        no_cmd && !i.modifiers.shift && i.key_pressed(Key::R),
                        no_cmd && i.modifiers.shift && i.key_pressed(Key::R),
                        no_cmd && i.key_pressed(Key::H),
                        no_cmd && i.key_pressed(Key::V),
                        no_cmd && i.key_pressed(Key::B),
                        no_cmd && i.key_pressed(Key::I),
                        no_cmd && i.key_pressed(Key::T),
                        no_cmd && i.key_pressed(Key::S),
                        no_cmd && i.key_pressed(Key::M),
                        no_cmd && (i.key_pressed(Key::OpenBracket) || i.key_pressed(Key::J)),
                        no_cmd && (i.key_pressed(Key::CloseBracket) || i.key_pressed(Key::L)),
                    )
                });

                if escape {
                    if self.is_fullscreen {
                        self.toggle_fullscreen();
                    } else {
                        self.return_to_gallery();
                    }
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
                    self.viewer.zoom_mode = ZoomMode::Contain;
                    self.viewer.pan = Vec2::ZERO;
                } else if one {
                    self.viewer.zoom_mode = ZoomMode::Custom;
                    self.viewer.zoom = 1.0;
                    self.viewer.pan = Vec2::ZERO;
                } else if plus {
                    self.viewer.zoom_mode = ZoomMode::Custom;
                    self.viewer.zoom = (self.viewer.zoom * 1.25).clamp(0.02, 40.0);
                } else if minus {
                    self.viewer.zoom_mode = ZoomMode::Custom;
                    self.viewer.zoom = (self.viewer.zoom / 1.25).clamp(0.02, 40.0);
                } else if rot_cw {
                    self.rotate_viewer(true);
                } else if rot_ccw {
                    self.rotate_viewer(false);
                } else if flip_h {
                    self.flip_viewer(true);
                } else if flip_v {
                    self.flip_viewer(false);
                } else if bg_cycle {
                    self.settings.viewer_bg = self.settings.viewer_bg.next();
                    self.settings.save();
                } else if info_toggle {
                    self.settings.show_info = !self.settings.show_info;
                    self.settings.save();
                } else if strip_toggle {
                    self.settings.show_filmstrip = !self.settings.show_filmstrip;
                    self.settings.save();
                } else if slide_toggle {
                    self.slideshow = if self.slideshow.is_some() {
                        None
                    } else {
                        Some(Instant::now())
                    };
                } else if mute_toggle {
                    self.settings.muted = !self.settings.muted;
                    self.settings.save();
                    if let Some(player) = &self.viewer.player {
                        player.set_muted(self.settings.muted);
                    }
                } else if seek_back {
                    self.seek_relative_us(-5_000_000);
                } else if seek_fwd {
                    self.seek_relative_us(5_000_000);
                }

                if let Some(last) = self.slideshow {
                    let is_image = self
                        .visible_item(self.viewer.index)
                        .map(|item| item.kind == MediaKind::Image)
                        .unwrap_or(false);
                    let interval = Duration::from_secs(self.settings.slideshow_secs.max(2));
                    if is_image && last.elapsed() >= interval {
                        self.slideshow = Some(Instant::now());
                        self.step(1);
                    }
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
            }
            Screen::Gallery => {
                let count = self.visible_len();
                if count > 0 {
                    let cols = self.gallery_columns.max(1);
                    let (left, right, up, down, home, end, enter) = ctx.input(|i| {
                        (
                            i.key_pressed(Key::ArrowLeft),
                            i.key_pressed(Key::ArrowRight),
                            i.key_pressed(Key::ArrowUp),
                            i.key_pressed(Key::ArrowDown),
                            i.key_pressed(Key::Home),
                            i.key_pressed(Key::End),
                            i.key_pressed(Key::Enter) || i.key_pressed(Key::Space),
                        )
                    });
                    if left {
                        self.selected_gallery_index =
                            self.selected_gallery_index.saturating_sub(1);
                    } else if right {
                        self.selected_gallery_index =
                            (self.selected_gallery_index + 1).min(count - 1);
                    } else if up {
                        self.selected_gallery_index =
                            self.selected_gallery_index.saturating_sub(cols);
                    } else if down {
                        self.selected_gallery_index =
                            (self.selected_gallery_index + cols).min(count - 1);
                    } else if home {
                        self.selected_gallery_index = 0;
                    } else if end {
                        self.selected_gallery_index = count - 1;
                    } else if enter {
                        self.open_viewer(self.selected_gallery_index);
                    }
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.toolbar(ui);
        self.status_bar(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| match self.screen {
                Screen::Gallery => self.gallery_ui(ui),
                Screen::Viewer => self.viewer_ui(ui),
            });
        self.modals_ui(&ctx);
    }
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.weak(label);
        ui.label(value);
    });
    ui.add_space(4.0);
}

fn truncate_middle(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars || max_chars < 6 {
        return text.to_string();
    }
    let head = (max_chars - 1) / 2;
    let tail = max_chars - 1 - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

fn cover_uv(rect: Rect, texture_size: [usize; 2]) -> Rect {
    let (tw, th) = (
        texture_size[0].max(1) as f32,
        texture_size[1].max(1) as f32,
    );
    let scale = (rect.width() / tw).max(rect.height() / th);
    let (uw, uh) = (rect.width() / (tw * scale), rect.height() / (th * scale));
    Rect::from_center_size(Pos2::new(0.5, 0.5), Vec2::new(uw, uh))
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
