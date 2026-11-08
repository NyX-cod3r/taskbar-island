//! view.rs — plain, drawable view models. Activities never draw.

use std::path::Path;
use std::sync::Arc;

use crate::convert::options::Tab;
use crate::design::{self, ActivityKind, Color, RectD};
use crate::engines::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconId {
    Music,
    Phone,
    Folder,
    Clock,
    Speaker,
    SpeakerMuted,
    #[allow(dead_code)] // Dropdown chrome lands with the Convert card (B2).
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
    RailMusic,
    RailTimer,
    RailCall,
    RailShelf,
    Clipboard,
    Tray,
    Convert,
    Check,
    Image,
    Video,
    File,
    Compress,
    Resize,
    EyeOff,
    #[allow(dead_code)] // Overflow menus land with the Convert card (B2).
    More,
    Copy,
    Undo,
    #[allow(dead_code)] // "Add files" lands with the Convert card (B2).
    Plus,
    #[allow(dead_code)] // Card back action lands with the Convert card (B2).
    Back,
    SelectAll,
    Trash,
    Edit,
    Extract,
    TextLines,
    Fit,
    Thumbnail,
    Web,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Grayscale,
    Contrast,
    Crop,
    Speed,
    Save,
    Pin,
    Link,
    GridAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    Pill,
    Bubble,
    #[allow(dead_code)] // Switch-chip hover/hit lands with the pill states (A).
    SwitchChip,
    ShelfTile(usize),
    ShelfRemove(usize),
    ShelfSelectAll,
    ShelfClear,
    ShelfCount,
    ShelfScrollbar,
    ConvertButton,
    ConvertClose,
    ConvertTab(Tab),
    ConvertOption(Action),
    ConvertCancel,
    ConvertShow,
    ConvertCopy,
    ConvertUndo,
    /// 0=reading, 1=converting, 2=saving; tooltips only.
    ConvertStage(usize),
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
    TimerPreset(u32),
    TimerStop,
    ClipFilter(ClipFilter),
    ClipPause,
    ClipClear,
    /// Second step of the two-step Clear: red-tinted check.
    ClipClearConfirm,
    /// Cancels an armed Clear.
    ClipClearCancel,
    ClipRow(u64),
    ClipPin(u64),
    ClipCopy(u64),
    ClipDelete(u64),
    ClipScrollbar,
    Rail(ActivityKind),
}

/// One icon-only tab in the Convert card's segmented strip.
#[derive(Debug, Clone)]
pub struct ConvertTabView {
    pub tab: Tab,
    #[allow(dead_code)] // Tab labels land with the Convert card (B2).
    pub label: String,
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PillKind {
    Normal,
    DragOver,
    Finished,
    /// Clipboard pill; `paused` marks capture suspended with a small dot.
    Clipboard {
        paused: bool,
    },
}

#[derive(Debug, Clone)]
pub struct PillView {
    pub icon: IconId,
    pub tile: TileView,
    pub label: String,
    pub value: String,
    pub progress: Option<f32>,
    pub kind: PillKind,
    /// Album cover for the leading tile; falls back to `icon` when absent.
    pub art: Option<Arc<AlbumArt>>,
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
}

#[derive(Debug, Clone)]
pub enum Trailing {
    None,
    /// Convert card: the only header affordance is a close glyph.
    Close,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // Subtitle line lands with the music card (B2).
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
    pub title: String,
    pub artist: String,
    pub source: String,
    pub art: Option<Arc<AlbumArt>>,
    pub elapsed: String,
    pub remaining: String,
    pub fraction: f32,
    pub playing: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub controls: MediaControls,
}

#[derive(Debug, Clone)]
pub struct CallBody {
    pub device: String,
    pub app_name: String,
    /// Running call time; empty when no call is live.
    pub elapsed: String,
    pub live: bool,
    pub mic_muted: bool,
    pub output_muted: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // size/badge/image line lands with the shelf tile UI (B2).
pub struct FileTileView {
    /// Index in the whole shelf, not just the visible window.
    pub index: usize,
    /// Absolute row and column; the renderer offsets by the scroll position.
    pub row: usize,
    pub col: usize,
    pub name: String,
    pub size: String,
    pub badge: String,
    pub badge_color: Color,
    /// Glyph on the colored tile.
    pub icon: IconId,
    /// Ctrl+clicked tile; a drag from any selected tile takes them all.
    pub selected: bool,
    /// Image files get a thumbnail instead of the colored type chip.
    pub image: bool,
    /// Cached thumbnail, when the worker has decoded one.
    pub thumb: Option<Arc<AlbumArt>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ShelfScroll {
    /// First visible row.
    pub first_row: usize,
    /// Number of rows built for this offset (includes the partially visible
    /// top and bottom rows).
    pub rows: usize,
    /// Total height of every row, including the gap after the last one.
    pub content_h: f32,
    /// Largest legal scroll offset in DIP.
    pub max_scroll: f32,
    /// Clamped scroll offset in DIP.
    pub scroll: f32,
}

#[derive(Debug, Clone)]
pub struct ShelfBody {
    /// Only the rows intersecting the viewport (virtualized).
    pub files: Vec<FileTileView>,
    pub total: usize,
    pub selected: usize,
    /// Selected entries the converter understands.
    pub convertible: usize,
    pub scroll: f32,
    pub max_scroll: f32,
    pub content_h: f32,
    /// The soft cap dropped the oldest entries; the count tooltip says so.
    #[allow(dead_code)] // Overflow tooltip lands with the shelf toolbar (B2).
    pub overflow: bool,
    /// Files are on their way to the converter; the toolbar stays visual only.
    pub converting: bool,
    /// Rubber-band rectangle in card DIP coordinates while one is dragged.
    pub rubber: Option<(f32, f32, f32, f32)>,
}

/// What a wheel notch does. An open card owns the wheel: it either scrolls
/// the card's list/grid or is ignored, but never switches activity underneath
/// the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelAction {
    ScrollList,
    Cycle,
    Ignore,
}

pub fn wheel_action(
    flyout_open: bool,
    list_card: bool,
    over_list: bool,
    has_scroll: bool,
) -> WheelAction {
    if !flyout_open {
        WheelAction::Cycle
    } else if list_card && over_list && has_scroll {
        WheelAction::ScrollList
    } else {
        WheelAction::Ignore
    }
}

#[derive(Debug, Clone)]
pub struct ConvertOptionView {
    pub action: Option<Action>,
    pub format: Option<String>,
    pub icon: Option<IconId>,
    /// Tooltip and accessibility name.
    #[allow(dead_code)] // Option tooltips land with the Convert card (B2).
    pub label: String,
}

#[derive(Debug, Clone)]
pub enum ConvertState {
    /// Header, tabs and the options grid.
    Grid,
    /// A file that cannot be converted (unsupported, cloud placeholder,
    /// oversized, damaged). The grid is replaced by an explanation.
    Notice { title: String, detail: String },
    /// A Ctrl+drag was detected but the file has not reached the island yet.
    /// The options appear as soon as the drag enters.
    Waiting,
    /// Work in flight. The card shows source → target and a progress bar.
    Working(WorkingView),
    /// Finished: the new file is already on disk next to the original.
    Done(DoneView),
    /// The engine refused the file, or the write failed.
    Failed { title: String, detail: String },
}

#[derive(Debug, Clone)]
pub struct WorkingView {
    /// 0..=100, already eased by the app so the bar never jumps.
    pub percent: u8,
    pub source_badge: String,
    pub source_color: Color,
    pub target_badge: String,
    /// Dashed target tile until the file lands.
    #[allow(dead_code)] // Target tile color lands with the working state (B2).
    pub target_color: Color,
    /// 0=reading, 1=converting, 2=saving; earlier stages show a green check
    /// and later ones stay gray.
    pub stage: u8,
    pub cancelling: bool,
}

#[derive(Debug, Clone)]
pub struct DoneView {
    pub name: String,
    pub meta: String,
    /// Positive green meta ("54% smaller") vs neutral ("2.4 MB").
    pub meta_accent: bool,
    /// Negative amber meta ("+12% larger"); ignored when `meta_accent` is set.
    pub meta_grew: bool,
    /// Clipboard-only results have no file, so the three shell buttons hide.
    pub has_file: bool,
    /// 0..=1 of the countdown still remaining, drawn as a thin line.
    pub countdown: f32,
}

#[derive(Debug, Clone)]
pub struct ConvertBody {
    /// Single-line, truncated file name or "3 files".
    pub title: String,
    /// Type badge text and color for the 24 DIP header tile when there is no
    /// thumbnail.
    #[allow(dead_code)] // Header badge lands with the Convert card (B2).
    pub badge: String,
    pub badge_color: Color,
    pub icon: IconId,
    /// 24 DIP header thumbnail for a single image; multi-file shows a stack.
    pub thumb: Option<Arc<AlbumArt>>,
    pub stacked: bool,
    pub tabs: Vec<ConvertTabView>,
    pub active_tab: Tab,
    pub options: Vec<ConvertOptionView>,
    /// Option under an in-flight Ctrl+drag: it gets the drop-target ring and
    /// every other tile dims.
    pub drag_target: Option<crate::engines::Action>,
    /// Hint row; notice/failure states keep their instructions here.
    pub hint: String,
    pub state: ConvertState,
}

#[derive(Debug, Clone)]
pub struct TimerBody {
    pub remaining: String,
    /// Elapsed fraction for the progress fill.
    pub fraction: f32,
    pub running: bool,
    /// A duration is set but the countdown has not started.
    pub armed: bool,
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipFilter {
    All,
    Text,
    Images,
    Links,
    Pinned,
}

impl ClipFilter {
    pub const ALL: [ClipFilter; 5] = [
        ClipFilter::All,
        ClipFilter::Text,
        ClipFilter::Images,
        ClipFilter::Links,
        ClipFilter::Pinned,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Text => "Text",
            Self::Images => "Images",
            Self::Links => "Links",
            Self::Pinned => "Pinned",
        }
    }

    pub fn icon(self) -> IconId {
        match self {
            Self::All => IconId::GridAll,
            Self::Text => IconId::TextLines,
            Self::Images => IconId::Image,
            Self::Links => IconId::Link,
            Self::Pinned => IconId::Pin,
        }
    }

    pub fn matches(self, item: &crate::clipboard::store::ClipItem) -> bool {
        use crate::clipboard::store::ClipKind;
        match self {
            Self::All => true,
            Self::Text => item.kind == ClipKind::Text,
            Self::Images => item.kind == ClipKind::Image,
            Self::Links => item.kind == ClipKind::Link,
            Self::Pinned => item.pinned,
        }
    }
}

/// One visible history row. `index` is absolute inside the filtered list so
/// keyboard focus and scrolling survive virtualization; `id` addresses the
/// store row for actions.
#[derive(Debug, Clone)]
pub struct ClipRowView {
    pub index: usize,
    pub id: u64,
    pub icon: IconId,
    pub preview: String,
    /// Code-like text previews use the mono font.
    pub mono: bool,
    pub link: bool,
    #[allow(dead_code)] // Row image chip lands with the clipboard card (B1).
    pub image: bool,
    pub thumb: Option<Arc<AlbumArt>>,
    pub age: String,
    /// Last 24 h of retention: the age turns amber.
    pub expiring: bool,
    #[allow(dead_code)] // Pin marker lands with the clipboard card (B1).
    pub pinned: bool,
    /// Every path of a file row is gone.
    pub dimmed: bool,
    /// The preview looks like a credential, so it is drawn as bullets until
    /// the row is hovered or focused (spec 3.6).
    pub masked: bool,
}

#[derive(Debug, Clone)]
pub struct ClipboardBody {
    pub total: usize,
    pub pinned: usize,
    /// Capture is suspended; the card and pill show a paused marker.
    pub paused: bool,
    pub filter: ClipFilter,
    /// "10 / 30", or "3 pinned" under the Pinned filter.
    pub count_text: String,
    /// Only the rows intersecting the viewport (virtualized).
    pub rows: Vec<ClipRowView>,
    pub scroll: f32,
    pub max_scroll: f32,
    pub content_h: f32,
    /// Row showing the hover/focus action cluster (filtered index).
    pub active_row: Option<usize>,
    /// Two-step Clear is armed: `Some(unpinned count)`. The toolbar draws the
    /// red-tinted check and the cancel in place of the trash (spec 3.3).
    pub clear_confirm: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Body {
    Music(MusicBody),
    Call(CallBody),
    Shelf(ShelfBody),
    Timer(TimerBody),
    Clipboard(ClipboardBody),
    Convert(ConvertBody),
}

#[derive(Debug, Clone)]
pub struct FlyoutView {
    pub kind: ActivityKind,
    /// Header tile (28 DIP) for Timer/Call/Shelf; Music uses the body's art.
    pub leading: Leading,
    pub title: String,
    /// Music only: `Artist · App`.
    #[allow(dead_code)] // Subtitle line lands with the music card (B2).
    pub subtitle: Subtitle,
    /// Right-aligned header status.
    pub status: String,
    /// Draw the status in the activity accent (otherwise secondary text).
    pub status_accent: bool,
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
    pub accent: Color,
    pub icon: IconId,
    #[allow(dead_code)] // Second-pill label lands with the pill states (A).
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct ViewModel {
    pub main: Option<ActivityView>,
    pub second: Option<SecondView>,
    /// Shelf drag-over replaces only the pill, never the flyout.
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

    /// Idle fallback: keeps the island visible and clickable on the taskbar
    /// when no activity is live (otherwise the window is fully transparent).
    pub fn idle(accent: Color) -> ActivityView {
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
                art: None,
            },
            flyout: FlyoutView {
                kind: ActivityKind::Shelf,
                leading: Leading::Glyph {
                    icon: IconId::Folder,
                    bg: accent,
                    fg: design::DARK_GLYPH,
                },
                title: "Shelf".into(),
                subtitle: Subtitle::default(),
                status: String::new(),
                status_accent: false,
                trailing: Trailing::None,
                body: Body::Shelf(ShelfBody {
                    files: Vec::new(),
                    total: 0,
                    selected: 0,
                    convertible: 0,
                    scroll: 0.0,
                    max_scroll: 0.0,
                    content_h: 0.0,
                    overflow: false,
                    converting: false,
                    rubber: None,
                }),
            },
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

pub fn fmt_remaining(seconds: f64) -> String {
    format!("-{}", fmt_clock(seconds))
}

/// Clipboard pill: the shell is always the clipboard glyph. `pop` holds the
/// number of items just captured; it shows as "+N" until the window clears it.
pub fn clipboard_pill(count: usize, pop: Option<u32>, paused: bool) -> PillView {
    let value = match pop {
        Some(n) => format!("+{n}"),
        None if count > 0 => count.to_string(),
        None => String::new(),
    };
    PillView {
        icon: IconId::Clipboard,
        tile: TileView::colored(design::RAIL_CLIP),
        label: "Clipboard".into(),
        value,
        progress: None,
        kind: PillKind::Clipboard { paused },
        art: None,
    }
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
        .filter(|m| !m.is_dir())
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
        "PDF" => ("PDF".into(), design::FILE_PDF),
        "PNG" | "JPG" | "JPEG" | "GIF" | "BMP" | "WEBP" | "SVG" | "TIF" | "TIFF" => {
            ("IMG".into(), design::FILE_IMAGE)
        }
        "ZIP" | "RAR" | "7Z" | "TAR" | "GZ" | "BZ2" | "XZ" => ("ZIP".into(), design::BADGE_ARCHIVE),
        "DOC" | "DOCX" | "RTF" | "ODT" => ("DOC".into(), design::BADGE_DOC),
        "XLS" | "XLSX" | "CSV" | "ODS" => ("XLS".into(), design::BADGE_SHEET),
        "MP4" | "MKV" | "AVI" | "MOV" | "WEBM" | "WMV" => ("MP4".into(), design::FILE_VIDEO),
        "TXT" | "MD" | "LOG" => ("TXT".into(), design::BADGE_OTHER),
        _ if ext.is_empty() => ("FILE".into(), design::BADGE_OTHER),
        _ if ext.len() <= 4 => (ext, design::BADGE_OTHER),
        _ => ("FILE".into(), design::BADGE_OTHER),
    }
}

/// Glyph drawn on the shelf tile for a file, chosen from the container family.
pub fn icon_for(path: &str) -> IconId {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    match ext.as_str() {
        "PNG" | "JPG" | "JPEG" | "GIF" | "BMP" | "WEBP" | "SVG" | "TIF" | "TIFF" | "ICO" => {
            IconId::Image
        }
        "MP4" | "MKV" | "AVI" | "MOV" | "WEBM" | "WMV" => IconId::Video,
        _ => IconId::File,
    }
}

/// Number of grid rows the viewport needs, including the rows clipped by the
/// top and bottom edges. `content_h` includes the gap after the last row so a
/// full last row can scroll flush against the bottom edge.
pub fn shelf_scroll(len: usize, scroll: f32) -> ShelfScroll {
    let cols = design::SHELF_COLS;
    let rows_total = len.div_ceil(cols);
    let content_h = if rows_total == 0 {
        0.0
    } else {
        rows_total as f32 * design::SHELF_ROW_H
    };
    let max_scroll = (content_h - design::SHELF_VIEW_H).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let row_h = design::SHELF_ROW_H;
    let first_row = (scroll / row_h).floor() as usize;
    let offset = scroll - first_row as f32 * row_h;
    let rows = ((design::SHELF_VIEW_H + offset) / row_h).ceil() as usize + 1;
    let rows = rows.min(rows_total.saturating_sub(first_row));
    ShelfScroll {
        first_row,
        rows,
        content_h,
        max_scroll,
        scroll,
    }
}

/// A row range that covers the viewport at `scroll`, built from the whole
/// path list so indices stay absolute.
pub fn shelf_tiles(
    paths: &[String],
    selected: &[usize],
    scroll: f32,
    thumb: &dyn Fn(&str) -> Option<Arc<AlbumArt>>,
) -> Vec<FileTileView> {
    let view = shelf_scroll(paths.len(), scroll);
    let cols = design::SHELF_COLS;
    let mut files = Vec::with_capacity(view.rows * cols);
    for row in view.first_row..view.first_row + view.rows {
        for col in 0..cols {
            let index = row * cols + col;
            let Some(path) = paths.get(index) else {
                break;
            };
            let name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            let meta = std::fs::metadata(path).ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let (size, badge, badge_color) = if is_dir {
                ("Folder".to_string(), "DIR".to_string(), design::BADGE_IMAGE)
            } else {
                let size = meta
                    .map(|m| fmt_size(m.len()))
                    .unwrap_or_else(|| "—".into());
                let (badge, badge_color) = badge_for(path);
                (size, badge, badge_color)
            };
            let icon = if is_dir {
                IconId::Folder
            } else {
                icon_for(path)
            };
            let image = !is_dir && is_image(path);
            files.push(FileTileView {
                index,
                row,
                col,
                name,
                size,
                badge,
                badge_color,
                icon,
                selected: selected.contains(&index),
                image,
                thumb: if image { thumb(path) } else { None },
            });
        }
    }
    files
}

/// Viewport math for the history list; same shape as the shelf grid but one
/// row per item with a 34 DIP stride.
pub fn clip_scroll(len: usize, scroll: f32) -> ShelfScroll {
    let content_h = len as f32 * design::CLIP_ROW_STRIDE;
    let max_scroll = (content_h - design::CLIP_LIST_H).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let stride = design::CLIP_ROW_STRIDE;
    let first_row = (scroll / stride).floor() as usize;
    let offset = scroll - first_row as f32 * stride;
    let rows = ((design::CLIP_LIST_H + offset) / stride).ceil() as usize + 1;
    let rows = rows.min(len.saturating_sub(first_row));
    ShelfScroll {
        first_row,
        rows,
        content_h,
        max_scroll,
        scroll,
    }
}

/// The history viewport in *window* coordinates, used for wheel ownership.
///
/// `flyout` is the card rect in that same space. The list geometry itself is
/// card-local, so the flyout origin is applied here — `Surface::shelf_viewport`
/// does the same for the shelf grid. Taking the origin as a parameter keeps the
/// two coordinate spaces from being mixed: with a zero origin the scroll region
/// is shifted by the flyout's offset and the wheel only works over part of the
/// list.
pub fn clipboard_viewport(flyout: RectD) -> RectD {
    RectD::new(
        flyout.x + design::CARD_CONTENT_X + design::CLIP_PAD,
        flyout.y + design::CLIP_PAD + design::CLIP_TOOLBAR_H + design::CLIP_TOOLBAR_GAP,
        design::CLIP_INNER_W - design::CLIP_SCROLL_W - design::CLIP_SCROLL_GAP,
        design::CLIP_LIST_H,
    )
}

/// Compact age for the row's right edge: now, 2m, 3h, 2d.
pub fn clip_age(created_at: i64, now: i64) -> String {
    let secs = (now - created_at).max(0);
    if secs < 60 {
        "now".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// Single-line preview: whitespace collapsed for text, the first file name
/// (plus a count) for file rows, dimensions for images.
pub fn clip_preview(item: &crate::clipboard::store::ClipItem) -> String {
    use crate::clipboard::store::ClipKind;
    match item.kind {
        ClipKind::Image => item
            .image
            .as_ref()
            .map(|meta| format!("{} \u{00d7} {}", meta.width, meta.height))
            .unwrap_or_default(),
        ClipKind::Files => {
            let first = item
                .paths
                .first()
                .map(|path| {
                    Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.clone())
                })
                .unwrap_or_default();
            if item.paths.len() > 1 {
                format!("{first} +{}", item.paths.len() - 1)
            } else {
                first
            }
        }
        _ => item.text.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// Full content for the row tooltip, capped at ~200 characters. Images show
/// dimensions and encoded size instead.
pub fn clip_tip(item: &crate::clipboard::store::ClipItem) -> String {
    use crate::clipboard::store::ClipKind;
    let full = match item.kind {
        ClipKind::Image => item
            .image
            .as_ref()
            .map(|meta| {
                format!(
                    "{} \u{00d7} {} \u{00b7} {}",
                    meta.width,
                    meta.height,
                    fmt_size(meta.bytes)
                )
            })
            .unwrap_or_default(),
        ClipKind::Files => item.paths.join(", "),
        _ => item.text.trim().to_string(),
    };
    if full.chars().count() > design::CLIP_TIP_CHARS {
        let mut cut: String = full.chars().take(design::CLIP_TIP_CHARS).collect();
        cut.push('\u{2026}');
        cut
    } else {
        full
    }
}

/// Local, deliberately simple heuristic: code punctuation density or a
/// keyword start. Only the preview font changes, never the content.
pub fn looks_like_code(text: &str) -> bool {
    let t = text.trim_start();
    const STARTERS: &[&str] = &[
        "fn ",
        "let ",
        "const ",
        "var ",
        "import ",
        "from ",
        "#include",
        "#!",
        "def ",
        "class ",
        "function ",
        "using ",
        "package ",
        "pub ",
        "impl ",
        "SELECT ",
        "select ",
        "<?",
        "<!",
    ];
    if STARTERS.iter().any(|s| t.starts_with(s)) {
        return true;
    }
    let punct = t
        .chars()
        .filter(|c| matches!(c, '{' | '}' | ';' | '(' | ')' | '=' | '[' | ']' | '<' | '>'))
        .count();
    punct >= 3
}

/// The toolbar count: capacity meter, or the pinned total under that filter.
pub fn clip_count_text(total: usize, pinned: usize, filter: ClipFilter) -> String {
    if filter == ClipFilter::Pinned {
        format!("{pinned} pinned")
    } else {
        format!("{total} / {}", design::CLIP_MAX_ITEMS)
    }
}

/// A row range covering the viewport, built from the filtered store so
/// indices stay absolute. Newest first, exactly the store's order.
pub fn clip_rows(
    items: &[crate::clipboard::store::ClipItem],
    filter: ClipFilter,
    scroll: f32,
    now: i64,
    thumb: &dyn Fn(&str) -> Option<Arc<AlbumArt>>,
) -> Vec<ClipRowView> {
    use crate::clipboard::store::ClipKind;
    let filtered: Vec<&crate::clipboard::store::ClipItem> =
        items.iter().filter(|item| filter.matches(item)).collect();
    let view = clip_scroll(filtered.len(), scroll);
    let mut rows = Vec::with_capacity(view.rows);
    for index in view.first_row..view.first_row + view.rows {
        let Some(item) = filtered.get(index) else {
            break;
        };
        let (icon, link, image) = match item.kind {
            ClipKind::Text => (IconId::TextLines, false, false),
            ClipKind::Link => (IconId::Link, true, false),
            ClipKind::Image => (IconId::Image, false, true),
            ClipKind::Files => (IconId::File, false, false),
        };
        let dimmed = item.kind == ClipKind::Files
            && !item.paths.is_empty()
            && item
                .paths
                .iter()
                .all(|path| std::fs::metadata(path).is_err());
        let mono = matches!(item.kind, ClipKind::Text) && looks_like_code(&item.text);
        // Secrets are only ever text-shaped; files and images have their own
        // previews (spec 3.6).
        let masked = matches!(item.kind, ClipKind::Text | ClipKind::Link)
            && crate::clipboard::secret::looks_secret(&item.text);
        let thumb = if image {
            thumb(&crate::clipboard::store::thumb_path(&item.digest).to_string_lossy())
        } else {
            None
        };
        rows.push(ClipRowView {
            index,
            id: item.id,
            icon,
            preview: clip_preview(item),
            mono,
            link,
            image,
            thumb,
            age: clip_age(item.created_at, now),
            expiring: item.expires_soon(now),
            pinned: item.pinned,
            dimmed,
            masked,
        });
    }
    rows
}

/// Cheap pre-check for the toolbar: the Native engine takes these raster
/// formats. The full probe still decides per file when Convert is pressed.
pub fn is_convertible(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico"
    )
}

/// Files whose thumbnail the worker can produce.
pub fn is_image(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tif" | "tiff"
    )
}

/// Interior of the expanded card: the content column after the rail and the
/// 1 DIP divider, inset by the card padding.
pub fn content_rect(fly: RectD) -> RectD {
    RectD::new(
        fly.x + design::CARD_CONTENT_X + design::CARD_PAD_X,
        fly.y + design::CARD_PAD_Y,
        design::CARD_CONTENT_W,
        design::CARD_CONTENT_H,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_pill_pops_then_idles() {
        let pop = clipboard_pill(3, Some(1), false);
        assert_eq!(pop.icon, IconId::Clipboard);
        assert_eq!(pop.label, "Clipboard");
        assert_eq!(pop.value, "+1");
        assert_eq!(pop.kind, PillKind::Clipboard { paused: false });
        assert_eq!(pop.tile.color, design::RAIL_CLIP);

        let idle = clipboard_pill(3, None, false);
        assert_eq!(idle.value, "3");
        let empty = clipboard_pill(0, None, false);
        assert!(empty.value.is_empty());
        assert_eq!(empty.kind, PillKind::Clipboard { paused: false });
        let paused = clipboard_pill(3, None, true);
        assert_eq!(paused.kind, PillKind::Clipboard { paused: true });
    }

    #[test]
    fn clock_formats() {
        assert_eq!(fmt_clock(84.0), "1:24");
        assert_eq!(fmt_clock(228.0), "3:48");
        assert_eq!(fmt_clock(3725.0), "1:02:05");
        assert_eq!(fmt_remaining(144.0), "-2:24");
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

    #[test]
    fn folder_tiles_show_a_dir_badge_and_selection() {
        let dir = std::env::temp_dir().join("taskbar-island-view-folder-test");
        std::fs::create_dir_all(&dir).unwrap();
        let paths = vec![dir.display().to_string()];
        let none = |_: &str| None;
        let tiles = shelf_tiles(&paths, &[0], 0.0, &none);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].index, 0);
        assert_eq!(tiles[0].badge, "DIR");
        assert_eq!(tiles[0].size, "Folder");
        assert!(tiles[0].selected);
        assert!(!shelf_tiles(&paths, &[], 0.0, &none)[0].selected);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rows_are_virtualized_and_keep_absolute_indices() {
        let paths: Vec<String> = (0..40).map(|i| format!("C:\\page-test-{i}.bin")).collect();
        let none = |_: &str| None;
        let top = shelf_tiles(&paths, &[5], 0.0, &none);
        assert!(top.len() <= 12, "only the visible rows are built");
        assert_eq!(top.iter().map(|t| t.index).min(), Some(0));
        assert_eq!(top[0].row, 0);
        assert_eq!(top[4].row, 1);
        assert_eq!(top[4].col, 0);
        assert!(top[5].selected);

        let view = shelf_scroll(paths.len(), 1000.0);
        assert_eq!(
            view.max_scroll,
            10.0 * design::SHELF_ROW_H - design::SHELF_VIEW_H
        );
        let bottom = shelf_tiles(&paths, &[], view.max_scroll, &none);
        assert_eq!(bottom.iter().map(|t| t.index).max(), Some(39));
        assert!(bottom.iter().all(|t| t.index < 40));
    }

    #[test]
    fn five_hundred_items_scroll_without_building_them_all() {
        let paths: Vec<String> = (0..500).map(|i| format!("C:\\big-{i}.bin")).collect();
        let none = |_: &str| None;
        // At most the rows touching the viewport plus the partially clipped
        // neighbours are built, never the 125 rows in the shelf.
        let max_per_frame =
            design::SHELF_COLS * ((design::SHELF_VIEW_H / design::SHELF_ROW_H).ceil() as usize + 2);
        for step in 0..100 {
            let scroll = step as f32 * 37.0;
            let tiles = shelf_tiles(&paths, &[0, 250, 499], scroll, &none);
            assert!(
                tiles.len() <= max_per_frame,
                "virtualized tiles per frame ({} built)",
                tiles.len()
            );
        }
        let end = shelf_scroll(500, f32::MAX);
        assert!(end.scroll <= end.max_scroll);
    }

    #[test]
    fn wheel_over_the_grid_scrolls_instead_of_switching_activity() {
        assert_eq!(
            wheel_action(true, true, true, true),
            WheelAction::ScrollList
        );
        // No scroll available, outside the grid, or another card: the wheel
        // must not switch activity underneath an open card.
        assert_eq!(wheel_action(true, true, true, false), WheelAction::Ignore);
        assert_eq!(wheel_action(true, true, false, true), WheelAction::Ignore);
        assert_eq!(wheel_action(true, false, false, true), WheelAction::Ignore);
        // A closed card keeps the original wheel-to-cycle gesture.
        assert_eq!(wheel_action(false, false, false, false), WheelAction::Cycle);
    }

    #[test]
    fn convertible_extensions_are_images_only() {
        assert!(is_convertible("C:\\a.PNG"));
        assert!(is_convertible("C:\\a.webp"));
        assert!(!is_convertible("C:\\a.pdf"));
        assert!(!is_convertible("C:\\a"));
    }

    #[test]
    fn empty_shelf_has_no_rows_and_no_scroll() {
        let none = |_: &str| None;
        let view = shelf_scroll(0, 0.0);
        assert_eq!(view.rows, 0);
        assert_eq!(view.content_h, 0.0);
        assert_eq!(view.max_scroll, 0.0);
        assert!(shelf_tiles(&[], &[], 0.0, &none).is_empty());
    }

    fn clip_item(
        id: u64,
        kind: crate::clipboard::store::ClipKind,
        created_at: i64,
    ) -> crate::clipboard::store::ClipItem {
        crate::clipboard::store::ClipItem {
            id,
            kind,
            digest: format!("d{id}"),
            created_at,
            pinned: false,
            text: String::new(),
            paths: Vec::new(),
            image: None,
        }
    }

    #[test]
    fn clip_rows_virtualize_thirty_mixed_items() {
        use crate::clipboard::store::ClipKind;
        let now = 1_000_000;
        let items: Vec<_> = (0..30)
            .map(|i| {
                let kind = match i % 4 {
                    0 => ClipKind::Text,
                    1 => ClipKind::Link,
                    2 => ClipKind::Image,
                    _ => ClipKind::Files,
                };
                clip_item(i, kind, now - i as i64 * 60)
            })
            .collect();
        let none = |_: &str| None;
        let max_per_frame = (design::CLIP_LIST_H / design::CLIP_ROW_STRIDE).ceil() as usize + 2;
        for step in 0..40 {
            let scroll = step as f32 * 17.0;
            let rows = clip_rows(&items, ClipFilter::All, scroll, now, &none);
            assert!(
                rows.len() <= max_per_frame,
                "virtualized rows per frame ({} built)",
                rows.len()
            );
        }
        let top = clip_rows(&items, ClipFilter::All, 0.0, now, &none);
        assert_eq!(top[0].index, 0);
        assert_eq!(top[0].id, 0);
        let end = clip_scroll(30, f32::MAX);
        assert!(end.scroll <= end.max_scroll);
        let bottom = clip_rows(&items, ClipFilter::All, end.max_scroll, now, &none);
        assert_eq!(bottom.iter().map(|r| r.index).max(), Some(29));
        assert!(bottom.iter().all(|r| r.index < 30));
    }

    #[test]
    fn clip_age_formats() {
        assert_eq!(clip_age(100, 100), "now");
        assert_eq!(clip_age(100, 159), "now");
        assert_eq!(clip_age(100, 160), "1m");
        assert_eq!(clip_age(100, 100 + 3599), "59m");
        assert_eq!(clip_age(100, 100 + 3600), "1h");
        assert_eq!(clip_age(100, 100 + 86_399), "23h");
        assert_eq!(clip_age(100, 100 + 86_400), "1d");
        assert_eq!(clip_age(100, 100 + 3 * 86_400), "3d");
    }

    #[test]
    fn code_detection_is_conservative() {
        assert!(looks_like_code("fn main() {"));
        assert!(looks_like_code("let x = 1;"));
        assert!(looks_like_code("import os"));
        assert!(looks_like_code("#include <stdio.h>"));
        assert!(looks_like_code("a(b) = {c[d];}"));
        assert!(!looks_like_code("hello world"));
        assert!(!looks_like_code("call me later"));
    }

    #[test]
    fn count_text_switches_under_pinned_filter() {
        assert_eq!(clip_count_text(10, 3, ClipFilter::All), "10 / 30");
        assert_eq!(clip_count_text(10, 3, ClipFilter::Text), "10 / 30");
        assert_eq!(clip_count_text(10, 3, ClipFilter::Pinned), "3 pinned");
        assert_eq!(clip_count_text(0, 0, ClipFilter::All), "0 / 30");
    }

    #[test]
    fn filters_match_their_kind_or_pin() {
        use crate::clipboard::store::ClipKind;
        let text = clip_item(1, ClipKind::Text, 0);
        let link = clip_item(2, ClipKind::Link, 0);
        let image = clip_item(3, ClipKind::Image, 0);
        let mut pinned = clip_item(4, ClipKind::Files, 0);
        pinned.pinned = true;
        assert!(ClipFilter::All.matches(&text));
        assert!(ClipFilter::All.matches(&pinned));
        assert!(ClipFilter::Text.matches(&text));
        assert!(!ClipFilter::Text.matches(&link));
        assert!(ClipFilter::Images.matches(&image));
        assert!(ClipFilter::Links.matches(&link));
        assert!(ClipFilter::Pinned.matches(&pinned));
        assert!(!ClipFilter::Pinned.matches(&text));
    }

    #[test]
    fn previews_collapse_text_and_name_files() {
        use crate::clipboard::store::ClipKind;
        let mut text = clip_item(1, ClipKind::Text, 0);
        text.text = "  hello\n\t  world  ".into();
        assert_eq!(clip_preview(&text), "hello world");

        let mut files = clip_item(2, ClipKind::Files, 0);
        files.paths = vec!["C:\\docs\\report.pdf".into(), "C:\\img\\photo.png".into()];
        assert_eq!(clip_preview(&files), "report.pdf +1");

        let mut image = clip_item(3, ClipKind::Image, 0);
        image.image = Some(crate::clipboard::store::ImageMeta {
            width: 1920,
            height: 1080,
            bytes: 2048,
            thumb: true,
        });
        assert_eq!(clip_preview(&image), "1920 \u{00d7} 1080");
    }

    #[test]
    fn tips_cap_long_text_and_describe_images() {
        use crate::clipboard::store::ClipKind;
        let mut text = clip_item(1, ClipKind::Text, 0);
        text.text = "x".repeat(500);
        let tip = clip_tip(&text);
        assert_eq!(tip.chars().count(), design::CLIP_TIP_CHARS + 1);
        assert!(tip.ends_with('\u{2026}'));

        let mut image = clip_item(2, ClipKind::Image, 0);
        image.image = Some(crate::clipboard::store::ImageMeta {
            width: 1920,
            height: 1080,
            bytes: 2 * 1024 * 1024,
            thumb: true,
        });
        let tip = clip_tip(&image);
        assert!(tip.contains("1920"));
        assert!(tip.contains("2.0 MB"));
    }

    #[test]
    fn wheel_in_clipboard_list_scrolls_not_switches() {
        // Open clipboard card, pointer over the list, scroll available.
        assert_eq!(
            wheel_action(true, true, true, true),
            WheelAction::ScrollList
        );
        // Over the toolbar or with nothing to scroll: ignored, never cycle.
        assert_eq!(wheel_action(true, true, false, true), WheelAction::Ignore);
        assert_eq!(wheel_action(true, true, true, false), WheelAction::Ignore);
    }

    #[test]
    fn clipboard_viewport_is_window_space_not_card_space() {
        // The card sits at (+60, +40) inside the island window: WINDOW_W 520
        // against FLY_W 400 centres it horizontally and WINDOW_TOP_PAD 40 lifts
        // it off the top. WM_MOUSEWHEEL hands `wheel` window coordinates, so
        // the scroll region has to be built in that space.
        let flyout = RectD::new(60.0, 40.0, design::FLY_W, design::FLY_H);
        let view = clipboard_viewport(flyout);
        let card_x = design::CARD_CONTENT_X + design::CLIP_PAD;
        let card_y = design::CLIP_PAD + design::CLIP_TOOLBAR_H + design::CLIP_TOOLBAR_GAP;
        // The list's top-left, in window coordinates, is inside the region.
        assert!(view.contains(flyout.x + card_x + 1.0, flyout.y + card_y + 1.0));
        // The same point read card-locally is outside it. Mixing the two
        // spaces is the regression that made the wheel work only over part of
        // the list: the region was shifted by the flyout's origin.
        assert!(!view.contains(card_x + 1.0, card_y + 1.0));
    }
}
