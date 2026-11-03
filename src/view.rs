//! view.rs — plain, drawable view models. Activities never draw.

use std::path::Path;
use std::sync::Arc;

use crate::design::{self, ActivityKind, Color, RectD};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconId {
    Music,
    Phone,
    Folder,
    Clock,
    Speaker,
    SpeakerMuted,
    DownArrow,
    Mic,
    MicMuted,
    Signal,
    Shuffle,
    Repeat,
    Play,
    Pause,
    Previous,
    Next,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    Pill,
    Bubble,
    SwitchChip,
    ClearAll,
    ShelfTile(usize),
    ProgressRow,
    MusicShuffle,
    MusicPrev,
    MusicPlay,
    MusicNext,
    MusicRepeat,
    CallMic,
    CallOutput,
    CallOpen,
    TimerToggle,
    TimerAdd,
    TimerStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    None,
    List,
    Track,
}

#[derive(Debug, Clone)]
pub struct AlbumArt {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Copy)]
pub struct TileView {
    pub color: Color,
    pub alpha: f32,
    pub glyph: Color,
}

impl TileView {
    pub fn colored(color: Color) -> Self {
        Self {
            color,
            alpha: 1.0,
            glyph: design::DARK_GLYPH,
        }
    }

    pub fn tinted(color: Color, alpha: f32) -> Self {
        Self {
            color,
            alpha,
            glyph: color,
        }
    }

    pub fn volume() -> Self {
        Self {
            color: design::white(1.0),
            alpha: design::VOLUME_TILE_A,
            glyph: design::white(1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PillKind {
    Normal,
    Volume { level: f32, muted: bool },
    DragOver,
    Finished,
}

#[derive(Debug, Clone)]
pub struct PillView {
    pub icon: IconId,
    pub tile: TileView,
    pub label: String,
    pub value: String,
    pub progress: Option<f32>,
    pub kind: PillKind,
}

#[derive(Debug, Clone)]
pub enum Leading {
    Art {
        art: Option<Arc<AlbumArt>>,
        accent: Color,
    },
    Glyph {
        icon: IconId,
        bg: Color,
        fg: Color,
    },
    Ring {
        fraction: f32,
    },
    Initial {
        ch: char,
        bg: Color,
        fg: Color,
    },
}

#[derive(Debug, Clone)]
pub enum Trailing {
    None,
    TimerTime(String),
    ClearAll,
}

#[derive(Debug, Clone, Default)]
pub struct Subtitle {
    pub prefix: String,
    pub accent: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MediaControls {
    pub toggle: bool,
    pub previous: bool,
    pub next: bool,
    pub shuffle: bool,
    pub repeat: bool,
    pub seek: bool,
}

#[derive(Debug, Clone)]
pub struct MusicBody {
    pub elapsed: String,
    pub remaining: String,
    pub fraction: f32,
    pub playing: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub controls: MediaControls,
    pub live: bool,
}

#[derive(Debug, Clone)]
pub struct CallBody {
    pub device: String,
    pub app_name: String,
    pub mic_muted: bool,
    pub output_muted: bool,
}

#[derive(Debug, Clone)]
pub struct FileTileView {
    pub name: String,
    pub size: String,
    pub badge: String,
    pub badge_color: Color,
}

#[derive(Debug, Clone)]
pub struct ShelfBody {
    pub files: Vec<FileTileView>,
}

#[derive(Debug, Clone)]
pub struct TimerBody {
    pub elapsed: String,
    pub total: String,
    pub fraction: f32,
    pub paused: bool,
    pub finished: bool,
}

#[derive(Debug, Clone)]
pub enum Body {
    Music(MusicBody),
    Call(CallBody),
    Shelf(ShelfBody),
    Timer(TimerBody),
}

#[derive(Debug, Clone)]
pub struct FlyoutView {
    pub leading: Leading,
    pub title: String,
    pub subtitle: Subtitle,
    pub trailing: Trailing,
    pub body: Body,
}

#[derive(Debug, Clone)]
pub struct ActivityView {
    pub kind: ActivityKind,
    pub accent: Color,
    pub pill: PillView,
    pub flyout: FlyoutView,
}

#[derive(Debug, Clone)]
pub struct SecondView {
    pub kind: ActivityKind,
    pub accent: Color,
    pub icon: IconId,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct ViewModel {
    pub main: Option<ActivityView>,
    pub second: Option<SecondView>,
    /// Volume HUD / shelf drag-over replace only the pill, never the flyout.
    pub overlay_pill: Option<PillView>,
}

impl ViewModel {
    pub fn empty() -> Self {
        Self {
            main: None,
            second: None,
            overlay_pill: None,
        }
    }
    pub fn pill(&self) -> Option<&PillView> {
        self.overlay_pill
            .as_ref()
            .or_else(|| self.main.as_ref().map(|m| &m.pill))
    }

    pub fn flyout(&self) -> Option<&FlyoutView> {
        if self.overlay_pill.is_some() {
            None
        } else {
            self.main.as_ref().map(|m| &m.flyout)
        }
    }

    pub fn empty_shelf_view(accent: Color) -> ActivityView {
        ActivityView {
            kind: ActivityKind::Shelf,
            accent,
            pill: PillView {
                icon: IconId::Folder,
                tile: TileView::colored(accent),
                label: "Shelf".into(),
                value: "empty".into(),
                progress: None,
                kind: PillKind::Normal,
            },
            flyout: FlyoutView {
                leading: Leading::Glyph {
                    icon: IconId::Folder,
                    bg: accent,
                    fg: design::DARK_GLYPH,
                },
                title: "Shelf empty".into(),
                subtitle: Subtitle::default(),
                trailing: Trailing::None,
                body: Body::Shelf(ShelfBody { files: Vec::new() }),
            },
        }
    }
}

pub fn icon_for(kind: ActivityKind) -> IconId {
    match kind {
        ActivityKind::Music => IconId::Music,
        ActivityKind::Call => IconId::Phone,
        ActivityKind::Shelf => IconId::Folder,
        ActivityKind::Timer => IconId::Clock,
        ActivityKind::Volume => IconId::Speaker,
    }
}

pub fn accent_for(kind: ActivityKind, theme_accent: Color) -> Color {
    match kind {
        ActivityKind::Music => design::MUSIC_DEFAULT,
        ActivityKind::Call => design::CALL_GREEN,
        ActivityKind::Shelf => theme_accent,
        ActivityKind::Timer => design::TIMER_AMBER,
        ActivityKind::Volume => design::VOLUME_WHITE,
    }
}

pub fn fmt_clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn fmt_clock_pad(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

pub fn fmt_remaining(seconds: f64) -> String {
    format!("-{}", fmt_clock(seconds))
}

pub fn fmt_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

pub fn total_size(paths: &[String]) -> u64 {
    paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

pub fn badge_for(path: &str) -> (String, Color) {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    match ext.as_str() {
        "PDF" => ("PDF".into(), design::BADGE_PDF),
        "PNG" | "JPG" | "JPEG" | "GIF" | "BMP" | "WEBP" | "SVG" | "TIF" | "TIFF" => {
            ("IMG".into(), design::BADGE_IMAGE)
        }
        "ZIP" | "RAR" | "7Z" | "TAR" | "GZ" | "BZ2" | "XZ" => ("ZIP".into(), design::BADGE_ARCHIVE),
        "DOC" | "DOCX" | "RTF" | "ODT" => ("DOC".into(), design::BADGE_DOC),
        "XLS" | "XLSX" | "CSV" | "ODS" => ("XLS".into(), design::BADGE_SHEET),
        "MP4" | "MKV" | "AVI" | "MOV" | "WEBM" | "WMV" => ("MP4".into(), design::BADGE_VIDEO),
        "TXT" | "MD" | "LOG" => ("TXT".into(), design::BADGE_OTHER),
        _ if ext.is_empty() => ("FILE".into(), design::BADGE_OTHER),
        _ if ext.len() <= 4 => (ext, design::BADGE_OTHER),
        _ => ("FILE".into(), design::BADGE_OTHER),
    }
}

pub fn file_tiles(paths: &[String]) -> Vec<FileTileView> {
    paths
        .iter()
        .take(3)
        .map(|path| {
            let name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            let size = std::fs::metadata(path)
                .map(|m| fmt_size(m.len()))
                .unwrap_or_else(|_| "—".into());
            let (badge, badge_color) = badge_for(path);
            FileTileView {
                name,
                size,
                badge,
                badge_color,
            }
        })
        .collect()
}

/// Splits the flyout's content box into the top row and body sections.
pub fn content_boxes(fly: RectD) -> (RectD, RectD) {
    let top = RectD::new(
        fly.x + design::FLY_PAD,
        fly.y + design::FLY_PAD,
        design::FLY_W - design::FLY_PAD * 2.0,
        design::TOP_ROW_H,
    );
    let body = RectD::new(
        fly.x + design::FLY_PAD,
        fly.y + design::FLY_PAD + design::TOP_ROW_H + design::ROW_GAP,
        design::FLY_W - design::FLY_PAD * 2.0,
        design::BODY_H,
    );
    (top, body)
}

/// Width available for the title/subtitle block after the leading tile and
/// (optional) trailing area. Renderers use measured text for the exact split.
pub fn text_block_width(top: RectD, has_trailing: bool) -> f32 {
    let leading = design::LEAD_TILE + design::ROW_GAP;
    let trailing = if has_trailing { 92.0 } else { 0.0 };
    (top.w - leading - trailing).max(40.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_formats() {
        assert_eq!(fmt_clock(84.0), "1:24");
        assert_eq!(fmt_clock(228.0), "3:48");
        assert_eq!(fmt_clock(3725.0), "1:02:05");
        assert_eq!(fmt_remaining(144.0), "-2:24");
        assert_eq!(fmt_clock_pad(760.0), "12:40");
    }

    #[test]
    fn badge_mapping() {
        assert_eq!(badge_for("a.pdf").0, "PDF");
        assert_eq!(badge_for("a.png").0, "IMG");
        assert_eq!(badge_for("a.zip").0, "ZIP");
        assert_eq!(badge_for("a.docx").0, "DOC");
        assert_eq!(badge_for("a.xlsx").0, "XLS");
        assert_eq!(badge_for("a.mp4").0, "MP4");
        assert_eq!(badge_for("a.txt").0, "TXT");
        assert_eq!(badge_for("a.weird").0, "FILE");
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(fmt_size(240 * 1024), "240 KB");
        assert_eq!(fmt_size(1_200_000), "1.1 MB");
    }
}
