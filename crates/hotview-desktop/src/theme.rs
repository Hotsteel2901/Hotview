//! "Hot-Steel" visual theme, procedural window icon, and cross-platform
//! system CJK/Unicode font discovery for Windows, macOS, and Linux.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, IconData, Pos2, Rect,
    Stroke, Style, Theme, ThemePreference, Vec2, Visuals,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

impl ThemeMode {
    #[allow(dead_code)]
    pub const ALL: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Dark, ThemeMode::Light];

    pub fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::System,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::System => "◐",
            Self::Dark => "☾",
            Self::Light => "☀",
        }
    }

    pub fn apply(self, ctx: &Context) {
        ctx.set_style_of(Theme::Dark, hotsteel_style(true));
        ctx.set_style_of(Theme::Light, hotsteel_style(false));
        let pref = match self {
            Self::System => ThemePreference::System,
            Self::Dark => ThemePreference::Dark,
            Self::Light => ThemePreference::Light,
        };
        ctx.set_theme(pref);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerBg {
    Dark,
    Checkerboard,
    Light,
}

impl ViewerBg {
    pub fn code(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Checkerboard => "checker",
            Self::Light => "light",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "checker" | "checkerboard" => Self::Checkerboard,
            "light" => Self::Light,
            _ => Self::Dark,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Dark => Self::Checkerboard,
            Self::Checkerboard => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

pub const ACCENT_AMBER: Color32 = Color32::from_rgb(255, 166, 61);
pub const ACCENT_CORAL: Color32 = Color32::from_rgb(255, 95, 86);
pub const ACCENT_COOL: Color32 = Color32::from_rgb(156, 202, 255);

fn hotsteel_style(dark: bool) -> Style {
    let mut style = Style::default();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.menu_margin = egui::Margin::same(8);

    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    let radius_sm = CornerRadius::same(8);
    let radius_md = CornerRadius::same(12);

    if dark {
        visuals.panel_fill = Color32::from_rgb(15, 15, 24);
        visuals.window_fill = Color32::from_rgb(19, 19, 30);
        visuals.extreme_bg_color = Color32::from_rgb(9, 9, 15);
        visuals.faint_bg_color = Color32::from_rgb(22, 22, 34);
        visuals.code_bg_color = Color32::from_rgb(26, 26, 40);
        visuals.window_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28));

        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(18, 18, 28);
        visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(18, 18, 28);
        visuals.widgets.noninteractive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 22));
        visuals.widgets.noninteractive.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(236, 233, 244));
        visuals.widgets.noninteractive.corner_radius = radius_sm;

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(28, 28, 42);
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(25, 25, 38);
        visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 20));
        visuals.widgets.inactive.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(230, 226, 240));
        visuals.widgets.inactive.corner_radius = radius_sm;

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(40, 38, 58);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(36, 34, 52);
        visuals.widgets.hovered.bg_stroke =
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 166, 61, 140));
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        visuals.widgets.hovered.corner_radius = radius_sm;

        visuals.widgets.active.bg_fill = Color32::from_rgb(58, 40, 56);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(52, 36, 50);
        visuals.widgets.active.bg_stroke = Stroke::new(1.2, ACCENT_AMBER);
        visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        visuals.widgets.active.corner_radius = radius_sm;

        visuals.widgets.open.bg_fill = Color32::from_rgb(36, 34, 52);
        visuals.widgets.open.weak_bg_fill = Color32::from_rgb(34, 32, 48);
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT_AMBER);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.widgets.open.corner_radius = radius_sm;

        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(255, 140, 66, 72);
        visuals.selection.stroke = Stroke::new(1.2, ACCENT_AMBER);
        visuals.hyperlink_color = ACCENT_COOL;
    } else {
        visuals.panel_fill = Color32::from_rgb(245, 240, 232);
        visuals.window_fill = Color32::from_rgb(252, 249, 244);
        visuals.extreme_bg_color = Color32::from_rgb(255, 255, 255);
        visuals.faint_bg_color = Color32::from_rgb(238, 232, 222);
        visuals.code_bg_color = Color32::from_rgb(235, 229, 219);
        visuals.window_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(34, 27, 46, 32));

        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(245, 240, 232);
        visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(245, 240, 232);
        visuals.widgets.noninteractive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(34, 27, 46, 26));
        visuals.widgets.noninteractive.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(36, 28, 48));
        visuals.widgets.noninteractive.corner_radius = radius_sm;

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(255, 255, 255);
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(236, 229, 218);
        visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(34, 27, 46, 28));
        visuals.widgets.inactive.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(38, 30, 52));
        visuals.widgets.inactive.corner_radius = radius_sm;

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(255, 246, 236);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(248, 236, 224);
        visuals.widgets.hovered.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(224, 90, 70));
        visuals.widgets.hovered.fg_stroke =
            Stroke::new(1.5, Color32::from_rgb(22, 16, 31));
        visuals.widgets.hovered.corner_radius = radius_sm;

        visuals.widgets.active.bg_fill = Color32::from_rgb(250, 226, 210);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(246, 220, 204);
        visuals.widgets.active.bg_stroke =
            Stroke::new(1.2, Color32::from_rgb(224, 53, 107));
        visuals.widgets.active.fg_stroke =
            Stroke::new(1.5, Color32::from_rgb(22, 16, 31));
        visuals.widgets.active.corner_radius = radius_sm;

        visuals.widgets.open.bg_fill = Color32::from_rgb(246, 234, 222);
        visuals.widgets.open.weak_bg_fill = Color32::from_rgb(242, 230, 218);
        visuals.widgets.open.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(224, 53, 107));
        visuals.widgets.open.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(22, 16, 31));
        visuals.widgets.open.corner_radius = radius_sm;

        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(224, 53, 107, 42);
        visuals.selection.stroke = Stroke::new(1.2, Color32::from_rgb(200, 45, 95));
        visuals.hyperlink_color = Color32::from_rgb(29, 99, 210);
    }

    visuals.window_corner_radius = radius_md;
    visuals.menu_corner_radius = radius_sm;
    visuals.slider_trailing_fill = true;

    style.visuals = visuals;
    style
}

/// Draw a clipped checkerboard backdrop under a transparent image.
pub fn paint_checkerboard(painter: &egui::Painter, rect: Rect, dark: bool) {
    let clipped = painter.with_clip_rect(rect);
    let (c1, c2) = if dark {
        (Color32::from_rgb(26, 26, 32), Color32::from_rgb(38, 38, 46))
    } else {
        (
            Color32::from_rgb(232, 232, 236),
            Color32::from_rgb(206, 206, 212),
        )
    };
    clipped.rect_filled(rect, 0.0, c1);
    let tile = 16.0_f32;
    let cols = ((rect.width() / tile).ceil() as i32).clamp(0, 256);
    let rows = ((rect.height() / tile).ceil() as i32).clamp(0, 256);
    for row in 0..rows {
        for col in 0..cols {
            if (row + col) % 2 == 1 {
                let min = Pos2::new(
                    rect.left() + col as f32 * tile,
                    rect.top() + row as f32 * tile,
                );
                let max = Pos2::new(
                    (min.x + tile).min(rect.right()),
                    (min.y + tile).min(rect.bottom()),
                );
                clipped.rect_filled(Rect::from_min_max(min, max), 0.0, c2);
            }
        }
    }
}

/// Discover and register system fonts for Symbols, Chinese (Simplified/Traditional),
/// Japanese, Korean, and broad Unicode scripts on Windows, macOS, and Linux.
///
/// This ensures UI glyphs (arrows, media controls, math operators, icons) and
/// multilingual filenames (CJK, Korean Hangul, Japanese Kana/Kanji, Cyrillic, etc.)
/// render cleanly without missing-glyph boxes (`□`).
///
/// Returns `true` if at least one CJK font was loaded.
pub fn install_system_fonts(ctx: &Context) -> bool {
    let mut defs = FontDefinitions::default();

    // 1. Ensure `Hack` is included in Proportional family as well.
    // Egui bundles Hack with rich box-drawing, mathematical, and arrow symbols
    // (such as ⇄, ⇅, ▦, ◉, ●, ◐, ↑, ↓, ←, →), but by default only places it in Monospace.
    if let Some(prop) = defs.families.get_mut(&FontFamily::Proportional) {
        if !prop.contains(&"Hack".to_string()) {
            prop.push("Hack".to_string());
        }
    }

    let mut loaded_canonical_paths = std::collections::HashSet::new();
    let mut has_cjk = false;

    // Helper closure to load the first available candidate font from a list.
    let mut load_font_candidate =
        |key: &str, candidates: &[PathBuf], prop_priority: bool| -> bool {
            for path in candidates {
                if !path.is_file() {
                    continue;
                }
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                if loaded_canonical_paths.contains(&canonical) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(path) else {
                    continue;
                };
                if bytes.len() < 1024 {
                    continue;
                }

                defs.font_data.insert(
                    key.to_owned(),
                    Arc::new(FontData::from_owned(bytes)),
                );

                if let Some(family) = defs.families.get_mut(&FontFamily::Proportional) {
                    if prop_priority {
                        // Place symbol fonts right after the primary Latin font (index 1)
                        // so vector symbols take precedence over default emoji outlines.
                        family.insert(1.min(family.len()), key.to_owned());
                    } else {
                        family.push(key.to_owned());
                    }
                }
                if let Some(family) = defs.families.get_mut(&FontFamily::Monospace) {
                    family.push(key.to_owned());
                }

                loaded_canonical_paths.insert(canonical);
                log::info!("Loaded system font for '{key}': {}", path.display());
                return true;
            }
            false
        };

    // 2. Discover system symbol & icon fonts (provides symbols like ✕, ☾, ⟲, ⟳, ⛶, etc.)
    load_font_candidate("hotview-symbols", &symbol_font_candidates(), true);

    // 3. Discover Chinese CJK fonts (Simplified / Traditional)
    if load_font_candidate("hotview-cjk-zh", &cjk_zh_font_candidates(), false) {
        has_cjk = true;
    }

    // 4. Discover Japanese CJK fonts (Kanji / Kana)
    if load_font_candidate("hotview-cjk-ja", &cjk_ja_font_candidates(), false) {
        has_cjk = true;
    }

    // 5. Discover Korean CJK fonts (Hangul syllables & Jamo)
    if load_font_candidate("hotview-cjk-ko", &cjk_ko_font_candidates(), false) {
        has_cjk = true;
    }

    // 6. Discover broad Unicode fallback font
    load_font_candidate("hotview-unicode", &unicode_fallback_candidates(), false);

    ctx.set_fonts(defs);
    has_cjk
}

fn symbol_font_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        let fonts = Path::new(&windir).join("Fonts");
        paths.push(fonts.join("seguisym.ttf"));
        paths.push(fonts.join("seguiemj.ttf"));
    }
    paths.push(PathBuf::from(r"C:\Windows\Fonts\seguisym.ttf"));
    paths.push(PathBuf::from(r"C:\Windows\Fonts\seguiemj.ttf"));

    // macOS
    paths.push(PathBuf::from("/System/Library/Fonts/Apple Symbols.ttf"));
    paths.push(PathBuf::from("/System/Library/Fonts/Supplemental/Apple Symbols.ttf"));
    paths.push(PathBuf::from("/System/Library/Fonts/Apple Color Emoji.ttc"));

    // Linux
    for p in [
        "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansSymbols-Regular.otf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/symbola/Symbola.ttf",
        "/usr/share/fonts/google-noto/NotoSansSymbols-Regular.ttf",
        "/usr/local/share/fonts/NotoSansSymbols-Regular.ttf",
    ] {
        paths.push(PathBuf::from(p));
    }
    paths
}

fn cjk_zh_font_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        let fonts = Path::new(&windir).join("Fonts");
        for name in ["msyh.ttc", "msyh.ttf", "simsun.ttc", "simhei.ttf", "mingliu.ttc"] {
            paths.push(fonts.join(name));
        }
    }
    for name in [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\mingliu.ttc",
    ] {
        paths.push(PathBuf::from(name));
    }

    // macOS
    for p in [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
    ] {
        paths.push(PathBuf::from(p));
    }

    // Linux
    for p in [
        "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/adobe-source-han-sans/SourceHanSansSC-Regular.otf",
        "/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Regular.otf",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/local/share/fonts/NotoSansCJK-Regular.ttc",
    ] {
        paths.push(PathBuf::from(p));
    }
    paths
}

fn cjk_ja_font_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        let fonts = Path::new(&windir).join("Fonts");
        for name in ["YuGothR.ttc", "meiryo.ttc", "msgothic.ttc"] {
            paths.push(fonts.join(name));
        }
    }
    for name in [
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ] {
        paths.push(PathBuf::from(name));
    }

    // macOS
    for p in [
        "/System/Library/Fonts/Hiragino Sans.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Supplemental/Yu Gothic Medium.otf",
    ] {
        paths.push(PathBuf::from(p));
    }

    // Linux
    for p in [
        "/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf",
        "/usr/share/fonts/opentype/noto/NotoSansJP-Regular.otf",
        "/usr/share/fonts/truetype/noto/NotoSansJP-Regular.ttf",
        "/usr/share/fonts/truetype/vlgothic/VL-Gothic-Regular.ttf",
        "/usr/share/fonts/truetype/ipafont-gothic/ipag.ttf",
    ] {
        paths.push(PathBuf::from(p));
    }
    paths
}

fn cjk_ko_font_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        let fonts = Path::new(&windir).join("Fonts");
        for name in ["malgun.ttf", "malgun.ttc", "gulim.ttc", "batang.ttc"] {
            paths.push(fonts.join(name));
        }
    }
    for name in [
        r"C:\Windows\Fonts\malgun.ttf",
        r"C:\Windows\Fonts\malgun.ttc",
        r"C:\Windows\Fonts\gulim.ttc",
        r"C:\Windows\Fonts\batang.ttc",
    ] {
        paths.push(PathBuf::from(name));
    }

    // macOS
    for p in [
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
    ] {
        paths.push(PathBuf::from(p));
    }

    // Linux
    for p in [
        "/usr/share/fonts/opentype/noto/NotoSansCJKkr-Regular.otf",
        "/usr/share/fonts/opentype/noto/NotoSansKR-Regular.otf",
        "/usr/share/fonts/truetype/noto/NotoSansKR-Regular.ttf",
        "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
        "/usr/share/fonts/truetype/baekmuk/gulim.ttf",
    ] {
        paths.push(PathBuf::from(p));
    }
    paths
}

fn unicode_fallback_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        let fonts = Path::new(&windir).join("Fonts");
        paths.push(fonts.join("segoeui.ttf"));
        paths.push(fonts.join("arial.ttf"));
    }
    paths.push(PathBuf::from(r"C:\Windows\Fonts\segoeui.ttf"));
    paths.push(PathBuf::from(r"C:\Windows\Fonts\arial.ttf"));

    // macOS
    paths.push(PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"));
    paths.push(PathBuf::from("/Library/Fonts/Arial Unicode.ttf"));

    // Linux
    paths.push(PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"));
    paths.push(PathBuf::from("/usr/share/fonts/truetype/freefont/FreeSans.ttf"));

    paths
}

/// Procedurally render the 64×64 Hotview window icon matching `docs/favicon.svg`:
/// deep indigo-violet squircle background, cool blue-purple aperture ring,
/// and warm golden-coral flame in the centre.
pub fn build_app_icon() -> IconData {
    const SIZE: u32 = 64;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            // Rounded square SDF: 64x64 with corner radius 14.
            let qx = (fx - 32.0).abs() - (32.0 - 14.0);
            let qy = (fy - 32.0).abs() - (32.0 - 14.0);
            let dist_box = (qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0)) - 14.0;
            let bg_alpha = (0.5 - dist_box).clamp(0.0, 1.0);
            if bg_alpha <= 0.0 {
                continue;
            }

            // Diagonal background gradient (#141a33 -> #231a45 -> #3a1b3d)
            let t_bg = ((fx + fy) / 128.0).clamp(0.0, 1.0);
            let (mut r, mut g, mut b) = if t_bg < 0.55 {
                let k = t_bg / 0.55;
                lerp_rgb((0x14, 0x1a, 0x33), (0x23, 0x1a, 0x45), k)
            } else {
                let k = (t_bg - 0.55) / 0.45;
                lerp_rgb((0x23, 0x1a, 0x45), (0x3a, 0x1b, 0x3d), k)
            };

            // Aperture ring: outer radius 18.0, inner radius 12.5, stroke width ~3.4
            let dx = fx - 32.0;
            let dy = fy - 32.0;
            let rad = dx.hypot(dy);
            let ring_dist = (rad - 15.25).abs() - 2.75;
            let ring_alpha = (0.6 - ring_dist).clamp(0.0, 1.0);
            if ring_alpha > 0.0 {
                let (rr, rg, rb) = lerp_rgb((0x9c, 0xca, 0xff), (0xb3, 0x88, 0xff), t_bg);
                r = lerp(r, rr, ring_alpha);
                g = lerp(g, rg, ring_alpha);
                b = lerp(b, rb, ring_alpha);
            }

            // Central flame silhouette (teardrop centered around (32, 31.5))
            let ny = (fy - 20.5) / 20.4; // 0 at top tip (20.5), 1 at base (40.9)
            if (-0.05..=1.08).contains(&ny) {
                let cy = ny.clamp(0.0, 1.0);
                // Half-width profile of a flame: narrow at top, widest around 0.62, rounded at bottom
                let half_w = 5.6 * (cy.powf(0.55)) * ((1.0 - cy * 0.72).max(0.0)).sqrt() * 1.45;
                let wave = (1.0 - cy) * 0.6 * ((cy * std::f32::consts::PI).sin());
                let flame_dx = (fx - (32.0 + wave)).abs();
                let flame_dist = flame_dx - half_w;
                let flame_alpha = (0.65 - flame_dist).clamp(0.0, 1.0)
                    * ((ny * 8.0).clamp(0.0, 1.0))
                    * (((1.04 - ny) * 8.0).clamp(0.0, 1.0));
                if flame_alpha > 0.0 {
                    let (fr, fg, fb) = if cy < 0.45 {
                        lerp_rgb((0xff, 0xe0, 0x82), (0xff, 0xa7, 0x26), cy / 0.45)
                    } else {
                        lerp_rgb((0xff, 0xa7, 0x26), (0xff, 0x5a, 0x36), (cy - 0.45) / 0.55)
                    };
                    r = lerp(r, fr, flame_alpha);
                    g = lerp(g, fg, flame_alpha);
                    b = lerp(b, fb, flame_alpha);
                }
            }

            let idx = ((y * SIZE + x) * 4) as usize;
            rgba[idx] = r.round().clamp(0.0, 255.0) as u8;
            rgba[idx + 1] = g.round().clamp(0.0, 255.0) as u8;
            rgba[idx + 2] = b.round().clamp(0.0, 255.0) as u8;
            rgba[idx + 3] = (bg_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }

    IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (f32, f32, f32) {
    let t = t.clamp(0.0, 1.0);
    (
        lerp(a.0 as f32, b.0 as f32, t),
        lerp(a.1 as f32, b.1 as f32, t),
        lerp(a.2 as f32, b.2 as f32, t),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontId;

    #[test]
    fn test_glyph_coverage() {
        let _ = env_logger::builder().is_test(true).try_init();
        let ctx = Context::default();
        let loaded = install_system_fonts(&ctx);
        println!("install_system_fonts returned: {loaded}");
        assert!(loaded, "install_system_fonts should discover system fonts");

        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        let mut output2 = ctx.run_ui(Default::default(), |_| {});
        output2.textures_delta.clear();
        
        let all_hotview_chars = [
            '⬅', '●', '📄', '📁', '◀', '▶', '−', '+', '⟲', '⟳', '⇄', '⇅', '▦',
            '↑', '↓', '←', '→', '🔍', '✕', '⛶', '⚙', '⏸', '⏪', '⏩',
            '🔇', '🔉', '🔊', '◉', '⚠', '📂', '📋', '⌨', '◐', '☾', '☀', 'ℹ',
            // Chinese
            '你', '好', '世', '界', '打', '开', '文', '件', '夹', '设', '置', '图', '片',
            // Japanese
            '日', '本', '語', '再', '生', '前', '次', '拡', '大', '画', '像',
            // Korean
            '한', '国', '어', '파', '일', '열', '기', '재', '생',
            // Cyrillic
            'Р', 'у', 'с', 'с', 'к', 'и', 'й',
            // German
            'ö', 'ä', 'ü', 'ß',
        ];
        
        ctx.fonts_mut(|f| {
            let font_id = FontId::proportional(14.0);
            let mut failed = Vec::new();
            for &c in &all_hotview_chars {
                let galley = f.layout_no_wrap(c.to_string(), font_id.clone(), egui::Color32::WHITE);
                let first = galley.rows[0].row.glyphs.first();
                if let Some(g) = first {
                    // Make sure it has a positive advance width and non-zero height
                    if g.advance_width <= 0.0 {
                        failed.push((c, "zero advance width"));
                    }
                } else {
                    failed.push((c, "no glyph produced"));
                }
            }
            println!("All {} tested chars verified!", all_hotview_chars.len());
            assert!(failed.is_empty(), "Failed glyphs: {:?}", failed);
        });
    }
}

