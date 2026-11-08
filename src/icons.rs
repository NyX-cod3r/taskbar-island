//! icons.rs — the SVG-path icons from section 9 of the spec.
//!
//! Paths are kept verbatim so they can be diffed against the document. A small
//! parser converts M/L/H/V/C/Q/A/Z (absolute and relative) into a Direct2D
//! geometry. Everything is device independent; callers scale the geometry.

use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{ID2D1Factory, ID2D1PathGeometry};
use windows_numerics::Vector2;

use crate::design::{rgb, Color};
use crate::view::IconId;

pub struct IconSpec {
    pub path: &'static str,
    pub filled: bool,
    /// Stroke width in the 24 x 24 viewBox.
    pub stroke: f32,
}

pub fn spec(icon: IconId) -> IconSpec {
    match icon {
        IconId::Music => IconSpec {
            path: "M9 18V6l10-2v12M9 18a3 3 0 1 1-3-3 3 3 0 0 1 3 3zm10-2a3 3 0 1 1-3-3 3 3 0 0 1 3 3z",
            filled: false,
            stroke: 2.0,
        },
        IconId::Phone => IconSpec {
            path: "M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2z",
            filled: false,
            stroke: 2.0,
        },
        IconId::Folder => IconSpec {
            path: "M3.5 7a2 2 0 0 1 2-2H10l2 2.5h6.5a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z",
            filled: false,
            stroke: 2.0,
        },
        IconId::Clock => IconSpec {
            path: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2",
            filled: false,
            stroke: 2.0,
        },
        IconId::Speaker => IconSpec {
            path: "M4 9.5v5h3.5l4.5 4v-13l-4.5 4zM16 9a4 4 0 0 1 0 6M18.5 6.5a8 8 0 0 1 0 11",
            filled: false,
            stroke: 2.0,
        },
        IconId::SpeakerMuted => IconSpec {
            path: "M4 9.5v5h3.5l4.5 4v-13l-4.5 4zM16 9a4 4 0 0 1 0 6M18.5 6.5a8 8 0 0 1 0 11M3 3l18 18",
            filled: false,
            stroke: 2.0,
        },
        IconId::DownArrow => IconSpec {
            path: "M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14",
            filled: false,
            stroke: 2.0,
        },
        IconId::Mic => IconSpec {
            path: "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3 3 3 0 0 1-3-3V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3",
            filled: false,
            stroke: 2.0,
        },
        IconId::MicMuted => IconSpec {
            path: "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3 3 3 0 0 1-3-3V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3M4 4l16 16",
            filled: false,
            stroke: 2.0,
        },
        IconId::Signal => IconSpec {
            path: "M4 20v-4M10 20V11M16 20V7M22 20V3",
            filled: false,
            stroke: 2.0,
        },
        IconId::Shuffle => IconSpec {
            path: "M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5",
            filled: false,
            stroke: 2.0,
        },
        IconId::Repeat => IconSpec {
            path: "M17 2l4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3",
            filled: false,
            stroke: 2.0,
        },
        IconId::Play => IconSpec {
            path: "M7.5 4.5v15l12-7.5z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Pause => IconSpec {
            path: "M6.5 4.5h4v15h-4zM13.5 4.5h4v15h-4z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Previous => IconSpec {
            path: "M6 5h2.2v14H6zM20 5.5v13L9.5 12z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Next => IconSpec {
            path: "M15.8 5H18v14h-2.2zM4 5.5v13L14.5 12z",
            filled: true,
            stroke: 0.0,
        },
        IconId::Close => IconSpec {
            path: "M6 6l12 12M18 6L6 18",
            filled: false,
            stroke: 2.0,
        },
        // v0.4 rail glyphs (24 x 24 grid).
        IconId::RailMusic => IconSpec {
            path: "M9 18V5l12-2v13M3 18a3 3 0 1 0 6 0a3 3 0 1 0-6 0zM15 16a3 3 0 1 0 6 0a3 3 0 1 0-6 0z",
            filled: false,
            stroke: 2.0,
        },
        IconId::RailTimer => IconSpec {
            path: "M4.5 13.5a7.5 7.5 0 1 0 15 0a7.5 7.5 0 1 0-15 0zM12 13.5V9.5M9.5 2.5h5M12 2.5v3M18.5 7l1.5-1.5",
            filled: false,
            stroke: 2.0,
        },
        IconId::RailCall => IconSpec {
            path: "M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z",
            filled: false,
            stroke: 2.0,
        },
        IconId::RailShelf => IconSpec {
            path: "M3.5 14.5v4a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-4M8 3.5h5.5L17 7v7H8z",
            filled: false,
            stroke: 2.0,
        },
        // v0.7 clipboard: clip rect plus the board outline.
        IconId::Clipboard => IconSpec {
            path: "M9 3h6a1 1 0 0 1 1 1v2a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM16 5h2a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2",
            filled: false,
            stroke: 2.0,
        },
        IconId::Tray => IconSpec {
            path: "M3.5 14.5v4a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-4M12 3.5v11M8 10.5l4 4 4-4",
            filled: false,
            stroke: 2.0,
        },
        IconId::Convert => IconSpec {
            path: "M4 8h13M13 4l4 4-4 4M20 16H7M11 12l-4 4 4 4",
            filled: false,
            stroke: 2.0,
        },
        IconId::Check => IconSpec {
            path: "M4.5 12.5l5 5L20 6.5",
            filled: false,
            stroke: 2.4,
        },
        IconId::Image => IconSpec {
            path: "M3.5 5.5h17v13h-17zM3.5 15l4.5-4 4 3.5 3.5-3 5 5M9 9.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z",
            filled: false,
            stroke: 2.0,
        },
        IconId::Video => IconSpec {
            path: "M3.5 7.5h11v9h-11zM14.5 11l5.5-3v8l-5.5-3z",
            filled: false,
            stroke: 2.0,
        },
        IconId::File => IconSpec {
            path: "M6 3.5h7l5 5v12H6zM13 3.5v5h5",
            filled: false,
            stroke: 2.0,
        },
        IconId::Compress => IconSpec {
            path: "M9 15l-4 4M5 15v4h4M15 9l4-4M19 5v4h-4",
            filled: false,
            stroke: 2.0,
        },
        IconId::Resize => IconSpec {
            path: "M8.5 4.5h-4v4M15.5 4.5h4v4M8.5 19.5h-4v-4M15.5 19.5h4v-4M9 9h6v6H9z",
            filled: false,
            stroke: 1.9,
        },
        IconId::EyeOff => IconSpec {
            path: "M2.5 12s3.5-5.5 9.5-5.5S21.5 12 21.5 12s-3.5 5.5-9.5 5.5S2.5 12 2.5 12zM9.7 9.7a3.2 3.2 0 0 0 4.6 4.6M4 4l16 16",
            filled: false,
            stroke: 1.9,
        },
        IconId::More => IconSpec {
            path: "M6 12h.01M12 12h.01M18 12h.01",
            filled: false,
            stroke: 3.2,
        },
        IconId::Back => IconSpec {
            path: "M14.5 5L8 12l6.5 7",
            filled: false,
            stroke: 2.0,
        },
        IconId::Copy => IconSpec {
            path: "M8 8h11v11H8zM5.5 16H4V4h11.5v1.5",
            filled: false,
            stroke: 1.9,
        },
        IconId::Undo => IconSpec {
            path: "M9 5L4 10l5 5M4 10h9a6 6 0 0 1 6 6v2",
            filled: false,
            stroke: 2.0,
        },
        IconId::Plus => IconSpec {
            path: "M12 5v14M5 12h14",
            filled: false,
            stroke: 2.2,
        },
        // v0.6 shelf toolbar.
        IconId::SelectAll => IconSpec {
            path: "M2 12.5l4.5 4.5L15 8.5M9 14.5l3.5 3.5L21.5 6",
            filled: false,
            stroke: 2.0,
        },
        IconId::Trash => IconSpec {
            path: "M4 7h16M9.5 7V5h5v2M6 7l1 13h10l1-13M10 11v5M14 11v5",
            filled: false,
            stroke: 1.9,
        },
        // v0.6 convert option glyphs (24 x 24 grid).
        IconId::Edit => IconSpec {
            path: "M4 20h4l10-10-4-4-10 10zM13.5 6.5l4 4",
            filled: false,
            stroke: 1.9,
        },
        IconId::Extract => IconSpec {
            path: "M12 3v10M8 9.5l4 4 4-4M4.5 15v4a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-4",
            filled: false,
            stroke: 2.0,
        },
        IconId::TextLines => IconSpec {
            path: "M5 6h14M5 10h14M5 14h9M5 18h6",
            filled: false,
            stroke: 2.0,
        },
        IconId::Fit => IconSpec {
            path: "M9 4H4v5M15 4h5v5M9 20H4v-5M15 20h5v-5",
            filled: false,
            stroke: 2.0,
        },
        IconId::Thumbnail => IconSpec {
            path: "M4 4h16v16H4zM7.5 7.5h5v5h-5zM14.5 8h2M14.5 11h2M7.5 15.5h9",
            filled: false,
            stroke: 1.8,
        },
        IconId::Web => IconSpec {
            path: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c2.5 2.5 3.8 5.6 3.8 9S14.5 18.5 12 21M12 3c-2.5 2.5-3.8 5.6-3.8 9S9.5 18.5 12 21",
            filled: false,
            stroke: 1.8,
        },
        IconId::RotateLeft => IconSpec {
            path: "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8M3 3v5h5",
            filled: false,
            stroke: 1.9,
        },
        IconId::RotateRight => IconSpec {
            path: "M21 12a9 9 0 1 1-9-9 9.75 9.75 0 0 1 6.74 2.74L21 8M21 3v5h-5",
            filled: false,
            stroke: 1.9,
        },
        IconId::FlipHorizontal => IconSpec {
            path: "M8 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h3M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3M12 2v2M12 8v2M12 14v2M12 20v2",
            filled: false,
            stroke: 1.8,
        },
        IconId::FlipVertical => IconSpec {
            path: "M21 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v3M21 16v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3M2 12h2M8 12h2M14 12h2M20 12h2",
            filled: false,
            stroke: 1.8,
        },
        IconId::Grayscale => IconSpec {
            path: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9 7.5h.01M13.5 7.5h.01M7.5 12h.01M12 12h.01M16.5 12h.01M9 16.5h.01M13.5 16.5h.01",
            filled: false,
            stroke: 2.4,
        },
        IconId::Contrast => IconSpec {
            path: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 3v18",
            filled: false,
            stroke: 2.0,
        },
        IconId::Crop => IconSpec {
            path: "M6 2v14a2 2 0 0 0 2 2h14M18 22V8a2 2 0 0 0-2-2H2",
            filled: false,
            stroke: 1.9,
        },
        IconId::Speed => IconSpec {
            path: "M4 18a8 8 0 1 1 16 0M12 18l4-6",
            filled: false,
            stroke: 2.0,
        },
        IconId::Save => IconSpec {
            path: "M5 3.5h11l3.5 3.5v13.5H5zM8 3.5v5h7v-5M8 20.5v-6h8v6",
            filled: false,
            stroke: 1.9,
        },
        // v0.7 clipboard: pushpin, chain link and the All filter grid.
        IconId::Pin => IconSpec {
            path: "M8 3h8M9 3v6l-3 4v2h12v-2l-3-4V3M12 15v6",
            filled: false,
            stroke: 1.9,
        },
        IconId::Link => IconSpec {
            path: "M9.5 14.5l5-5M8 11l-2.5 2.5a3.54 3.54 0 0 0 5 5L13 16M11 8l2.5-2.5a3.54 3.54 0 0 1 5 5L16 13",
            filled: false,
            stroke: 1.9,
        },
        IconId::GridAll => IconSpec {
            path: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
            filled: false,
            stroke: 1.8,
        },
    }
}

// -------------------------------------------------------------- app icons

/// The four activity identity tiles (call.svg, music.svg, shelf.svg,
/// timer.svg): a rounded gradient square with a white glyph on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppIcon {
    Music,
    Call,
    Shelf,
    Timer,
}

impl AppIcon {
    pub fn of(icon: IconId) -> Option<Self> {
        match icon {
            IconId::Music => Some(Self::Music),
            IconId::Phone => Some(Self::Call),
            IconId::Folder => Some(Self::Shelf),
            IconId::Clock => Some(Self::Timer),
            _ => None,
        }
    }
}

pub struct AppGlyph {
    /// Path in the 64 x 64 icon space.
    pub path: &'static str,
    /// Solid white fill opacity (0 = stroke only).
    pub fill: f32,
    /// Stroke width in the 64 x 64 icon space (0 = fill only).
    pub stroke: f32,
}

pub struct AppIconSpec {
    pub top: Color,
    pub bottom: Color,
    /// White sheen over the upper half (0 = none).
    pub shine: f32,
    /// Corner radius in the 64 x 64 icon space.
    pub radius: f32,
    /// Optional (scale, dx, dy) baked into the glyph paths.
    pub xform: Option<(f32, f32, f32)>,
    pub glyph: &'static [AppGlyph],
}

const MUSIC_GLYPH: &[AppGlyph] = &[
    AppGlyph {
        path: "M26 41V22.5l17-4.5V36.5",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M26 29.5l17-4.5",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M16 41.5a5.5 4.5 0 1 0 11 0a5.5 4.5 0 1 0-11 0z",
        fill: 1.0,
        stroke: 0.0,
    },
    AppGlyph {
        path: "M33 37a5.5 4.5 0 1 0 11 0a5.5 4.5 0 1 0-11 0z",
        fill: 1.0,
        stroke: 0.0,
    },
];

const CALL_GLYPH: &[AppGlyph] = &[AppGlyph {
    path: "M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z",
    fill: 1.0,
    stroke: 1.6,
}];

const SHELF_GLYPH: &[AppGlyph] = &[
    AppGlyph {
        path: "M15 36v8a4 4 0 0 0 4 4h26a4 4 0 0 0 4-4v-8",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M25 13h10l6 6v17H25z",
        fill: 0.22,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M35 13v6h6",
        fill: 0.0,
        stroke: 3.6,
    },
];

const TIMER_GLYPH: &[AppGlyph] = &[
    AppGlyph {
        path: "M16 35a16 16 0 1 0 32 0a16 16 0 1 0-32 0z",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M27 12.5h10",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M32 12.5V19",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M44.5 22.5l3-3",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M32 35V26.5",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M32 35l6 4",
        fill: 0.0,
        stroke: 3.6,
    },
    AppGlyph {
        path: "M29.6 35a2.4 2.4 0 1 0 4.8 0a2.4 2.4 0 1 0-4.8 0z",
        fill: 1.0,
        stroke: 0.0,
    },
];

pub fn app_spec(icon: AppIcon) -> AppIconSpec {
    match icon {
        AppIcon::Music => AppIconSpec {
            top: rgb(0xFF, 0x8F, 0xA8),
            bottom: rgb(0xF0, 0x45, 0x7A),
            shine: 0.28,
            radius: 16.0,
            xform: None,
            glyph: MUSIC_GLYPH,
        },
        AppIcon::Call => AppIconSpec {
            top: rgb(0x5B, 0xE3, 0x8F),
            bottom: rgb(0x18, 0xB2, 0x5B),
            shine: 0.28,
            radius: 16.0,
            // call.svg draws a 24-unit glyph through translate(16 16) scale(1.333).
            xform: Some((1.333, 16.0, 16.0)),
            glyph: CALL_GLYPH,
        },
        AppIcon::Shelf => AppIconSpec {
            top: rgb(0x8F, 0xA6, 0xD6),
            bottom: rgb(0x54, 0x68, 0xA3),
            shine: 0.28,
            radius: 16.0,
            xform: None,
            glyph: SHELF_GLYPH,
        },
        AppIcon::Timer => AppIconSpec {
            top: rgb(0xFF, 0xC2, 0x4D),
            bottom: rgb(0xF2, 0x8A, 0x0F),
            shine: 0.30,
            radius: 16.0,
            xform: None,
            glyph: TIMER_GLYPH,
        },
    }
}

// ------------------------------------------------------------------ parser

#[derive(Clone)]
enum Seg {
    Line(Vector2),
    Cubic(Vector2, Vector2, Vector2),
}

struct Subpath {
    start: Vector2,
    segs: Vec<Seg>,
    closed: bool,
}

struct Lexer<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    fn skip_sep(&mut self) {
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i];
            if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' || c == b',' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip_sep();
        let c = *self.bytes.get(self.i)?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c)
        } else {
            None
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_sep();
        let start = self.i;
        if matches!(self.bytes.get(self.i), Some(b'+') | Some(b'-')) {
            self.i += 1;
        }
        let mut digits = false;
        while matches!(self.bytes.get(self.i), Some(b'0'..=b'9')) {
            self.i += 1;
            digits = true;
        }
        if self.bytes.get(self.i) == Some(&b'.') {
            self.i += 1;
            while matches!(self.bytes.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
                digits = true;
            }
        }
        if !digits {
            self.i = start;
            return None;
        }
        if matches!(self.bytes.get(self.i), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.bytes.get(self.i), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            while matches!(self.bytes.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}

fn parse(path: &str) -> Vec<Subpath> {
    let mut lex = Lexer::new(path);
    let mut out: Vec<Subpath> = Vec::new();
    let mut cur = Vector2::new(0.0, 0.0);
    let mut sub_start = cur;
    let mut open: Option<Subpath> = None;
    // Reflect the previous cubic's second control point for `S`/`s`.
    let mut prev_ctrl: Option<Vector2> = None;
    let mut cmd = b'M';

    while let Some(c) = lex.command() {
        cmd = c;
        loop {
            // Z consumes no coordinates. It must be handled before the
            // next-command peek below: peeking first skips the Z, leaving the
            // figure open, and D2D does not fill open (hollow) figures — the
            // pause glyph lost its first bar that way.
            if cmd.eq_ignore_ascii_case(&b'Z') {
                if let Some(mut p) = open.take() {
                    p.closed = true;
                    out.push(p);
                }
                cur = sub_start;
                prev_ctrl = None;
                break;
            }
            lex.skip_sep();
            if lex.command().is_some() {
                // Put the command back by remembering it; the outer loop
                // expects to consume it. The lexer advanced, so restore.
                lex.i -= 1;
                break;
            }
            let rel = cmd.is_ascii_lowercase();
            let base = if rel { cur } else { Vector2::new(0.0, 0.0) };
            match cmd.to_ascii_uppercase() {
                b'M' => {
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    if let Some(mut p) = open.take() {
                        p.closed = false;
                        out.push(p);
                    }
                    cur = Vector2::new(base.X + x, base.Y + y);
                    sub_start = cur;
                    prev_ctrl = None;
                    open = Some(Subpath {
                        start: cur,
                        segs: Vec::new(),
                        closed: false,
                    });
                    // Additional pairs after M are implicit lines.
                    cmd = if rel { b'l' } else { b'L' };
                }
                b'L' => {
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let to = Vector2::new(base.X + x, base.Y + y);
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Line(to));
                    }
                    cur = to;
                    prev_ctrl = None;
                }
                b'H' => {
                    let Some(x) = lex.number() else { break };
                    let to = Vector2::new(if rel { cur.X + x } else { x }, cur.Y);
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Line(to));
                    }
                    cur = to;
                    prev_ctrl = None;
                }
                b'V' => {
                    let Some(y) = lex.number() else { break };
                    let to = Vector2::new(cur.X, if rel { cur.Y + y } else { y });
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Line(to));
                    }
                    cur = to;
                    prev_ctrl = None;
                }
                b'C' => {
                    let Some(x1) = lex.number() else { break };
                    let Some(y1) = lex.number() else { break };
                    let Some(x2) = lex.number() else { break };
                    let Some(y2) = lex.number() else { break };
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let c1 = Vector2::new(base.X + x1, base.Y + y1);
                    let c2 = Vector2::new(base.X + x2, base.Y + y2);
                    let to = Vector2::new(base.X + x, base.Y + y);
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Cubic(c1, c2, to));
                    }
                    cur = to;
                    prev_ctrl = Some(c2);
                }
                b'S' => {
                    let Some(x2) = lex.number() else { break };
                    let Some(y2) = lex.number() else { break };
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let c1 = prev_ctrl.unwrap_or(cur);
                    let c2 = Vector2::new(base.X + x2, base.Y + y2);
                    let to = Vector2::new(base.X + x, base.Y + y);
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Cubic(c1, c2, to));
                    }
                    cur = to;
                    prev_ctrl = Some(c2);
                }
                b'Q' => {
                    let Some(qx) = lex.number() else { break };
                    let Some(qy) = lex.number() else { break };
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let q = Vector2::new(base.X + qx, base.Y + qy);
                    let to = Vector2::new(base.X + x, base.Y + y);
                    let c1 = Vector2::new(
                        cur.X + (q.X - cur.X) * 2.0 / 3.0,
                        cur.Y + (q.Y - cur.Y) * 2.0 / 3.0,
                    );
                    let c2 = Vector2::new(
                        to.X + (q.X - to.X) * 2.0 / 3.0,
                        to.Y + (q.Y - to.Y) * 2.0 / 3.0,
                    );
                    if let Some(p) = open.as_mut() {
                        p.segs.push(Seg::Cubic(c1, c2, to));
                    }
                    cur = to;
                    prev_ctrl = None;
                }
                b'A' => {
                    let Some(rx) = lex.number() else { break };
                    let Some(ry) = lex.number() else { break };
                    let Some(rot) = lex.number() else { break };
                    let Some(large) = lex.number() else { break };
                    let Some(sweep) = lex.number() else { break };
                    let Some(x) = lex.number() else { break };
                    let Some(y) = lex.number() else { break };
                    let to = Vector2::new(base.X + x, base.Y + y);
                    let mut arc = Vec::new();
                    arc_to_cubics(cur, rx, ry, rot, large != 0.0, sweep != 0.0, to, &mut arc);
                    if let Some(p) = open.as_mut() {
                        for seg in arc {
                            p.segs.push(seg);
                        }
                    }
                    cur = to;
                    prev_ctrl = None;
                }
                b'Z' => break,
                _ => break,
            }
            // Repeat the same command for its remaining number groups; the
            // peek above stops as soon as a new command starts.
        }
    }
    if let Some(mut p) = open.take() {
        p.closed = false;
        out.push(p);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    p0: Vector2,
    rx: f32,
    ry: f32,
    rotation_deg: f32,
    large: bool,
    sweep: bool,
    p1: Vector2,
    out: &mut Vec<Seg>,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx <= f32::EPSILON || ry <= f32::EPSILON {
        out.push(Seg::Line(p1));
        return;
    }
    let phi = rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let dx = (p0.X - p1.X) / 2.0;
    let dy = (p0.Y - p1.Y) / 2.0;
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;

    let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    // SVG F.6.5: +1 when the flags differ, -1 when they match.
    let sign = if large != sweep { 1.0 } else { -1.0 };
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let coef = sign * (num.max(0.0) / den.max(f32::EPSILON)).sqrt();
    let cxp = coef * rx * y1 / ry;
    let cyp = -coef * ry * x1 / rx;
    let cx = cos_phi * cxp - sin_phi * cyp + (p0.X + p1.X) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (p0.Y + p1.Y) / 2.0;

    let ux = (x1 - cxp) / rx;
    let uy = (y1 - cyp) / ry;
    let vx = (-x1 - cxp) / rx;
    let vy = (-y1 - cyp) / ry;
    let theta = uy.atan2(ux);
    let mut delta = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let segments = ((delta.abs() / (std::f32::consts::FRAC_PI_2)).ceil() as usize).max(1);
    let step = delta / segments as f32;
    let mut angle = theta;
    for _ in 0..segments {
        let next = angle + step;
        let alpha = 4.0 / 3.0 * (step / 4.0).tan();
        let (sa, ca) = angle.sin_cos();
        let (sn, cn) = next.sin_cos();
        let c1 = Vector2::new(cx + rx * (ca - alpha * sa), cy + ry * (sa + alpha * ca));
        let c2 = Vector2::new(cx + rx * (cn + alpha * sn), cy + ry * (sn - alpha * cn));
        let to = Vector2::new(cx + rx * cn, cy + ry * sn);
        out.push(Seg::Cubic(c1, c2, to));
        angle = next;
    }
}

pub fn build(factory: &ID2D1Factory, icon: IconId) -> Result<ID2D1PathGeometry> {
    let subpaths = parse(spec(icon).path);
    build_subpaths(factory, &subpaths)
}

/// Builds one layer of an app icon. Glyph coordinates stay in the SVG's
/// 64-unit space; the optional `g` transform is baked in here.
pub fn build_app(factory: &ID2D1Factory, icon: AppIcon, layer: usize) -> Result<ID2D1PathGeometry> {
    let spec = app_spec(icon);
    let mut subpaths = parse(spec.glyph[layer].path);
    if let Some((s, tx, ty)) = spec.xform {
        for sub in &mut subpaths {
            sub.start = Vector2::new(sub.start.X * s + tx, sub.start.Y * s + ty);
            for seg in &mut sub.segs {
                match seg {
                    Seg::Line(p) => *p = Vector2::new(p.X * s + tx, p.Y * s + ty),
                    Seg::Cubic(a, b, c) => {
                        *a = Vector2::new(a.X * s + tx, a.Y * s + ty);
                        *b = Vector2::new(b.X * s + tx, b.Y * s + ty);
                        *c = Vector2::new(c.X * s + tx, c.Y * s + ty);
                    }
                }
            }
        }
    }
    build_subpaths(factory, &subpaths)
}

fn build_subpaths(factory: &ID2D1Factory, subpaths: &[Subpath]) -> Result<ID2D1PathGeometry> {
    let geometry = unsafe { factory.CreatePathGeometry() }?;
    let sink = unsafe { geometry.Open() }?;
    // SAFETY: the sink belongs to `geometry` and is closed below.
    unsafe {
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        for sub in subpaths {
            sink.BeginFigure(
                sub.start,
                if sub.closed {
                    D2D1_FIGURE_BEGIN_FILLED
                } else {
                    D2D1_FIGURE_BEGIN_HOLLOW
                },
            );
            for seg in &sub.segs {
                match seg {
                    Seg::Line(p) => sink.AddLine(*p),
                    Seg::Cubic(c1, c2, p) => {
                        let bezier = D2D1_BEZIER_SEGMENT {
                            point1: *c1,
                            point2: *c2,
                            point3: *p,
                        };
                        sink.AddBezier(&bezier);
                    }
                }
            }
            sink.EndFigure(if sub.closed {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
        }
        sink.Close()?;
    }
    Ok(geometry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_glyph_keeps_both_bars() {
        let subs = parse(spec(IconId::Pause).path);
        assert_eq!(
            subs.len(),
            2,
            "pause must keep two bars, not merge into one"
        );
        for (i, sub) in subs.iter().enumerate() {
            assert_eq!(sub.segs.len(), 3, "bar {i} segments");
            assert!(sub.closed, "bar {i} closed");
        }
    }

    #[test]
    fn every_icon_parses_to_a_subpath() {
        for icon in [
            IconId::Music,
            IconId::Phone,
            IconId::Folder,
            IconId::Clock,
            IconId::Speaker,
            IconId::SpeakerMuted,
            IconId::DownArrow,
            IconId::Mic,
            IconId::MicMuted,
            IconId::Signal,
            IconId::Shuffle,
            IconId::Repeat,
            IconId::Play,
            IconId::Pause,
            IconId::Previous,
            IconId::Next,
            IconId::Close,
            IconId::RailMusic,
            IconId::RailTimer,
            IconId::RailCall,
            IconId::RailShelf,
            IconId::Tray,
            IconId::Convert,
            IconId::Check,
            IconId::Image,
            IconId::Video,
            IconId::File,
            IconId::Compress,
            IconId::Resize,
            IconId::EyeOff,
            IconId::More,
            IconId::Copy,
            IconId::Undo,
            IconId::Plus,
            IconId::Back,
            IconId::SelectAll,
            IconId::Trash,
            IconId::Edit,
            IconId::Extract,
            IconId::TextLines,
            IconId::Fit,
            IconId::Thumbnail,
            IconId::Web,
            IconId::RotateLeft,
            IconId::RotateRight,
            IconId::FlipHorizontal,
            IconId::FlipVertical,
            IconId::Grayscale,
            IconId::Contrast,
            IconId::Crop,
            IconId::Speed,
            IconId::Save,
            IconId::Clipboard,
            IconId::Pin,
            IconId::Link,
            IconId::GridAll,
        ] {
            let subpaths = parse(spec(icon).path);
            assert!(!subpaths.is_empty(), "no subpaths for {icon:?}");
            assert!(
                subpaths.iter().any(|p| !p.segs.is_empty()),
                "no segments for {icon:?}"
            );
        }
    }

    #[test]
    fn relative_and_absolute_arcs_end_where_requested() {
        let subs = parse("M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z");
        assert_eq!(subs.len(), 1);
        let last = subs[0].segs.last().unwrap();
        match last {
            Seg::Cubic(_, _, p) => {
                assert!((p.X - 12.0).abs() < 0.1);
                assert!((p.Y - 3.0).abs() < 0.1);
            }
            _ => panic!("expected cubic"),
        }
    }

    #[test]
    fn every_app_glyph_layer_parses() {
        for app in [
            AppIcon::Music,
            AppIcon::Call,
            AppIcon::Shelf,
            AppIcon::Timer,
        ] {
            let spec = app_spec(app);
            assert!(!spec.glyph.is_empty(), "no layers for {app:?}");
            for (i, layer) in spec.glyph.iter().enumerate() {
                let subs = parse(layer.path);
                assert!(!subs.is_empty(), "no subpaths for {app:?} layer {i}");
                assert!(
                    subs.iter().any(|p| !p.segs.is_empty()),
                    "no segments for {app:?} layer {i}"
                );
            }
        }
    }
}

#[cfg(test)]
mod app_glyph_bounds {
    use super::*;

    fn bounds(path: &str) -> (f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for s in &parse(path) {
            let mut add = |p: Vector2| {
                b.0 = b.0.min(p.X);
                b.1 = b.1.min(p.Y);
                b.2 = b.2.max(p.X);
                b.3 = b.3.max(p.Y);
            };
            add(s.start);
            for seg in &s.segs {
                match seg {
                    Seg::Line(p) => add(*p),
                    Seg::Cubic(a, c, p) => {
                        add(*a);
                        add(*c);
                        add(*p);
                    }
                }
            }
        }
        b
    }

    /// Arcs with the wrong centre sign mirror their bulge and can fling a
    /// glyph far outside its own viewBox.
    #[test]
    fn glyphs_stay_inside_the_tile() {
        for app in [
            AppIcon::Music,
            AppIcon::Call,
            AppIcon::Shelf,
            AppIcon::Timer,
        ] {
            let spec = app_spec(app);
            for (i, layer) in spec.glyph.iter().enumerate() {
                let (x0, y0, x1, y1) = bounds(layer.path);
                assert!(
                    x0 >= 0.0 && y0 >= 0.0 && x1 <= 64.0 && y1 <= 64.0,
                    "{app:?} layer {i} escapes the 64-unit tile: x {x0}..{x1} y {y0}..{y1}"
                );
            }
        }
    }
}
