//! Persistent desktop preferences and cross-platform OS helpers (Windows,
//! macOS, Linux).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::i18n::{Lang, LanguagePref, strings};
use crate::theme::{ThemeMode, ViewerBg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Date,
    Size,
    Kind,
}

impl SortBy {
    pub const ALL: [SortBy; 4] = [SortBy::Name, SortBy::Date, SortBy::Size, SortBy::Kind];

    pub fn code(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Date => "date",
            Self::Size => "size",
            Self::Kind => "kind",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "date" | "modified" => Self::Date,
            "size" => Self::Size,
            "kind" | "type" => Self::Kind,
            _ => Self::Name,
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        let s = strings(lang);
        match self {
            Self::Name => s.sort_name,
            Self::Date => s.sort_date,
            Self::Size => s.sort_size,
            Self::Kind => s.sort_kind,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DesktopSettings {
    pub theme: ThemeMode,
    pub language: LanguagePref,
    pub sort_by: SortBy,
    pub sort_asc: bool,
    pub grid_size: f32,
    pub auto_play: bool,
    pub loop_video: bool,
    pub volume: f32,
    pub muted: bool,
    pub show_filmstrip: bool,
    pub show_info: bool,
    pub viewer_bg: ViewerBg,
    pub slideshow_secs: u64,
    pub recent_folders: Vec<PathBuf>,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: LanguagePref::Auto,
            sort_by: SortBy::Name,
            sort_asc: true,
            grid_size: 168.0,
            auto_play: true,
            loop_video: false,
            volume: 1.0,
            muted: false,
            show_filmstrip: true,
            show_info: false,
            viewer_bg: ViewerBg::Dark,
            slideshow_secs: 5,
            recent_folders: Vec::new(),
        }
    }
}

impl DesktopSettings {
    pub fn load() -> Self {
        let mut settings = Self::default();
        let Some(path) = config_file_path() else {
            return settings;
        };
        let Ok(contents) = std::fs::read_to_string(path) else {
            return settings;
        };
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "theme" => settings.theme = ThemeMode::from_code(value),
                "language" => settings.language = LanguagePref::from_code(value),
                "sort_by" => settings.sort_by = SortBy::from_code(value),
                "sort_asc" => settings.sort_asc = value != "false" && value != "0",
                "grid_size" => {
                    if let Ok(v) = value.parse::<f32>() {
                        settings.grid_size = v.clamp(104.0, 320.0);
                    }
                }
                "auto_play" => settings.auto_play = value != "false" && value != "0",
                "loop_video" => settings.loop_video = value == "true" || value == "1",
                "volume" => {
                    if let Ok(v) = value.parse::<f32>() {
                        settings.volume = v.clamp(0.0, 1.0);
                    }
                }
                "muted" => settings.muted = value == "true" || value == "1",
                "show_filmstrip" => settings.show_filmstrip = value != "false" && value != "0",
                "show_info" => settings.show_info = value == "true" || value == "1",
                "viewer_bg" => settings.viewer_bg = ViewerBg::from_code(value),
                "slideshow_secs" => {
                    if let Ok(v) = value.parse::<u64>() {
                        settings.slideshow_secs = v.clamp(2, 60);
                    }
                }
                "recent" => {
                    if !value.is_empty() {
                        let p = PathBuf::from(value);
                        if p.is_dir() && !settings.recent_folders.contains(&p) {
                            settings.recent_folders.push(p);
                        }
                    }
                }
                _ => {}
            }
        }
        settings.recent_folders.truncate(8);
        settings
    }

    pub fn save(&self) {
        let Some(path) = config_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out = String::new();
        out.push_str("# Hotview desktop settings\n");
        out.push_str(&format!("theme={}\n", self.theme.code()));
        out.push_str(&format!("language={}\n", self.language.code()));
        out.push_str(&format!("sort_by={}\n", self.sort_by.code()));
        out.push_str(&format!("sort_asc={}\n", self.sort_asc));
        out.push_str(&format!("grid_size={:.1}\n", self.grid_size));
        out.push_str(&format!("auto_play={}\n", self.auto_play));
        out.push_str(&format!("loop_video={}\n", self.loop_video));
        out.push_str(&format!("volume={:.3}\n", self.volume));
        out.push_str(&format!("muted={}\n", self.muted));
        out.push_str(&format!("show_filmstrip={}\n", self.show_filmstrip));
        out.push_str(&format!("show_info={}\n", self.show_info));
        out.push_str(&format!("viewer_bg={}\n", self.viewer_bg.code()));
        out.push_str(&format!("slideshow_secs={}\n", self.slideshow_secs));
        for folder in self.recent_folders.iter().take(8) {
            if let Some(s) = folder.to_str() {
                if !s.contains('\n') && !s.contains('\r') {
                    out.push_str(&format!("recent={s}\n"));
                }
            }
        }
        let _ = std::fs::write(path, out);
    }

    pub fn push_recent(&mut self, folder: PathBuf) {
        let folder = normalize_path(&folder);
        if !folder.is_dir() {
            return;
        }
        self.recent_folders.retain(|existing| existing != &folder);
        self.recent_folders.insert(0, folder);
        self.recent_folders.truncate(8);
        self.save();
    }
}

fn config_file_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.is_empty() {
                return Some(PathBuf::from(appdata).join("Hotview").join("settings.conf"));
            }
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            if !profile.is_empty() {
                return Some(
                    PathBuf::from(profile)
                        .join("AppData")
                        .join("Roaming")
                        .join("Hotview")
                        .join("settings.conf"),
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                return Some(
                    PathBuf::from(home)
                        .join("Library")
                        .join("Application Support")
                        .join("Hotview")
                        .join("settings.conf"),
                );
            }
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("hotview").join("settings.conf"));
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                return Some(
                    PathBuf::from(home)
                        .join(".config")
                        .join("hotview")
                        .join("settings.conf"),
                );
            }
        }
    }

    None
}

/// Normalize a path so relative CLI arguments like `hotview photo.jpg` resolve
/// against the current working directory and always have a valid `.parent()`.
pub fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        // On Windows `canonicalize` prefixes `\\?\`; strip it for clean display
        // and compatibility with standard Win32 tools like explorer.exe.
        #[cfg(target_os = "windows")]
        {
            if let Some(s) = canon.to_str() {
                if let Some(stripped) = s.strip_prefix(r"\\?\") {
                    return PathBuf::from(stripped);
                }
            }
        }
        return canon;
    }
    if path.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(path);
        }
    }
    path.to_path_buf()
}

/// Reveal a file or folder in the native OS file manager (Windows Explorer,
/// macOS Finder, or Linux xdg-open).
pub fn reveal_in_file_manager(path: &Path) {
    let norm = normalize_path(path);

    #[cfg(target_os = "windows")]
    {
        if norm.is_file() {
            let _ = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", norm.display()))
                .spawn();
        } else {
            let _ = std::process::Command::new("explorer.exe").arg(&norm).spawn();
        }
    }

    #[cfg(target_os = "macos")]
    {
        if norm.is_file() {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(&norm)
                .spawn();
        } else {
            let _ = std::process::Command::new("open").arg(&norm).spawn();
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let dir = if norm.is_dir() {
            norm.as_path()
        } else {
            norm.parent().unwrap_or(norm.as_path())
        };
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Case-insensitive natural sort (`img2.jpg` < `img10.jpg`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();

    while ia.peek().is_some() && ib.peek().is_some() {
        let ca = *ia.peek().unwrap();
        let cb = *ib.peek().unwrap();

        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            while ia.peek() == Some(&'0') {
                ia.next();
            }
            while ib.peek() == Some(&'0') {
                ib.next();
            }
            let mut da = String::new();
            let mut db = String::new();
            while let Some(&c) = ia.peek() {
                if c.is_ascii_digit() {
                    da.push(c);
                    ia.next();
                } else {
                    break;
                }
            }
            while let Some(&c) = ib.peek() {
                if c.is_ascii_digit() {
                    db.push(c);
                    ib.next();
                } else {
                    break;
                }
            }
            match da.len().cmp(&db.len()).then_with(|| da.cmp(&db)) {
                Ordering::Equal => continue,
                non_eq => return non_eq,
            }
        }

        let la = ca.to_lowercase().next().unwrap_or(ca);
        let lb = cb.to_lowercase().next().unwrap_or(cb);
        match la.cmp(&lb) {
            Ordering::Equal => {
                ia.next();
                ib.next();
            }
            non_eq => return non_eq,
        }
    }

    ia.peek()
        .is_some()
        .cmp(&ib.peek().is_some())
        .then_with(|| a.cmp(b))
}

/// Extract the modification timestamp (seconds since UNIX_EPOCH) of a file.
pub fn file_modified_secs(path: &Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t: SystemTime| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format Unix seconds into `YYYY-MM-DD HH:MM` (Howard Hinnant's civil_from_days).
pub fn format_unix_date(secs: u64) -> String {
    if secs == 0 {
        return "—".to_string();
    }
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02} {hour:02}:{minute:02}")
}
