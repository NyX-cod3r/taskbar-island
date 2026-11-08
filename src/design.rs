//! design.rs — every size, timing and color from the TaskbarIsland v1.0 spec.
//!
//! This is the single source of truth. No other module may introduce a magic
//! dimension, duration or token color.

// ---------------------------------------------------------------- sizes (DIP)
/// Original compact footprint. The new renderer and flyout use this same
/// stable footprint so activity changes never create a second-sized pill.
pub const PILL_W: f32 = 224.0;
pub const PILL_H: f32 = 32.0;
pub const BUBBLE: f32 = 32.0;
pub const BUBBLE_GAP: f32 = 8.0;
pub const FLY_W: f32 = 400.0;
pub const FLY_H: f32 = 192.0;
/// Shared expanded-card content padding from the v0.8 design system.
pub const CARD_PAD: f32 = 7.0;
/// Vertical separation between card sections.
pub const CARD_GAP: f32 = 8.0;
/// Rail dimensions shared by every expanded card.
pub const RAIL_W: f32 = 56.0;
pub const RAIL_DIVIDER: f32 = 1.0;
pub const RAIL_TILE: f32 = 32.0;
pub const RAIL_GAP: f32 = 5.0;
pub const RAIL_ICON: f32 = 18.0;
pub const RAIL_INDICATOR_W: f32 = 3.0;
pub const RAIL_INDICATOR_H: f32 = 16.0;
/// Compatibility alias for pre-v0.8 cards. New layout code must use
/// `card_content_rect()` rather than deriving dimensions from this value.
pub const FLY_PAD: f32 = CARD_PAD;
pub const TOP_ROW_H: f32 = 56.0;
pub const ROW_GAP: f32 = 12.0;
pub const BODY_H: f32 = 92.0;
pub const LEAD_TILE: f32 = 56.0;
pub const PILL_TILE: f32 = 24.0;
pub const PILL_PAD_L: f32 = 4.0;
pub const PILL_PAD_R: f32 = 10.0;
pub const GAP_TRAY: f32 = 8.0;
/// The pill is inset by 4 DIP from the top and bottom of the taskbar band.
pub const PILL_BAND_INSET: f32 = 4.0;
/// Resting distance from the taskbar to the bottom edge of the expanded card.
pub const FLY_BOTTOM_GAP: f32 = 6.0;
pub const WINDOW_W: f32 = 520.0;
pub const WINDOW_TOP_PAD: f32 = 40.0;
pub const PILL_GAP: f32 = 8.0;
pub const PILL_TILE_GAP: f32 = 8.0;
pub const PILL_PROGRESS_H: f32 = 2.0;
pub const PILL_GLYPH: f32 = 14.0;
pub const LEAD_GLYPH: f32 = 26.0;

// ------------------------------------------------------- flyout internals
pub const FLY_CONTENT_W: f32 = FLY_W - RAIL_W - RAIL_DIVIDER - CARD_PAD * 2.0;
pub const CHIP_H: f32 = 30.0;
pub const CHIP_TILE: f32 = 20.0;
pub const R_CHIP_TILE: f32 = 5.0;
pub const CHIP_PAD_L: f32 = 6.0;
pub const CHIP_PAD_R: f32 = 10.0;
pub const CHIP_GAP: f32 = 6.0;
pub const CHIP_TILE_GAP: f32 = 6.0;
pub const TRAILING_GAP: f32 = 8.0;
pub const BUTTON_H: f32 = 40.0;
pub const BUTTON_H_SMALL: f32 = 36.0;
pub const BUTTON_PREV: f32 = 40.0;
pub const BUTTON_PLAY: f32 = 48.0;
pub const BUTTON_CALL: f32 = 44.0;
pub const BUTTON_GAP: f32 = 8.0;
pub const BUTTON_RADIUS: f32 = R_CONTROL;
pub const CONTROL_GLYPH_SMALL: f32 = 17.0;
pub const CONTROL_GLYPH: f32 = 20.0;
pub const CONTROL_GLYPH_PLAY: f32 = 22.0;
pub const CONTROL_GLYPH_CLOSE: f32 = 16.0;
pub const PROGRESS_LABEL_ELAPSED: f32 = 32.0;
pub const PROGRESS_LABEL_REMAIN: f32 = 36.0;
pub const PROGRESS_LABEL_GAP: f32 = 10.0;
pub const PROGRESS_H: f32 = 20.0;
pub const PROGRESS_STROKE: f32 = 3.0;
pub const MUSIC_BODY_H: f32 = 72.0;
pub const MUSIC_ROW2_H: f32 = 40.0;
pub const MUSIC_ROW_GAP: f32 = 12.0;
pub const CALL_BODY_TOP: f32 = 20.0;
pub const CALL_BODY_GAP: f32 = 12.0;
pub const CALL_BODY_H: f32 = 76.0;
pub const SHELF_TILE_H: f32 = 66.0;
pub const SHELF_TILE_GAP: f32 = 8.0;
pub const SHELF_TILE_PAD: f32 = 10.0;
pub const SHELF_HINT_H: f32 = 14.0;
pub const SHELF_HINT_GAP: f32 = 12.0;
pub const BADGE_PAD_X: f32 = 6.0;
pub const BADGE_PAD_Y: f32 = 2.0;
pub const TIMER_BODY_H: f32 = 72.0;
pub const TIMER_TOGGLE_W: f32 = 48.0;
pub const TIMER_STOP_W: f32 = 40.0;
pub const RING_CENTER: f32 = 28.0;
pub const RING_RADIUS: f32 = 19.0;
pub const RING_STROKE: f32 = 4.0;
pub const VOL_SLIDER_H: f32 = 4.0;
pub const VOL_VALUE_W: f32 = 22.0;
pub const VOL_VALUE_GAP: f32 = 8.0;
pub const SHEEN_ALPHA: f32 = 0.12;
pub const SHADOW_1_DY: f32 = 16.0;
pub const SHADOW_1_BLUR: f32 = 40.0;
pub const SHADOW_1_A: f32 = 0.45;
pub const SHADOW_2_DY: f32 = 2.0;
pub const SHADOW_2_BLUR: f32 = 8.0;
pub const SHADOW_2_A: f32 = 0.30;
pub const SHADOW_STEPS: usize = 12;
/// The click-through region may include a transparent halo so the soft
/// shadow can be painted; `WM_NCHITTEST` still returns `HTTRANSPARENT` outside
/// the interactive surfaces.
pub const SHADOW_HALO: f32 = 44.0;
pub const ICON_STROKE: f32 = 2.3;
pub const FOCUS_RING_W: f32 = 2.0;
pub const FOCUS_RING_GAP: f32 = 2.0;
pub const CHIP_LABEL_PAD: f32 = 4.0;
pub const PILL_TILE_Y: f32 = 4.0;
pub const BADGE_H: f32 = 18.0;
pub const RING_ARC_CAP: f32 = 0.0;
pub const SNAPSHOT_BG: Color = rgb(0x1E, 0x1E, 0x1E);
pub const AUTO_CLOSE_TIMER_MS: u32 = AUTO_CLOSE_MS;
pub const HUD_TIMER_MS: u32 = HUD_MS;
pub const TIMER_TICK_MS: u32 = 1000;
pub const FRAME_MS: u32 = 16;
pub const OPEN_TOP_FADE_MS: u32 = 280;
pub const OPEN_TOP_MOVE_MS: u32 = 400;
pub const OPEN_TRANSLATE: f32 = 8.0;

// ----------------------------------------------------------------- radii
pub const R_SURFACE: f32 = 6.0;
pub const R_CONTROL: f32 = 6.0;
pub const R_BADGE: f32 = 4.0;
pub const R_TILE_PILL: f32 = 6.0;
pub const R_TILE_LEAD: f32 = 8.0;
pub const R_THUMB: f32 = 2.0;

// ------------------------------------------------------------------ wave
pub const WAVE_LEN: f32 = 20.0;
pub const WAVE_AMP: f32 = 3.0;
pub const WAVE_STROKE: f32 = 3.0;
pub const WAVE_ROW_H: f32 = 20.0;
pub const WAVE_SPEED: f32 = 28.57;
pub const WAVE_FLAT: f32 = 0.02;
pub const WAVE_EASE_MS: u32 = 320;
pub const THUMB_W: f32 = 4.0;
pub const THUMB_H: f32 = 18.0;

// --------------------------------------------------------------- timings
pub const OPEN_MS: u32 = 460;
pub const CLOSE_MS: u32 = 260;
pub const FADE_IN_MS: u32 = 80;
pub const CONTENT_TOP_DELAY: u32 = 150;
pub const CONTENT_BODY_DELAY: u32 = 210;
pub const HOVER_MS: u32 = 160;
pub const SHEEN_MS: u32 = 800;
pub const PILL_FADE_MS: u32 = 200;
pub const AUTO_CLOSE_MS: u32 = 700;
pub const HUD_MS: u32 = 1800;
pub const PAUSED_KEEP_MS: u32 = 60_000;
pub const CONTENT_IN_MS: u32 = 280;
pub const CONTENT_IN_MOVE_MS: u32 = 400;
pub const CONTENT_SWAP_OUT_MS: u32 = 80;
pub const CONTENT_SWAP_MS: u32 = 120;
pub const PRESS_MS: u32 = 120;
pub const ICON_SWAP_MS: u32 = 220;
pub const CLOSE_FADE_START_MS: u32 = 180;
pub const CLOSE_FADE_MS: u32 = 90;
pub const TIMER_FLASH_MS: u32 = 600;
pub const TIMER_FLASHES: u32 = 3;
pub const DRAG_BOB_MS: u32 = 1000;
pub const HUD_FADE_MS: u32 = 200;
pub const PROGRESS_SMOOTH_MS: u32 = 1000;

// ----------------------------------------------------------------- curves
pub const EASE_OPEN: [f32; 4] = [0.2, 1.1, 0.3, 1.0];
pub const EASE_CLOSE: [f32; 4] = [0.4, 0.0, 0.2, 1.0];
pub const EASE_CONTENT: [f32; 4] = [0.2, 0.9, 0.25, 1.0];
pub const EASE_ICON: [f32; 4] = [0.3, 1.4, 0.5, 1.0];

// ---------------------------------------------------------------- colors
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    rgba(r, g, b, 1.0)
}

pub const fn white(a: f32) -> Color {
    Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a,
    }
}

pub const fn black(a: f32) -> Color {
    Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a,
    }
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub fn scale_alpha(self, k: f32) -> Self {
        Self {
            a: (self.a * k).clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: a.r + (b.r - a.r) * t,
            g: a.g + (b.g - a.g) * t,
            b: a.b + (b.b - a.b) * t,
            a: a.a + (b.a - a.a) * t,
        }
    }

    /// Relative luminance (sRGB) used to pick readable glyphs on colored tiles.
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }
}

// Activity colors (section 4.2)
pub const MUSIC_DEFAULT: Color = rgb(0xFF, 0x6B, 0x8A);
pub const CALL_GREEN: Color = rgb(0x4A, 0xDE, 0x80);
pub const SHELF_BLUE: Color = rgb(0x60, 0xCD, 0xFF);
pub const TIMER_AMBER: Color = rgb(0xFF, 0xB5, 0x47);
pub const VOLUME_WHITE: Color = rgb(0xFF, 0xFF, 0xFF);
pub const DARK_GLYPH: Color = rgb(0x0B, 0x0B, 0x0C);
pub const DANGER_RED: Color = rgb(0xC4, 0x2B, 0x1C);

// File badge colors (section 6.3)
pub const BADGE_PDF: Color = rgb(0xFF, 0x6B, 0x6B);
pub const BADGE_IMAGE: Color = SHELF_BLUE;
pub const BADGE_ARCHIVE: Color = TIMER_AMBER;
pub const BADGE_DOC: Color = rgb(0x7A, 0xA2, 0xFF);
pub const BADGE_SHEET: Color = CALL_GREEN;
pub const BADGE_VIDEO: Color = rgb(0xC7, 0x92, 0xEA);
pub const BADGE_OTHER: Color = rgb(0xB0, 0xB0, 0xB8);

// Dark theme surface tokens (section 4.1)
pub const PILL_FILL_REST_A: f32 = 0.09;
pub const PILL_FILL_HOVER_A: f32 = 0.14;
pub const PILL_FILL_PRESS_A: f32 = 0.06;
pub const PILL_BORDER_REST_A: f32 = 0.10;
pub const PILL_BORDER_ACTIVE_A: f32 = 0.55;
pub const PILL_BORDER_DRAG_A: f32 = 0.90;
pub const DRAG_FILL_A: f32 = 0.14;
pub const FLY_FILL: Color = rgb(0x20, 0x20, 0x20);
pub const FLY_FILL_A: f32 = 0.95;
pub const FLY_BORDER_A: f32 = 0.10;
pub const TEXT_PRIMARY_A: f32 = 1.0;
pub const TEXT_PILL_LABEL_A: f32 = 0.94;
pub const TEXT_SECONDARY_A: f32 = 0.68;
pub const TEXT_TERTIARY_A: f32 = 0.58;
pub const SURFACE_BUTTON_A: f32 = 0.08;
pub const SURFACE_BUTTON_HOVER_A: f32 = 0.15;
pub const SURFACE_BUTTON_PRESS_A: f32 = 0.06;
pub const TRACK_A: f32 = 0.22;
pub const TIMER_TRACK_A: f32 = 0.14;
pub const CHIP_FILL_A: f32 = 0.08;
pub const SHELF_TILE_A: f32 = 0.06;
pub const SHELF_TILE_BORDER_A: f32 = 0.06;
pub const SHELF_TILE_HOVER_A: f32 = 0.11;
pub const VOLUME_TILE_A: f32 = 0.14;
pub const VOLUME_TRACK_A: f32 = 0.20;

// Light theme tokens (section 4.4)
pub const LIGHT_PILL_FILL_A: f32 = 0.05;
pub const LIGHT_PILL_FILL_HOVER_A: f32 = 0.09;
pub const LIGHT_PILL_BORDER_A: f32 = 0.08;
pub const LIGHT_FLY_FILL: Color = rgb(0xF3, 0xF3, 0xF3);
pub const LIGHT_FLY_BORDER_A: f32 = 0.08;
pub const LIGHT_TEXT: Color = rgb(0x1A, 0x1A, 0x1A);
pub const LIGHT_TEXT_SECONDARY_A: f32 = 0.62;
pub const LIGHT_TRACK_A: f32 = 0.16;
pub const LIGHT_SURFACE_BUTTON_A: f32 = 0.05;

// Typography (section 4.3)
pub const FONT_FAMILY: &str = "Segoe UI Variable Text";
pub const FONT_FALLBACK: &str = "Segoe UI";
pub const FONT_PILL_LABEL: f32 = 12.0;
pub const FONT_PILL_VALUE: f32 = 12.0;
pub const FONT_FLY_TITLE: f32 = 15.0;
pub const FONT_FLY_SUBTITLE: f32 = 12.0;
pub const FONT_TIMER_BIG: f32 = 22.0;
pub const FONT_TIME_LABEL: f32 = 11.0;
pub const FONT_BUTTON: f32 = 12.5;
pub const FONT_HINT: f32 = 11.0;
pub const FONT_FILE_NAME: f32 = 12.0;
pub const FONT_FILE_SIZE: f32 = 11.0;
pub const FONT_BADGE: f32 = 10.0;
pub const LINE_PILL: f32 = 16.0;
pub const LINE_FLY_TITLE: f32 = 20.0;
pub const LINE_FLY_SUBTITLE: f32 = 18.0;
pub const LINE_TIMER_BIG: f32 = 28.0;
pub const LINE_TIME: f32 = 14.0;
pub const LINE_HINT: f32 = 14.0;

// ------------------------------------------------------------ activity ids
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActivityKind {
    Music,
    Call,
    Shelf,
    Timer,
    Volume,
}

impl ActivityKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Music => "Music",
            Self::Call => "Call",
            Self::Shelf => "File shelf",
            Self::Timer => "Timer",
            Self::Volume => "Volume",
        }
    }
}

// --------------------------------------------------------------- geometry
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RectD {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectD {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn center_x(self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn center_y(self) -> f32 {
        self.y + self.h / 2.0
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn inset(self, d: f32) -> Self {
        Self {
            x: self.x + d,
            y: self.y + d,
            w: (self.w - d * 2.0).max(0.0),
            h: (self.h - d * 2.0).max(0.0),
        }
    }

    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    /// Per-edge lerp used by the flyout morph.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
            w: a.w + (b.w - a.w) * t,
            h: a.h + (b.h - a.h) * t,
        }
    }

    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            w: self.right().max(other.right()) - x,
            h: self.bottom().max(other.bottom()) - y,
        }
    }
}

/// The sole source of expanded-card content geometry. `fly` is normally the
/// fixed 400 × 192 card, but accepting a rectangle makes DPI/layout tests use
/// the same rounding-free DIP calculation as rendering.
pub fn card_content_rect(fly: RectD) -> RectD {
    RectD::new(
        fly.x + RAIL_W + RAIL_DIVIDER + CARD_PAD,
        fly.y + CARD_PAD,
        (fly.w - RAIL_W - RAIL_DIVIDER - CARD_PAD * 2.0).max(0.0),
        (fly.h - CARD_PAD * 2.0).max(0.0),
    )
}

#[cfg(test)]
mod v08_tests {
    use super::*;

    #[test]
    fn v08_fixed_geometry_and_rail_guard() {
        assert_eq!(PILL_W, 224.0);
        assert_eq!((FLY_W, FLY_H), (400.0, 192.0));
        assert_eq!(
            (RAIL_W, RAIL_TILE, RAIL_GAP, RAIL_ICON),
            (56.0, 32.0, 5.0, 18.0)
        );
        assert_eq!(RAIL_INDICATOR_H, 16.0);
        assert_eq!(CARD_PAD, 7.0);
        assert_eq!((PILL_BAND_INSET, FLY_BOTTOM_GAP), (4.0, 6.0));
        assert!(RAIL_TILE * 5.0 + RAIL_GAP * 4.0 <= FLY_H - 4.0);
    }

    #[test]
    fn card_content_rect_is_dpi_invariant_in_dips() {
        let dip = card_content_rect(RectD::new(0.0, 0.0, FLY_W, FLY_H));
        assert_eq!(dip, RectD::new(64.0, 7.0, 329.0, 178.0));
        for scale in [1.0_f32, 1.25, 1.5, 1.75] {
            let expected_w = ((FLY_W - RAIL_W - RAIL_DIVIDER - CARD_PAD * 2.0) * scale).round();
            let expected_h = ((FLY_H - CARD_PAD * 2.0) * scale).round();
            assert_eq!((dip.w * scale).round(), expected_w);
            assert_eq!((dip.h * scale).round(), expected_h);
        }
    }
}
