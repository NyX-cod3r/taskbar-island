//! design.rs â€” every size, timing and color from the TaskbarIsland v1.0 spec.
//!
//! This is the single source of truth. No other module may introduce a magic
//! dimension, duration or token color.

// ---------------------------------------------------------------- sizes (DIP)
/// Original compact footprint. The new renderer and flyout use this same
/// stable footprint so activity changes never create a second-sized pill.
pub const PILL_W: f32 = 224.0;
/// Vertical gap between the pill and the taskbar edge. It matches the margin
/// of a taskbar icon's hit box, so the band is fully visible with equal
/// spacing top and bottom.
pub const PILL_INSET: f32 = 4.0;
pub const BUBBLE: f32 = 32.0;
pub const BUBBLE_GAP: f32 = 8.0;
pub const FLY_W: f32 = 400.0;
pub const FLY_H: f32 = 192.0;
pub const PILL_TILE: f32 = 24.0;
/// Equal horizontal padding: the leading tile's gap to the left edge must
/// match the text's gap to the right edge.
pub const PILL_PAD: f32 = 10.0;
pub const GAP_TRAY: f32 = 12.0;
/// The flyout sits closer to the pill than the v1.0 spec's 12 DIP: the taskbar
/// surface ends at the top of the band, and the owner asked for the card to
/// come down so it does not read as a detached popup.
pub const GAP_TASKBAR: f32 = 6.0;
pub const WINDOW_W: f32 = 520.0;
pub const WINDOW_TOP_PAD: f32 = 40.0;
pub const PILL_TILE_GAP: f32 = 8.0;
pub const PILL_PROGRESS_H: f32 = 2.0;
/// Distance from the pill's bottom edge to the progress line. A small inset
/// keeps the line inside the rounded band at every DPI.
pub const PILL_PROGRESS_INSET: f32 = 8.0;
/// Compact-pill wave amplitude and stroke; smaller than the card so it reads
/// as a ripple on the icon's bottom edge.
pub const PILL_WAVE_AMP: f32 = 1.6;
pub const PILL_WAVE_STROKE: f32 = 1.6;
/// Music and timer pills lift their text this far above centre to leave the
/// wave its own band.
pub const PILL_TEXT_DY: f32 = -2.0;
pub const PILL_GLYPH: f32 = 14.0;
pub const LEAD_GLYPH: f32 = 26.0;

// ------------------------------------------------ expanded card (v0.4)
/// Left activity rail. The divider is the rail's right edge, so the content
/// column starts at 57 DIP.
pub const RAIL_W: f32 = 56.0;
pub const RAIL_DIVIDER: f32 = 1.0;
pub const CARD_CONTENT_X: f32 = RAIL_W + RAIL_DIVIDER;
/// Uniform inner padding for every card: 7 DIP (design tokens; was 12).
pub const CARD_PAD: f32 = 7.0;
/// Gap between stacked content sections inside a card: 8 DIP (design tokens; was 12).
pub const CARD_GAP: f32 = 8.0;
/// Inner content rectangle of an expanded card: the 400×192 card minus the 56 DIP rail
/// and 1 px divider, minus CARD_PAD on all four sides. Every card computes its content
/// area with this single function — no card keeps its own padding constant.
pub fn card_content_rect(fly: RectD) -> RectD {
    RectD::new(
        fly.x + CARD_CONTENT_X + CARD_PAD,
        fly.y + CARD_PAD,
        FLY_W - CARD_CONTENT_X - CARD_PAD * 2.0,
        FLY_H - CARD_PAD * 2.0,
    )
}
/// v0.7 rail: five 32 DIP tiles with 5 DIP gaps (180 DIP) centred in the card.
pub const RAIL_BTN: f32 = 32.0;
pub const RAIL_GAP: f32 = 5.0;
pub const RAIL_INSET: f32 = (FLY_H - (RAIL_BTN * 5.0 + RAIL_GAP * 4.0)) * 0.5;
pub const RAIL_GLYPH: f32 = 18.0;
pub const RAIL_STROKE: f32 = 1.8;
pub const RAIL_IND_W: f32 = 3.0;
pub const RAIL_IND_H: f32 = 16.0;
pub const RAIL_IND_R: f32 = 2.0;
/// The indicator is centred on the tile: (32 - 16) / 2 = 8 DIP from its top.
pub const RAIL_IND_DY: f32 = 8.0;
pub const HEADER_H: f32 = 28.0;
pub const HEADER_TILE: f32 = 28.0;
pub const HEADER_GAP: f32 = 10.0;

// Music card
pub const MUSIC_ART: f32 = 64.0;
pub const R_ART: f32 = 6.0;
pub const MUSIC_INFO_GAP: f32 = 12.0;
pub const MUSIC_LINE_GAP: f32 = 3.0;
pub const MUSIC_PROGRESS_H: f32 = 14.0;
pub const MUSIC_WAVE_AMP: f32 = 2.0;
pub const MUSIC_WAVE_LEN: f32 = 14.0;
pub const MUSIC_WAVE_STROKE: f32 = 2.6;
pub const MUSIC_TRACK_STROKE: f32 = 3.0;
pub const MUSIC_THUMB_W: f32 = 3.5;
pub const MUSIC_THUMB_H: f32 = 14.0;
pub const MUSIC_TIME_W: f32 = 30.0;
pub const MUSIC_REMAIN_W: f32 = 34.0;
pub const MUSIC_TIME_GAP: f32 = 10.0;
pub const MUSIC_BTN: f32 = 32.0;
pub const MUSIC_BTN_GAP: f32 = 8.0;
pub const MUSIC_GLYPH: f32 = 18.0;
pub const MUSIC_PLAY_W: f32 = 56.0;
pub const MUSIC_PLAY_H: f32 = 34.0;

// Timer card
pub const TIMER_CTRL_H: f32 = 34.0;
pub const TIMER_START_W: f32 = 64.0;
pub const TIMER_CHIP_W: f32 = 44.0;
pub const TIMER_CHIP_GAP: f32 = 6.0;
pub const TIMER_RESET_W: f32 = 34.0;
pub const TIMER_TRACK_H: f32 = 4.0;
pub const TIMER_PROGRESS_H: f32 = 14.0;
pub const TIMER_THUMB_W: f32 = 3.0;
pub const TIMER_THUMB_H: f32 = 14.0;
/// Preset chips, in minutes. Idle arms the timer; running adds the minutes.
pub const TIMER_PRESETS: [u32; 4] = [1, 5, 10, 25];

// Call card
pub const CALL_STATUS_H: f32 = 52.0;
pub const CALL_BTN_W: f32 = 48.0;
pub const CALL_BTN_H: f32 = 40.0;
pub const CALL_BTN_GAP: f32 = 8.0;
pub const SIGNAL_W: f32 = 18.0;
pub const SIGNAL_H: f32 = 16.0;

// Shelf card. The shelf owns the whole content column from
// `card_content_rect()` (CARD_PAD on all four sides), a 28 DIP toolbar, then a
// scrolling 4-column grid that fills the remaining height.
pub const SHELF_INNER_W: f32 = FLY_W - CARD_CONTENT_X - CARD_PAD * 2.0;
pub const SHELF_INNER_H: f32 = FLY_H - CARD_PAD * 2.0;
pub const SHELF_TOOLBAR_H: f32 = 28.0;
/// Space between the toolbar and the first grid row (design tokens: gap 8).
pub const SHELF_TOOLBAR_GAP: f32 = 8.0;
/// The grid viewport fills all remaining height (two full rows + the gap).
pub const SHELF_VIEW_H: f32 = SHELF_INNER_H - SHELF_TOOLBAR_H - SHELF_TOOLBAR_GAP;
pub const SHELF_COLS: usize = 4;
pub const SHELF_TILE_H: f32 = 66.0;
pub const SHELF_TILE_GAP: f32 = 6.0;
pub const SHELF_ROW_H: f32 = SHELF_TILE_H + SHELF_TILE_GAP;
/// Width of one grid cell once the scrollbar column is taken out.
pub const SHELF_TILE_W: f32 = (SHELF_INNER_W
    - SHELF_SCROLL_W
    - SHELF_SCROLL_GAP
    - SHELF_TILE_GAP * (SHELF_COLS as f32 - 1.0))
    / SHELF_COLS as f32;
pub const SHELF_TILE_R: f32 = 8.0;
pub const SHELF_NAME_H: f32 = 16.0;
/// Colored type chip for non-image files.
pub const SHELF_CHIP: f32 = 30.0;
pub const SHELF_CHIP_R: f32 = 7.0;
pub const SHELF_CHIP_GLYPH: f32 = 16.0;
pub const SHELF_DROP_BORDER: f32 = 1.5;
pub const SHELF_GLYPH: f32 = 22.0;
/// Thin, draggable scrollbar to the right of the grid.
pub const SHELF_SCROLL_W: f32 = 4.0;
pub const SHELF_SCROLL_GAP: f32 = 10.0;
pub const SHELF_SCROLL_MIN: f32 = 24.0;
/// One wheel notch moves the grid this far.
pub const SHELF_SCROLL_STEP: f32 = 60.0;
/// Selected tile ring and the top-left check badge.
pub const SHELF_CHECK: f32 = 18.0;
pub const SHELF_SELECT_RING: f32 = 2.0;
/// Square icon button on the tile's hover state.
pub const SHELF_REMOVE: f32 = 18.0;
pub const SHELF_TOOL_BTN: f32 = 28.0;
pub const SHELF_TOOL_GAP: f32 = 6.0;
pub const SHELF_CONVERT_W: f32 = 36.0;
pub const SHELF_CONVERT_H: f32 = 28.0;
pub const SHELF_CONVERT_R: f32 = 8.0;
pub const SHELF_TOOL_GLYPH: f32 = 16.0;
pub const SHELF_COUNT_GLYPH: f32 = 16.0;
/// Soft cap: the oldest entry is dropped silently and the count tooltip warns.
pub const SHELF_SOFT_CAP: usize = 500;
/// A name strip is painted over the bottom of every tile.
pub const SHELF_NAME_ALPHA: f32 = 0.72;

// Convert card
/// The card uses the shared content rect from `card_content_rect()`. Stack:
/// 28 header + 8 + 28 tabs + 8 + grid (fills the rest of the column).
pub const CONVERT_INNER_W: f32 = FLY_W - CARD_CONTENT_X - CARD_PAD * 2.0;
pub const CONVERT_INNER_H: f32 = FLY_H - CARD_PAD * 2.0;
pub const CONVERT_STACK_GAP: f32 = 8.0;
pub const CONVERT_STACK_H: f32 =
    CONVERT_HEADER_H + CONVERT_STACK_GAP + CONVERT_TABS_H + CONVERT_STACK_GAP + CONVERT_OPTIONS_H;

/// Header: 24 DIP thumb/type tile, one truncated name line, close glyph.
pub const CONVERT_HEADER_H: f32 = 28.0;
pub const CONVERT_HEADER_TILE: f32 = 24.0;
pub const CONVERT_HEADER_GAP: f32 = 8.0;
pub const CONVERT_NAME_FONT: f32 = 12.0;
pub const CONVERT_CLOSE: f32 = 20.0;

/// Clipboard card. Same shared content rect as the shelf/convert cards
/// (`card_content_rect()`); the list fills the height the toolbar leaves.
pub const CLIP_INNER_W: f32 = FLY_W - CARD_CONTENT_X - CARD_PAD * 2.0;
pub const CLIP_INNER_H: f32 = FLY_H - CARD_PAD * 2.0;
/// Large muted glyph of the empty Clipboard card.
pub const CLIP_EMPTY_GLYPH: f32 = 40.0;
pub const CLIP_EMPTY_STROKE: f32 = 2.0;
/// Small amber dot that marks capture paused on the pill and empty card.
pub const CLIP_PAUSED_DOT: f32 = 6.0;
/// The pill shows "+N" for this long after a new copy, then the idle look.
pub const CLIP_POP_MS: u32 = 2000;
/// Two-step Clear (spec 3.3): the trash swaps for a red âœ“ and a âœ• for this
/// long, then the confirm auto-cancels.
pub const CLIP_CLEAR_CONFIRM_MS: u32 = 4000;
/// Danger tint of the armed Clear check.
pub const CLIP_DANGER: Color = rgb(0xF0, 0x61, 0x7F);
/// History capacity and retention from the v0.7 clipboard spec.
pub const CLIP_MAX_ITEMS: usize = 30;
pub const CLIP_RETENTION_DAYS: i64 = 7;
/// Coalesce bursts of `WM_CLIPBOARDUPDATE` before touching the clipboard.
pub const CLIP_DEBOUNCE_MS: u32 = 150;
/// Retention sweep cadence while the app runs.
pub const CLIP_SWEEP_MS: u32 = 60 * 60 * 1000;
/// Capture limits: text bytes and image megapixels / payload bytes.
pub const CLIP_TEXT_MAX_BYTES: usize = 1024 * 1024;
pub const CLIP_IMAGE_MAX_PIXELS: u64 = 20_000_000;
pub const CLIP_IMAGE_MAX_BYTES: u64 = 25 * 1024 * 1024;
/// Longest side of the cached image thumbnail: 128 px = 2× DPI for the 64 DIP
/// row thumbnail (spec 7).
pub const CLIP_THUMB_PX: u32 = 128;

// Clipboard card: 28 DIP toolbar + 8 gap + list fills the rest (142 DIP).
pub const CLIP_TOOLBAR_H: f32 = 28.0;
pub const CLIP_TOOLBAR_GAP: f32 = 8.0;
pub const CLIP_LIST_H: f32 = CLIP_INNER_H - CLIP_TOOLBAR_H - CLIP_TOOLBAR_GAP;
/// Segmented filter: 2 DIP padding around 26 x 24 icon-only tabs.
pub const CLIP_FILTER_PAD: f32 = 2.0;
pub const CLIP_FILTER_TAB_W: f32 = 26.0;
pub const CLIP_FILTER_TAB_H: f32 = 24.0;
pub const CLIP_FILTER_GLYPH: f32 = 14.0;
pub const CLIP_FILTER_ACTIVE: Color = rgba(0x5F, 0xD0, 0xC5, 0.24);
pub const CLIP_TOOL_BTN: f32 = 28.0;
pub const CLIP_TOOL_GAP: f32 = 6.0;
/// History rows: 34 DIP tall, image rows 50 (64 × 40 thumb), 2 DIP gap,
/// radius 6 (spec 7).
pub const CLIP_ROW_H: f32 = 34.0;
pub const CLIP_ROW_IMG_H: f32 = 50.0;
pub const CLIP_ROW_GAP: f32 = 2.0;
pub const CLIP_ROW_STRIDE: f32 = CLIP_ROW_H + CLIP_ROW_GAP;
pub const CLIP_ROW_IMG_STRIDE: f32 = CLIP_ROW_IMG_H + CLIP_ROW_GAP;
pub const CLIP_ROW_R: f32 = 6.0;
pub const CLIP_ROW_PAD: f32 = 8.0;
pub const CLIP_ROW_HOVER: Color = rgba(255, 255, 255, 0.07);
pub const CLIP_GLYPH_SLOT: f32 = 20.0;
pub const CLIP_GLYPH: f32 = 16.0;
/// Small leading slot: file rows keep a 24 × 18 thumbnail of a single image.
pub const CLIP_THUMB_W: f32 = 24.0;
pub const CLIP_THUMB_H: f32 = 18.0;
/// Image row thumbnail (spec 7: "50 DIP row, 64 × 40 thumbnail").
pub const CLIP_ROW_THUMB_W: f32 = 64.0;
pub const CLIP_ROW_THUMB_H: f32 = 40.0;
/// Checkerboard behind images smaller than the thumb slot, so a 37 × 13 image
/// is obviously tiny (spec 7).
pub const CLIP_CHECKER: f32 = 4.0;
pub const CLIP_CHECKER_A: Color = rgb(0x50, 0x50, 0x50);
pub const CLIP_CHECKER_B: Color = rgb(0x40, 0x40, 0x40);
/// Hover preview overlay (spec 7): appears after 450 ms of steady hover and
/// stays inside the card (206 × 150 of the 329 × 178 content rect).
pub const CLIP_PREVIEW_MS: u32 = 450;
pub const CLIP_PREVIEW_W: f32 = 206.0;
pub const CLIP_PREVIEW_H: f32 = 150.0;
pub const CLIP_PREVIEW_PAD: f32 = 6.0;
pub const CLIP_PREVIEW_IMG_W: f32 = 192.0;
pub const CLIP_PREVIEW_IMG_H: f32 = 118.0;
/// Bands under the picture: one `LINE_HINT` caption line, then the shorter
/// `from <app>` line. Together with the pad and the picture they fill the
/// 150 DIP popup exactly, so the source line ends on its bottom edge.
pub const CLIP_PREVIEW_CAPTION_H: f32 = LINE_HINT;
pub const CLIP_PREVIEW_SOURCE_H: f32 = 12.0;
/// Hover/focus cluster: three 24 DIP icon buttons replace the age.
pub const CLIP_ACTION_BTN: f32 = 24.0;
pub const CLIP_ACTION_GAP: f32 = 2.0;
pub const CLIP_AGE_W: f32 = 30.0;
/// Small pin marker left of the age on a pinned row.
pub const CLIP_PIN_MARK: f32 = 12.0;
pub const CLIP_PIN_MARK_GAP: f32 = 4.0;
pub const CLIP_LINK: Color = rgb(0x8F, 0xB8, 0xFF);
/// Row-preview tooltips cap the quoted text at ~200 characters.
pub const CLIP_TIP_CHARS: usize = 200;
/// Thin, draggable scrollbar to the right of the list.
pub const CLIP_SCROLL_W: f32 = 4.0;
pub const CLIP_SCROLL_GAP: f32 = 8.0;
pub const CLIP_SCROLL_MIN: f32 = 24.0;
/// One wheel notch moves the list two rows.
pub const CLIP_SCROLL_STEP: f32 = CLIP_ROW_STRIDE * 2.0;
/// Code-like previews render in Cascadia Mono.
pub const FONT_MONO: &str = "Cascadia Mono";
pub const FONT_MONO_FALLBACK: &str = "Consolas";

/// Segmented, icon-only tab strip: 2 DIP padding around 24 DIP tabs.
pub const CONVERT_TABS_H: f32 = 28.0;
pub const CONVERT_TAB: f32 = 24.0;
pub const CONVERT_TAB_GAP: f32 = 2.0;
pub const CONVERT_TAB_GLYPH: f32 = 15.0;

/// Options: 5 columns, 8 DIP gap; the rows split the grid height (one row of
/// 106 DIP, two rows of 49 DIP — never below the 40 DIP minimum).
pub const CONVERT_OPTIONS_H: f32 = 106.0;
pub const CONVERT_OPTION_COLS: usize = 5;
pub const CONVERT_OPTION_H: f32 = 40.0;
pub const CONVERT_OPTION_GAP: f32 = 8.0;
pub const CONVERT_OPTION_R: f32 = 6.0;
pub const CONVERT_OPTION_GLYPH: f32 = 18.0;
pub const CONVERT_OPTION_FONT: f32 = 12.0;
pub const CONVERT_OPTION_W: f32 = (CONVERT_INNER_W
    - CONVERT_OPTION_GAP * (CONVERT_OPTION_COLS as f32 - 1.0))
    / CONVERT_OPTION_COLS as f32;
/// The card never renders more than one screen of options; the table itself
/// also pins each tab at ten or fewer.
pub const CONVERT_MAX_OPTIONS: usize = 10;

// v0.5 header/actions still used by the provisional Working/Done states.
/// Hint row under the grid, same height as the shelf hint.
pub const CONVERT_HINT_H: f32 = 15.0;
// Converting / done states
pub const CONVERT_STAGE_TILE: f32 = 56.0;
pub const CONVERT_STAGE_R: f32 = 14.0;
pub const CONVERT_STAGE_GAP: f32 = 14.0;
pub const CONVERT_STAGE_ARROW: f32 = 18.0;
/// Large accent percent at the right of the Converting row.
pub const CONVERT_PERCENT_FONT: f32 = 26.0;
/// The three stage dots and the rules that join them.
pub const CONVERT_STAGE_ICON: f32 = 26.0;
pub const CONVERT_STAGE_LINK: f32 = 22.0;
pub const CONVERT_STAGE_LINK_H: f32 = 2.0;
pub const CONVERT_PROGRESS_H: f32 = 8.0;
/// Done view: green check, three equal icon buttons, thin countdown.
pub const CONVERT_DONE_CHECK: f32 = 40.0;
pub const CONVERT_DONE_BTN_H: f32 = 32.0;
pub const CONVERT_DONE_BTN_R: f32 = 8.0;
pub const CONVERT_DONE_COUNTDOWN_H: f32 = 3.0;
pub const CONVERT_DONE_ICON: f32 = 18.0;
pub const CONVERT_COUNTDOWN_MS: u32 = 5000;

// Card motion. The 160 ms indicator uses ease-out (0.2, 0, 0, 1).
pub const RAIL_MOVE_MS: u32 = 160;
pub const CONTENT_OUT_MS: u32 = 100;
pub const CONTENT_IN_MS: u32 = 160;
pub const CONTENT_SETTLE: f32 = 4.0;
pub const EASE_RAIL: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
/// v0.6 tooltips appear after 450 ms of hover/focus. Moving between icon
/// buttons inside this window shows the next tooltip instantly.
pub const TOOLTIP_DELAY_MS: u32 = 450;
pub const TOOLTIP_NEIGHBOR_MS: u32 = 300;
pub const TOOLTIP_GAP: f32 = 4.0;
pub const TOOLTIP_PAD_X: f32 = 8.0;
pub const TOOLTIP_PAD_Y: f32 = 4.0;
pub const TOOLTIP_R: f32 = 4.0;
pub const TOOLTIP_FONT: f32 = 12.0;
pub const TOOLTIP_SUB_FONT: f32 = 11.0;
pub const TOOLTIP_LINE: f32 = 16.0;
pub const TOOLTIP_SUB_LINE: f32 = 14.0;
pub const TOOLTIP_MAX_W: f32 = 280.0;
/// Fill/border/text from the ShelfTip artboard.
pub const TOOLTIP_FILL: Color = rgb(0x2C, 0x2C, 0x2C);
pub const TOOLTIP_BORDER: Color = rgb(0x4A, 0x4A, 0x4A);

// ------------------------------------------------------- flyout internals
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
pub const FRAME_MS: u32 = 16;

// ----------------------------------------------------------------- radii
pub const R_SURFACE: f32 = 8.0;
pub const R_CONTROL: f32 = 6.0;
pub const R_TILE_PILL: f32 = 6.0;
pub const R_TILE_LEAD: f32 = 8.0;
pub const R_THUMB: f32 = 2.0;

// ------------------------------------------------------------------ wave
pub const WAVE_LEN: f32 = 20.0;
pub const WAVE_AMP: f32 = 3.0;
pub const WAVE_FLAT: f32 = 0.02;

// --------------------------------------------------------------- timings
/// Approved flyout motion: 180 ms fade, 220 ms scale/rise, 110 ms close.
pub const FLYOUT_OPEN_FADE_MS: u32 = 180;
pub const FLYOUT_OPEN_MOVE_MS: u32 = 220;
pub const FLYOUT_CLOSE_MS: u32 = 110;
pub const FLYOUT_RISE: f32 = 6.0;
pub const FLYOUT_SCALE: f32 = 0.94;
pub const EASE_FLYOUT: [f32; 4] = [0.2, 0.8, 0.2, 1.0];
pub const HOVER_MS: u32 = 160;
pub const AUTO_CLOSE_MS: u32 = 700;
/// Players drop the GSMTC session for a moment while a track changes. Keep
/// the last snapshot this long so the card and pill do not flicker.
pub const MEDIA_CLEAR_MS: u32 = 3000;
/// After one of our own transport clicks the session can stay away while the
/// player loads the next track (and possibly an ad). Hold the old snapshot
/// for this long instead, so the card never closes mid-interaction.
pub const MEDIA_COMMAND_HOLD_MS: u32 = 6000;
pub const TIMER_FLASH_MS: u32 = 600;
pub const TIMER_FLASHES: u32 = 3;

// ----------------------------------------------------------------- curves
pub const EASE_CONTENT: [f32; 4] = [0.2, 0.9, 0.25, 1.0];

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
        Self {
            a: (self.a * a).clamp(0.0, 1.0),
            ..self
        }
    }

    /// Replace (not scale) the alpha channel.
    pub fn set_alpha(self, a: f32) -> Self {
        Self { a, ..self }
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
}

// Activity colors (section 4.2)
pub const MUSIC_DEFAULT: Color = rgb(0xFF, 0x6B, 0x8A);
pub const CALL_GREEN: Color = rgb(0x4A, 0xDE, 0x80);
pub const SHELF_BLUE: Color = rgb(0x60, 0xCD, 0xFF);
pub const TIMER_AMBER: Color = rgb(0xFF, 0xB5, 0x47);
pub const VOLUME_WHITE: Color = rgb(0xFF, 0xFF, 0xFF);
pub const DARK_GLYPH: Color = rgb(0x0B, 0x0B, 0x0C);

// File badge colors (section 6.3)
pub const BADGE_PDF: Color = rgb(0xFF, 0x6B, 0x6B);
pub const BADGE_IMAGE: Color = SHELF_BLUE;
pub const BADGE_ARCHIVE: Color = TIMER_AMBER;
pub const BADGE_DOC: Color = rgb(0x7A, 0xA2, 0xFF);
pub const BADGE_SHEET: Color = CALL_GREEN;
pub const BADGE_VIDEO: Color = rgb(0xC7, 0x92, 0xEA);
pub const BADGE_OTHER: Color = rgb(0xB0, 0xB0, 0xB8);
/// v0.5 file tiles: identity colors from the ShelfFiles artboard.
pub const FILE_IMAGE: Color = rgb(0x2F, 0xBF, 0xA0);
pub const FILE_PDF: Color = rgb(0xFF, 0x5F, 0x57);
pub const FILE_VIDEO: Color = rgb(0x8B, 0x5C, 0xF6);
pub const FILE_AUDIO: Color = rgb(0xFF, 0x8A, 0x4C);
pub const FILE_DOC: Color = rgb(0x7A, 0xA2, 0xFF);

// Dark theme surface tokens (section 4.1)
pub const PILL_BORDER_REST_A: f32 = 0.10;
pub const PILL_BORDER_ACTIVE_A: f32 = 0.55;
pub const PILL_BORDER_DRAG_A: f32 = 0.90;
pub const DRAG_FILL_A: f32 = 0.14;
pub const FLY_FILL: Color = rgb(0x28, 0x28, 0x28);
pub const FLY_FILL_A: f32 = 0.95;
pub const FLY_BORDER_A: f32 = 0.09;
pub const TEXT_PRIMARY_A: f32 = 1.0;
pub const TEXT_PILL_LABEL_A: f32 = 0.94;
pub const TEXT_SECONDARY_A: f32 = 0.68;
pub const TEXT_TERTIARY_A: f32 = 0.58;
pub const SURFACE_BUTTON_A: f32 = 0.08;
pub const SURFACE_BUTTON_HOVER_A: f32 = 0.15;
pub const SURFACE_BUTTON_PRESS_A: f32 = 0.06;
pub const TRACK_A: f32 = 0.22;
pub const SHELF_TILE_A: f32 = 0.06;
pub const SHELF_TILE_BORDER_A: f32 = 0.06;
pub const SHELF_TILE_HOVER_A: f32 = 0.11;

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

// Convert card (v0.5). The periwinkle accent is shared by target titles and the
// enabled Shelf chip; actions stay neutral white.
pub const CONVERT_ACCENT: Color = rgb(0xA9, 0xC1, 0xF0);
pub const CONVERT_ACCENT_LIGHT: Color = rgb(0x46, 0x5E, 0x99);
pub const CONVERT_ON_ACCENT: Color = rgb(0x24, 0x2F, 0x49);
pub const CONVERT_TILE_BORDER_A: f32 = 0.05;
pub const CONVERT_DONE: Color = CALL_GREEN;
/// The Undo glyph is the only warm control in the Done footer.
pub const CONVERT_UNDO: Color = rgb(0xE8, 0x9C, 0x9C);
/// Drag-over tile fill from section 4.4.
pub const CONVERT_DROP_FILL_A: f32 = 0.28;
pub const CONVERT_DROP_DIM_A: f32 = 0.45;
/// §5 rule 3: a tile that needs more input than the selection has is drawn at
/// 40 % opacity and its tip says what is missing.
pub const CONVERT_NEEDS_A: f32 = 0.4;

// v0.6 option tiles (exact artboard hexes for the dark card).
pub const CONVERT_OPTION_FILL: Color = rgb(0x30, 0x30, 0x30);
pub const CONVERT_OPTION_BORDER: Color = rgb(0x3A, 0x3A, 0x3A);
pub const CONVERT_OPTION_HOVER: Color = rgb(0x3B, 0x3B, 0x3B);
pub const CONVERT_TAB_ACTIVE: Color = Color {
    r: 0x8F as f32 / 255.0,
    g: 0xA6 as f32 / 255.0,
    b: 0xD6 as f32 / 255.0,
    a: 0.26,
};
pub const LIGHT_CONVERT_OPTION_FILL: Color = rgb(0xEC, 0xEC, 0xEC);
pub const LIGHT_CONVERT_OPTION_BORDER: Color = rgb(0xDC, 0xDC, 0xDC);
pub const LIGHT_CONVERT_OPTION_HOVER: Color = rgb(0xE2, 0xE2, 0xE2);
pub const LIGHT_CONVERT_TAB_ACTIVE: Color = Color {
    r: 0x46 as f32 / 255.0,
    g: 0x5E as f32 / 255.0,
    b: 0x99 as f32 / 255.0,
    a: 0.20,
};

// Expanded card surfaces (v0.4 Â§3.1). The card shell keeps its existing
// material; only the rail and divider are new.
pub const CARD_RAIL: Color = rgb(0x21, 0x21, 0x21);
pub const CARD_DIVIDER: Color = rgb(0x34, 0x34, 0x34);
pub const LIGHT_CARD_RAIL: Color = rgb(0xEA, 0xEA, 0xEA);
pub const LIGHT_CARD_DIVIDER: Color = rgb(0xDC, 0xDC, 0xDC);

// Typography (section 4.3)
pub const FONT_FAMILY: &str = "Segoe UI Variable Text";
pub const FONT_FALLBACK: &str = "Segoe UI";
pub const FONT_PILL_LABEL: f32 = 12.0;
pub const FONT_PILL_VALUE: f32 = 12.0;
pub const FONT_FLY_TITLE: f32 = 15.0;
pub const FONT_FLY_SUBTITLE: f32 = 12.0;
pub const FONT_TIME_LABEL: f32 = 11.0;
pub const FONT_BUTTON: f32 = 12.5;
pub const FONT_HINT: f32 = 11.0;
pub const FONT_FILE_NAME: f32 = 12.0;
pub const LINE_PILL: f32 = 16.0;
pub const LINE_FLY_TITLE: f32 = 20.0;
pub const LINE_TIME: f32 = 14.0;
pub const LINE_HINT: f32 = 14.0;

// Card typography (v0.4).
pub const LINE_CARD_STATUS: f32 = 16.0;
pub const FONT_MUSIC_TITLE: f32 = 16.0;
pub const LINE_MUSIC_TITLE: f32 = 20.0;
pub const FONT_MUSIC_META: f32 = 12.0;
pub const LINE_MUSIC_META: f32 = 16.0;
pub const FONT_TIMER_READOUT: f32 = 36.0;
pub const LINE_TIMER_READOUT: f32 = 40.0;
pub const FONT_CALL_TITLE: f32 = 18.0;
pub const LINE_CALL_TITLE: f32 = 24.0;
pub const FONT_CHIP: f32 = 12.0;
pub const FONT_OPEN_CALL: f32 = 13.0;

// ------------------------------------------------------------ activity ids
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActivityKind {
    Music,
    Call,
    Shelf,
    Timer,
    Clipboard,
}

// Rail accent per activity (v0.4 Â§3.1). Card-only: the compact pill keeps its
// existing activity colors above.
pub const RAIL_MUSIC: Color = rgb(0xF0, 0x61, 0x7F);
pub const RAIL_TIMER: Color = rgb(0xFF, 0xB5, 0x47);
pub const RAIL_CALL: Color = rgb(0x5B, 0xE3, 0x8F);
pub const RAIL_SHELF: Color = rgb(0x8F, 0xA6, 0xD6);
/// v0.7 Clipboard accent.
pub const RAIL_CLIP: Color = rgb(0x5F, 0xD0, 0xC5);

pub const fn rail_accent(kind: ActivityKind) -> Color {
    match kind {
        ActivityKind::Music => RAIL_MUSIC,
        ActivityKind::Timer => RAIL_TIMER,
        ActivityKind::Call => RAIL_CALL,
        ActivityKind::Shelf => RAIL_SHELF,
        ActivityKind::Clipboard => RAIL_CLIP,
    }
}

/// Fixed top-to-bottom order of the rail buttons.
pub const RAIL_ORDER: [ActivityKind; 5] = [
    ActivityKind::Music,
    ActivityKind::Timer,
    ActivityKind::Call,
    ActivityKind::Shelf,
    ActivityKind::Clipboard,
];

pub fn rail_index(kind: ActivityKind) -> usize {
    RAIL_ORDER
        .iter()
        .position(|&item| item == kind)
        .unwrap_or(0)
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
}

// Layout invariants, checked at compile time in every build: the convert
// stack exactly fills the shared content column, the three cards share one
// inner width, and every grid row keeps the 40 DIP tile minimum (one row
// takes the whole 106 DIP grid, two rows split it 49/49).
const _: () = assert!(CONVERT_STACK_H == CONVERT_INNER_H);
const _: () = assert!(
    CONVERT_OPTIONS_H
        == CONVERT_INNER_H - CONVERT_HEADER_H - CONVERT_TABS_H - CONVERT_STACK_GAP * 2.0
);
const _: () = assert!(SHELF_INNER_W == CLIP_INNER_W && SHELF_INNER_W == CONVERT_INNER_W);
const _: () = assert!(SHELF_ROW_H * 2.0 - SHELF_TILE_GAP <= SHELF_VIEW_H);
const _: () = assert!(CONVERT_OPTIONS_H >= CONVERT_OPTION_H);
const _: () = assert!((CONVERT_OPTIONS_H - CONVERT_OPTION_GAP) / 2.0 >= CONVERT_OPTION_H);
// Clipboard (spec 7): the hover preview overlay always fits the content rect
// and the 64 x 40 image thumb fits the 50 DIP row with room to spare.
const _: () = assert!(CLIP_PREVIEW_W <= CLIP_INNER_W && CLIP_PREVIEW_H <= CLIP_INNER_H);
// The 150 DIP popup exactly fills the pad, the picture and its two caption
// bands, and that height in turn equals the 8 DIP toolbar gap plus the 142 DIP
// list column — so sitting on the content rect's bottom edge it covers the
// list and never a toolbar control.
const _: () = assert!(
    CLIP_PREVIEW_H
        == CLIP_PREVIEW_PAD + CLIP_PREVIEW_IMG_H + CLIP_PREVIEW_CAPTION_H + CLIP_PREVIEW_SOURCE_H
);
const _: () = assert!(CLIP_PREVIEW_H == CLIP_TOOLBAR_GAP + CLIP_LIST_H);
// The 192 DIP picture column keeps its 6 DIP pad on both sides.
const _: () = assert!(CLIP_PREVIEW_IMG_W + CLIP_PREVIEW_PAD * 2.0 <= CLIP_PREVIEW_W);
const _: () = assert!(CLIP_ROW_THUMB_H + 2.0 * CLIP_ROW_GAP <= CLIP_ROW_IMG_H);
const _: () = assert!(CLIP_ROW_IMG_STRIDE - CLIP_ROW_GAP == CLIP_ROW_IMG_H);

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule 6 of the v0.4 spec: the pill and the expanded card's outer
    /// footprint are frozen. Changing any of these must fail the build.
    #[test]
    fn frozen_pill_and_card_geometry() {
        assert_eq!(PILL_W, 224.0);
        assert_eq!(FLY_W, 400.0);
        assert_eq!(FLY_H, 192.0);
        assert_eq!(GAP_TASKBAR, 6.0);
    }

    /// The v0.4 interior is derived, so guard the numbers the artboards use.
    /// v0.8: card padding is 7 DIP, so the content rect is the card (400×192)
    /// minus the 56 DIP rail + 1 px divider minus CARD_PAD on all sides.
    #[test]
    fn card_interior_matches_v08() {
        assert_eq!(CARD_CONTENT_X, 57.0);
        assert_eq!(CARD_PAD, 7.0);
        assert_eq!(CARD_GAP, 8.0);
        let content = card_content_rect(RectD::new(0.0, 0.0, 400.0, 192.0));
        assert_eq!(content.x, 64.0);
        assert_eq!(content.y, 7.0);
        assert_eq!(content.w, 329.0);
        assert_eq!(content.h, 178.0);
        assert_eq!(RAIL_INSET, 6.0);
        assert_eq!(RAIL_BTN * 5.0 + RAIL_GAP * 4.0 + RAIL_INSET * 2.0, FLY_H);
    }

    /// card_content_rect() derives from the fixed values at every DPI: padding
    /// and rail are constant DIP, so the rect is the card inner minus rail
    /// minus padding, computed the same way everywhere.
    #[test]
    fn card_content_rect_derives_from_fixed_values() {
        for (fly_x, fly_y) in [(0.0, 0.0), (100.0, 200.0), (1000.0, 300.0)] {
            let fly = RectD::new(fly_x, fly_y, FLY_W, FLY_H);
            let content = card_content_rect(fly);
            assert_eq!(content.x, fly_x + CARD_CONTENT_X + CARD_PAD);
            assert_eq!(content.y, fly_y + CARD_PAD);
            assert_eq!(content.w, FLY_W - CARD_CONTENT_X - CARD_PAD * 2.0);
            assert_eq!(content.h, FLY_H - CARD_PAD * 2.0);
            assert!(content.x >= fly_x + CARD_CONTENT_X);
            assert!(content.right() <= fly_x + FLY_W);
            assert!(content.bottom() <= fly_y + FLY_H);
        }
    }

    /// v0.7 rail: five 32 x 32 tiles with 5 DIP gaps, 18 DIP glyphs and a
    /// 3 x 16 indicator 8 DIP below the tile top.
    #[test]
    fn rail_fits_five_tiles_matches_v07() {
        assert_eq!(RAIL_W, 56.0);
        assert_eq!(RAIL_BTN, 32.0);
        assert_eq!(RAIL_GAP, 5.0);
        assert_eq!(RAIL_GLYPH, 18.0);
        assert_eq!(RAIL_IND_W, 3.0);
        assert_eq!(RAIL_IND_H, 16.0);
        assert_eq!(RAIL_IND_DY, 8.0);
        let stack = RAIL_BTN * 5.0 + RAIL_GAP * 4.0;
        assert_eq!(stack, 180.0);
        assert!(FLY_H - stack >= 4.0, "five tiles need at least 4 DIP spare");
        assert_eq!(RAIL_ORDER.len(), 5);
        assert_eq!(
            RAIL_ORDER,
            [
                ActivityKind::Music,
                ActivityKind::Timer,
                ActivityKind::Call,
                ActivityKind::Shelf,
                ActivityKind::Clipboard,
            ]
        );
        assert_eq!(rail_accent(ActivityKind::Clipboard), RAIL_CLIP);
        assert_eq!(RAIL_CLIP, rgba(0x5F, 0xD0, 0xC5, 1.0));
    }

    /// Clipboard card uses the same content column as the shelf.
    #[test]
    fn clipboard_card_matches_v07() {
        assert_eq!(CLIP_INNER_W, 329.0);
        assert_eq!(CLIP_INNER_H, 178.0);
        assert_eq!(CLIP_EMPTY_GLYPH, 40.0);
        assert_eq!(CLIP_POP_MS, 2000);
        assert_eq!(CLIP_CLEAR_CONFIRM_MS, 4000);
        assert_eq!(CLIP_DANGER, rgb(0xF0, 0x61, 0x7F));
        assert_eq!(CLIP_MAX_ITEMS, 30);
        assert_eq!(CLIP_RETENTION_DAYS, 7);
        assert_eq!(CLIP_DEBOUNCE_MS, 150);
        assert_eq!(CLIP_SWEEP_MS, 3_600_000);
        assert_eq!(CLIP_TEXT_MAX_BYTES, 1_048_576);
        assert_eq!(CLIP_IMAGE_MAX_PIXELS, 20_000_000);
        assert_eq!(CLIP_IMAGE_MAX_BYTES, 26_214_400);
        assert_eq!(CLIP_THUMB_PX, 128);
    }

    /// v0.7 Phase 3 card on the shared content rect: 28 toolbar + 8 gap +
    /// 142 list, 34/50 DIP rows with a 2 DIP gap, 26 x 24 filter tabs in a
    /// 2 DIP padded strip, 24 DIP row actions.
    #[test]
    fn clipboard_list_matches_v07() {
        assert_eq!(CLIP_TOOLBAR_H, 28.0);
        assert_eq!(CLIP_TOOLBAR_GAP, 8.0);
        assert_eq!(CLIP_LIST_H, 142.0);
        assert_eq!(
            CLIP_TOOLBAR_H + CLIP_TOOLBAR_GAP + CLIP_LIST_H,
            CLIP_INNER_H
        );
        assert_eq!(CLIP_FILTER_PAD, 2.0);
        assert_eq!(CLIP_FILTER_TAB_W, 26.0);
        assert_eq!(CLIP_FILTER_TAB_H, 24.0);
        assert_eq!(CLIP_FILTER_TAB_H + CLIP_FILTER_PAD * 2.0, CLIP_TOOLBAR_H);
        assert_eq!(CLIP_FILTER_ACTIVE, rgba(0x5F, 0xD0, 0xC5, 0.24));
        assert_eq!(CLIP_ROW_H, 34.0);
        assert_eq!(CLIP_ROW_IMG_H, 50.0);
        assert_eq!(CLIP_ROW_GAP, 2.0);
        assert_eq!(CLIP_ROW_STRIDE, 36.0);
        assert_eq!(CLIP_ROW_IMG_STRIDE, 52.0);
        assert_eq!(CLIP_ROW_R, 6.0);
        assert_eq!(CLIP_ROW_HOVER, rgba(255, 255, 255, 0.07));
        assert_eq!(CLIP_GLYPH_SLOT, 20.0);
        assert_eq!(CLIP_THUMB_W, 24.0);
        assert_eq!(CLIP_THUMB_H, 18.0);
        assert_eq!(CLIP_ROW_THUMB_W, 64.0);
        assert_eq!(CLIP_ROW_THUMB_H, 40.0);
        assert_eq!(CLIP_ACTION_BTN, 24.0);
        assert_eq!(CLIP_LINK, rgb(0x8F, 0xB8, 0xFF));
        assert_eq!(CLIP_SCROLL_W, 4.0);
        assert_eq!(CLIP_TIP_CHARS, 200);
    }

    /// Hover preview overlay (spec 7): 450 ms delay, 206 × 150 popup with a
    /// 192 × 118 image area, always inside the 329 × 178 content rect.
    #[test]
    fn clipboard_preview_overlay_matches_spec() {
        assert_eq!(CLIP_PREVIEW_MS, 450);
        assert_eq!(CLIP_PREVIEW_W, 206.0);
        assert_eq!(CLIP_PREVIEW_H, 150.0);
        assert_eq!(CLIP_PREVIEW_IMG_W, 192.0);
        assert_eq!(CLIP_PREVIEW_IMG_H, 118.0);
        // The picture column's fit is a compile-time `const _: () = assert!`
        // next to the tokens; what is worth pinning here is where it lands.
        // Horizontal centering, vertical fit: the popup sits on the toolbar's
        // bottom edge and ends exactly at the content rect's bottom, so it
        // covers the toolbar gap and the list and nothing else.
        let x = (CLIP_INNER_W - CLIP_PREVIEW_W) * 0.5;
        let y = CLIP_INNER_H - CLIP_PREVIEW_H;
        assert_eq!(x, 61.5);
        assert_eq!(y, CLIP_TOOLBAR_H);
        assert_eq!(CLIP_PREVIEW_H, CLIP_TOOLBAR_GAP + CLIP_LIST_H);
        assert!(x + CLIP_PREVIEW_W <= CLIP_INNER_W);
        assert!(y + CLIP_PREVIEW_H <= CLIP_INNER_H);
        // The picture band and the two caption bands fill the popup exactly.
        assert_eq!(
            CLIP_PREVIEW_PAD + CLIP_PREVIEW_IMG_H + CLIP_PREVIEW_CAPTION_H + CLIP_PREVIEW_SOURCE_H,
            CLIP_PREVIEW_H
        );
        assert_eq!(CLIP_PREVIEW_CAPTION_H, LINE_HINT);
        // Body copy shares the picture's column, so text lines up with it:
        // a 7 DIP inset on each side of the 192 DIP column.
        assert_eq!((CLIP_PREVIEW_W - CLIP_PREVIEW_IMG_W) * 0.5, 7.0);
    }

    /// Shelf geometry on the shared content rect (design tokens): a 28 DIP
    /// toolbar, 8 DIP gap and a 4-column scrolling grid of 74.25 x 66 tiles
    /// with 6 DIP gaps, the viewport filling the rest (142 DIP = two rows).
    #[test]
    fn shelf_grid_matches_v06() {
        assert_eq!(SHELF_INNER_W, 329.0);
        assert_eq!(SHELF_INNER_H, 178.0);
        assert_eq!(SHELF_TOOLBAR_H, 28.0);
        assert_eq!(SHELF_TOOLBAR_GAP, 8.0);
        assert_eq!(SHELF_VIEW_H, 142.0);
        assert_eq!(SHELF_COLS, 4);
        assert_eq!(SHELF_TILE_H, 66.0);
        assert_eq!(SHELF_TILE_GAP, 6.0);
        assert_eq!(SHELF_TILE_W, 74.25);
        assert_eq!(SHELF_ROW_H, 72.0);
        assert_eq!(SHELF_SCROLL_W, 4.0);
        assert_eq!(SHELF_CONVERT_W, 36.0);
        assert_eq!(SHELF_CONVERT_H, 28.0);
        assert_eq!(SHELF_SOFT_CAP, 500);
    }

    /// Tooltip timing and tokens from the v0.6 shelf spec.
    #[test]
    fn tooltip_timing_matches_v06() {
        assert_eq!(TOOLTIP_DELAY_MS, 450);
        assert_eq!(TOOLTIP_NEIGHBOR_MS, 300);
        assert_eq!(TOOLTIP_GAP, 4.0);
        assert_eq!(TOOLTIP_R, 4.0);
        assert_eq!(TOOLTIP_FONT, 12.0);
        assert_eq!(TOOLTIP_SUB_FONT, 11.0);
    }

    /// Convert stack on the shared content rect: 28 header + 8 + 28 tabs +
    /// 8 + a 106 DIP grid that fills the column (178 exactly), five 59.4 DIP
    /// tiles per row, 8 DIP gaps.
    #[test]
    fn convert_stack_matches_v06() {
        assert_eq!(CONVERT_INNER_W, 329.0);
        assert_eq!(CONVERT_INNER_H, 178.0);
        assert_eq!(CONVERT_HEADER_H, 28.0);
        assert_eq!(CONVERT_TABS_H, 28.0);
        assert_eq!(CONVERT_OPTIONS_H, 106.0);
        assert_eq!(CONVERT_STACK_GAP, 8.0);
        assert_eq!(CONVERT_STACK_H, 178.0);
        assert_eq!(CONVERT_OPTION_COLS, 5);
        assert_eq!(CONVERT_OPTION_GAP, 8.0);
        assert_eq!(CONVERT_OPTION_W, 59.4);
        // One row takes the whole grid; two rows split it (49 DIP each) —
        // both checked at compile time with the layout invariants above.
        assert_eq!(CONVERT_OPTIONS_H, 106.0);
        assert_eq!((CONVERT_OPTIONS_H - CONVERT_OPTION_GAP) / 2.0, 49.0);
        assert_eq!(CONVERT_MAX_OPTIONS, 10);
    }

    /// Converting/Done geometry from the v0.6 artboards (section 4.5).
    #[test]
    fn convert_result_views_match_v06() {
        assert_eq!(CONVERT_STAGE_TILE, 56.0);
        assert_eq!(CONVERT_PROGRESS_H, 8.0);
        assert_eq!(CONVERT_PERCENT_FONT, 26.0);
        assert_eq!(CONVERT_STAGE_ICON, 26.0);
        assert_eq!(CONVERT_DONE_CHECK, 40.0);
        assert_eq!(CONVERT_DONE_COUNTDOWN_H, 3.0);
        assert_eq!(CONVERT_COUNTDOWN_MS, 5000);
    }
}
