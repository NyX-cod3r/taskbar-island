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
/// §8: the one gap every taskbar-placement rule names — the pill's left
/// margin beside the Widgets button, its gap to the last app icon and its
/// gap to the system tray. The spec writes it as 8 px at 100 % scale, which
/// is 8 DIP; it replaces the old fixed 12 DIP tray gap.
pub const SLOT_GAP_DIP: f32 = 8.0;
/// §8: a placement change moves the island with a 200 ms ease-out slide.
pub const SLIDE_MS: u32 = 200;
/// Ease-out curve for that slide: quick departure, gentle landing.
pub const EASE_SLIDE: [f32; 4] = [0.0, 0.0, 0.2, 1.0];
/// §8: taskbar structure changes settle for 150 ms before anchor.rs rescans,
/// so one icon shuffle (a pin, a window opening, a tray icon arriving) costs
/// one UIA walk instead of one per element event.
pub const SPACE_DEBOUNCE_MS: u32 = 150;
/// §8: a `TaskbarAl` change is different — Windows *slides* the icon group
/// into its new alignment over ~550 ms (measured on the owner's machine),
/// and a scan in the middle of that slide reports a half-way group, which
/// makes Auto pick the wrong band. Alignment changes therefore wait this
/// long (one-shot timer, not a poll) before the single rescan.
pub const ALIGN_DEBOUNCE_MS: u32 = 900;
/// §2: moving the taskbar to another edge arrives as a storm of
/// `LOCATIONCHANGE`s (the bar slides, resizes and settles), so an edge
/// change waits this long *after the last one* before it is applied. One
/// one-shot timer, never a poll: only the edge still current when it fires
/// flips the island.
pub const EDGE_DEBOUNCE_MS: u32 = 150;
/// The flyout sits closer to the pill than the v1.0 spec's 12 DIP: the taskbar
/// surface ends at the top of the band, and the owner asked for the card to
/// come down so it does not read as a detached popup.
pub const GAP_TASKBAR: f32 = 6.0;
/// Design-token name for the same 6 DIP gap between the card's bottom edge
/// and the taskbar top (`design-tokens.json` → `flyout.bottom_gap_dip`).
/// One value, two names: the token drift test checks this spelling.
#[allow(dead_code)] // consumed by the design-tokens drift test
pub const FLY_BOTTOM_GAP: f32 = GAP_TASKBAR;
/// §2 top edge: the card hangs this far below the taskbar's bottom edge
/// (the v0.12 brief's 12 px). The bottom edge keeps [`GAP_TASKBAR`]. The
/// island window's height is computed from *this* gap on both edges, so an
/// edge flip is a position-only move — the swapchain cannot resize at
/// runtime, and a top↔bottom flip must not need one.
pub const FLY_TOP_GAP: f32 = 12.0;
pub const WINDOW_W: f32 = 520.0;
pub const WINDOW_TOP_PAD: f32 = 40.0;
/// §3 the paused-edge notice card (360 px wide, per the brief): shown once
/// per edge change while the taskbar sits on a left or right edge, centred
/// in the island window, which stays its normal size so flipping back to the
/// top/bottom edge remains a position-only move.
pub const NOTICE_W: f32 = 360.0;
pub const NOTICE_H: f32 = 176.0;
/// The notice's own paddings and rhythm (DIP), used only by `render.rs`.
pub const NOTICE_PAD: f32 = 16.0;
pub const NOTICE_TITLE_DY: f32 = -1.0;
pub const NOTICE_BODY_GAP: f32 = 6.0;
pub const NOTICE_BUTTON_GAP: f32 = 14.0;
pub const NOTICE_BUTTON_W: f32 = 156.0;
pub const NOTICE_BUTTON_H: f32 = 44.0;
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
/// Left activity rail. v0.10 slims it to 48 design px (38.4 DIP at the
/// 400/500 = 0.8 design-to-repo scale), so the content column grows from
/// 329 DIP; v0.11 §1 takes the shared padding to 8 DIP, leaving 344.6 DIP.
/// The divider is the rail's right edge.
pub const RAIL_W: f32 = 38.4;
pub const RAIL_DIVIDER: f32 = 1.0;
pub const CARD_CONTENT_X: f32 = RAIL_W + RAIL_DIVIDER;
/// Uniform inner padding for every card: 8 DIP (v0.11 §1 "one shared grid";
/// was 7, originally 12). Every card's *root* rect is
/// `card_content_rect()` — no card may keep its own outer margin, not even
/// music: its rows fill the same root rect every other card uses.
pub const CARD_PAD: f32 = 8.0;
/// Gap between stacked content sections inside a card: 8 DIP (design tokens; was 12).
pub const CARD_GAP: f32 = 8.0;
/// Inner content rectangle of an expanded card: the 400×192 card minus the 38.4 DIP rail
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
/// v0.10 rail: five 36 × 36 design-px buttons (28.8 DIP at 125 %, the design's
/// own scale, so every hit target is at least 36 × 36 physical px) with 8 design
/// px gaps (6.4 DIP), a 10 design px top padding (8 DIP) and 18 design px
/// glyphs (14.4 DIP).
pub const RAIL_BTN: f32 = 28.8;
pub const RAIL_GAP: f32 = 6.4;
/// Top padding of the button stack (design: 10 px), no longer centred: the
/// design's stack leaves a wider gap at the bottom than at the top.
pub const RAIL_INSET: f32 = 8.0;
pub const RAIL_GLYPH: f32 = 14.4;
pub const RAIL_STROKE: f32 = 1.8;
/// Accent bar at the rail's left edge (design: 3 × 18).
pub const RAIL_IND_W: f32 = 2.4;
pub const RAIL_IND_H: f32 = 14.4;
pub const RAIL_IND_R: f32 = 2.0;
/// The indicator is centred on the button: (28.8 - 14.4) / 2 = 7.2 DIP.
pub const RAIL_IND_DY: f32 = 7.2;
pub const HEADER_GAP: f32 = 10.0;

// Music card (v0.11 §2 — "redesigned a third time")
/// v0.11 §1: the music card lays out in the *shared* root rect, exactly like
/// every other card — it adds no outer margin of its own (the design's extra
/// 4 DIP inner padding was dropped so the card's content lands on the one
/// grid line `--debug-layout` strokes in cyan). 344.6 × 176 DIP.
pub const MUSIC_ROOT_W: f32 = FLY_W - CARD_CONTENT_X - CARD_PAD * 2.0;
pub const MUSIC_ROOT_H: f32 = FLY_H - CARD_PAD * 2.0;
/// §2 "Rows: art and text 96" — 96 design px x 0.8 = 76.8 DIP.
pub const MUSIC_ART: f32 = 76.8;
pub const R_ART: f32 = 8.0;
/// Gap from the art tile to the text block (design 12 px).
pub const MUSIC_INFO_GAP: f32 = 9.6;
pub const MUSIC_LINE_GAP: f32 = 3.0;
/// Line 2 is `artist <gap> app` on one line (v0.9 §5): the gap between the
/// two segments (design 12 px), and the artist's share of the line so the app
/// name always keeps a visible tail.
pub const MUSIC_META_GAP: f32 = 9.6;
pub const MUSIC_META_ARTIST_MAX: f32 = 0.6;
/// §2 "wavy bar with times 34" (design px) = 27.2 DIP: a 16 DIP wave row
/// (20 design px) over the 11.2 DIP times line (`LINE_TIME`).
pub const MUSIC_PROGRESS_H: f32 = 16.0;
pub const MUSIC_PROGRESS_BLOCK: f32 = MUSIC_PROGRESS_H + LINE_TIME;
/// §7 "Flyout Motion" (unchanged in v0.11): wavelength 20, amplitude 3,
/// stroke 3, round cap.
pub const MUSIC_WAVE_AMP: f32 = 3.0;
/// Matches `WAVE_LEN` (the phase travels in whole wavelengths), so the wrap
/// of `wave_phase` lands exactly on a wave crest and never jumps.
pub const MUSIC_WAVE_LEN: f32 = 20.0;
pub const MUSIC_WAVE_STROKE: f32 = 3.0;
pub const MUSIC_TRACK_STROKE: f32 = 3.0;
/// §7 thumb 4 × 18 design px -> 3.2 × 14.4 DIP, so it still fits the 16 DIP
/// wave row that §2's 34 design px progress block leaves it.
pub const MUSIC_THUMB_W: f32 = 3.2;
pub const MUSIC_THUMB_H: f32 = 14.4;
/// Times boxes of the row under the wave (elapsed left, remaining right).
pub const MUSIC_TIME_W: f32 = 30.0;
pub const MUSIC_REMAIN_W: f32 = 34.0;
/// §2: the 36 × 36 output icon button at the top right of the text block; its
/// tooltip is the device the sheet would pick (v0.9 §4 sheet unchanged).
pub const MUSIC_OUTPUT: f32 = 28.8;
/// Room the title keeps before that button.
pub const MUSIC_BTN_GAP: f32 = 8.0;
/// §2 controls row — design px × 0.8: shuffle 44, previous / next 56,
/// play 76 × 56, repeat 44, and every button is 56 design px high.
pub const MUSIC_SHUFFLE_W: f32 = 35.2;
pub const MUSIC_SKIP_W: f32 = 44.8;
pub const MUSIC_CONTROLS_H: f32 = 44.8;
pub const MUSIC_PLAY_W: f32 = 60.8;
/// The play button shares the row height (`MUSIC_CONTROLS_H`), so it is
/// 60.8 x 44.8 DIP.
/// The five centred controls, and the four equal gaps that let them span
/// the whole interior (the row is centred, never left-packed with a chip).
pub const MUSIC_CTRL_TOTAL: f32 =
    MUSIC_SHUFFLE_W + MUSIC_SKIP_W + MUSIC_PLAY_W + MUSIC_SKIP_W + MUSIC_SHUFFLE_W;
pub const MUSIC_CTRL_GAP: f32 = (MUSIC_ROOT_W - MUSIC_CTRL_TOTAL) / 4.0;
pub const MUSIC_GLYPH: f32 = 16.0;
/// Equal gap between the three rows: art 76.8 + progress 27.2 + controls
/// 44.8 fill the 176 DIP shared root with 13.6 DIP between them.
pub const MUSIC_ROW_GAP: f32 =
    (MUSIC_ROOT_H - MUSIC_ART - MUSIC_PROGRESS_BLOCK - MUSIC_CONTROLS_H) / 2.0;

// Timer card (v0.11 §3). Both views are rows of the SAME shared content
// grid (`card_content_rect()`), so arming, starting, pausing or finishing
// the countdown never moves the shell by a pixel — only what each row
// contains changes. Row heights are the spec's design px x 0.8.
/// `card_content_rect(...).h`: the 192 DIP card minus CARD_PAD top/bottom.
pub const TIMER_CONTENT_H: f32 = FLY_H - CARD_PAD * 2.0;
/// Header row (design 48): leading tile, title over the sub-line, status
/// right-aligned.
pub const TIMER_HEADER_H: f32 = 38.4;
/// Header leading tile (design 44 — the call card's is 56).
pub const TIMER_LEAD: f32 = 35.2;
/// Set view stepper row (design 64): − / readout + caption / + as 64 x 64.
pub const TIMER_STEP: f32 = 51.2;
/// Set view preset row (design 36): 5 / 15 / 30 min, three equal chips.
pub const TIMER_PRESET_H: f32 = 28.8;
/// The three §3 presets; the one matching the armed duration turns amber.
pub const TIMER_PRESETS: [u32; 3] = [5, 15, 30];
/// Set view Start (design 52, amber, dark text).
pub const TIMER_START_H: f32 = 41.6;
/// Running view middle block (design 100): the 56 px time over the wavy bar.
pub const TIMER_RUN_BLOCK_H: f32 = 80.0;
/// Wavy bar at the bottom of that block: the §7 music wave + thumb, driven
/// by the same `ProgressVisual` (paused timers ease flat over 320 ms).
pub const TIMER_WAVE_H: f32 = 20.0;
/// Running view controls row (design 52): Pause, −1 min, +1 min, Stop.
pub const TIMER_RUN_BTN_H: f32 = 41.6;
/// The rows fill the root exactly: the set view is 48 + 64 + 36 + 52 design
/// px with three gaps, the running view 48 + 100 + 52 with two.
pub const TIMER_SET_GAP: f32 =
    (TIMER_CONTENT_H - TIMER_HEADER_H - TIMER_STEP - TIMER_PRESET_H - TIMER_START_H) / 3.0;
pub const TIMER_RUN_GAP: f32 =
    (TIMER_CONTENT_H - TIMER_HEADER_H - TIMER_RUN_BLOCK_H - TIMER_RUN_BTN_H) / 2.0;
/// Hold-to-repeat on the stepper: the first repeat after 420 ms, then one
/// every 110 ms until the button is released.
pub const TIMER_HOLD_FIRST_MS: u32 = 420;
pub const TIMER_HOLD_EVERY_MS: u32 = 110;
/// Duration in minutes: a 1-minute step inside 1..=180, 15 by default.
pub const TIMER_MIN_MINUTES: u32 = 1;
pub const TIMER_MAX_MINUTES: u32 = 180;
pub const TIMER_DEFAULT_MINUTES: u32 = 15;
/// Pausing flattens the wave over 320 ms instead of snapping it flat.
pub const TIMER_FLATTEN_MS: u32 = 320;

/// Clamp a minute count into the §3 stepper range.
pub fn clamp_minutes(minutes: i64) -> u32 {
    minutes.clamp(TIMER_MIN_MINUTES as i64, TIMER_MAX_MINUTES as i64) as u32
}

// Call card (v0.11 §4). Three fixed rows — header, box, action row — whose
// heights are the design's own px x 0.8 (60 / 60 / 66 design px). The rows
// keep those exact proportions and the two gaps share whatever the 176 DIP
// content column leaves over, so every phase fills the card with one shell.
/// `card_content_rect(...).h`: the 192 DIP card minus CARD_PAD top/bottom.
pub const CALL_CONTENT_H: f32 = FLY_H - CARD_PAD * 2.0;
/// Header row (design 60 px).
pub const CALL_HEADER_H: f32 = 48.0;
/// Connected box row (design 60 px) — drawn by every phase.
pub const CALL_BOX_H: f32 = 48.0;
/// Action row (design 66 px): Hold / End call / Mute, or Decline / Answer.
pub const CALL_BTN_ROW_H: f32 = 52.8;
/// The two gaps split the column's leftover, so rows + gaps = 176 exactly.
pub const CALL_ROW_GAP: f32 = (CALL_CONTENT_H - CALL_HEADER_H - CALL_BOX_H - CALL_BTN_ROW_H) / 2.0;
/// Header avatar tile: flush with the header row (design tile = 60 px).
pub const CALL_LEAD: f32 = CALL_HEADER_H;
/// Output button inside the connected box (design 132 x 44).
pub const CALL_OUTPUT_W: f32 = 105.6;
pub const CALL_OUTPUT_H: f32 = 35.2;
/// Green "Open Phone Link" button of the no-call box (design 157 x 49).
pub const CALL_OPEN_W: f32 = 111.2;
/// Ringing dot of the incoming box (design 17 px) and its right inset.
pub const CALL_DOT: f32 = 12.0;
pub const CALL_DOT_INSET: f32 = 12.8;
/// Gap between action buttons (design 10 px).
pub const CALL_BTN_GAP: f32 = 8.0;
/// Incoming: Decline + Answer each fill half the row, gap 8 (design 10).
pub const CALL_ANSWER_GAP: f32 = 8.0;
/// Phone Link confirmed the call is on hold (design §5: amber Resume).
pub const CALL_HOLD: Color = rgb(0xFF, 0xB5, 0x47);
/// Resting call-state header tile (artboards a-06 / a-08): dark green with
/// the bright `CALL_GREEN` glyph. Only an incoming call keeps the bright
/// tile (a-07: `CALL_GREEN` fill, dark glyph), so the card signals the ring
/// with color alone and the layout never moves.
pub const CALL_TILE_DIM: Color = rgb(0x27, 0x42, 0x31);
/// §4: the ringing pill breathes its border green once per interval.
pub const CALL_PULSE_MS: u32 = 900;
/// Signal glyph in the connected box (design 18 x 16).
pub const SIGNAL_W: f32 = 14.4;
pub const SIGNAL_H: f32 = 12.8;

// ------------------------------------------- audio output sheet (v0.9 §4)
// The sheet slides up inside the same flyout (240 ms, EASE_CONTENT), covers
// the content column only — the rail and its divider stay visible — and
// never changes the card's size. v0.11 §6 re-scales the v0.9 numbers from
// the artboards (design px × 0.8): pad 8, header/close 36, rows 44 on the
// music sheet and 52 on the call sheet, tile 32, all measured on a-03/a-09.
pub const SHEET_PAD_X: f32 = 6.4;
pub const SHEET_PAD_Y: f32 = 6.4;
pub const SHEET_TITLE: f32 = 11.2;
/// Header band height: the 36 design px close button the title centres on.
pub const SHEET_HEADER_H: f32 = 28.8;
pub const SHEET_CLOSE: f32 = 28.8;
/// Close glyph inside the button (design 16 px).
pub const SHEET_CLOSE_GLYPH: f32 = 12.8;
/// Gap between the header band and the first row (design 4 px).
pub const SHEET_HEADER_GAP: f32 = 3.2;
/// Music rows are 44 design px; the call sheet's device rows are 52.
pub const SHEET_ROW_H: f32 = 35.2;
pub const SHEET_ROW_H_CALL: f32 = 41.6;
pub const SHEET_ROW_GAP: f32 = 1.6;
/// Padding inside a row: tile and text both start 8 design px in.
pub const SHEET_ROW_INNER: f32 = 6.4;
pub const SHEET_TILE: f32 = 25.6;
/// Glyph inside a row's tile: sized so our 24-unit icons ink the
/// artboard's 12 design px (design nominal is a 16 px glyph in a 32 tile).
pub const SHEET_TILE_GLYPH: f32 = 11.2;
/// Tile → text gap (design 12 px).
pub const SHEET_TEXT_GAP: f32 = 9.6;
/// The check on the current route: design 15 px, 8 design px off the edge
/// (its centre lands 17.5 design px from the row's right, as measured).
pub const SHEET_CHECK: f32 = 12.0;
pub const SHEET_CHECK_INSET: f32 = 8.0;
pub const SHEET_NAME: f32 = 13.0;
pub const SHEET_SUB: f32 = 11.5;
/// Line boxes of a row's name / sub stack (design 16 / 14.5 px).
pub const LINE_SHEET_NAME: f32 = 16.0;
pub const LINE_SHEET_SUB: f32 = 14.5;
/// Sheet surface: always rgb(38,38,38), whatever theme the card is in.
pub const SHEET_BG: Color = rgb(38, 38, 38);
/// Slide-up duration: 240 ms, cubic-bezier(.2,.9,.25,1) — EASE_CONTENT.
pub const SHEET_MS: u32 = 240;
/// Rows each sheet shows at once: four 44 px music rows (184 DIP with the
/// header) or three 52 px call rows (166.4 DIP) fit inside the fixed
/// flyout; anything past this scrolls with the wheel.
pub const SHEET_VISIBLE_MUSIC: usize = 4;
pub const SHEET_VISIBLE_CALL: usize = 3;

// Output chips (v0.9 §4). v0.11 §2 drops the music chip from the controls
// row: the music card now carries a 36 x 36 output *icon* button at the top
// right of its text block (`MUSIC_OUTPUT`), and the call card keeps its
// in-box chip (inked with `FONT_OPEN_CALL`).

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

// Clipboard card: 28 DIP toolbar + 8 gap + list fills the rest (140 DIP).
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
/// stays inside the card (206 × 148 of the 344.6 × 176 content rect).
pub const CLIP_PREVIEW_MS: u32 = 450;
pub const CLIP_PREVIEW_W: f32 = 206.0;
pub const CLIP_PREVIEW_H: f32 = 148.0;
pub const CLIP_PREVIEW_PAD: f32 = 6.0;
pub const CLIP_PREVIEW_IMG_W: f32 = 192.0;
pub const CLIP_PREVIEW_IMG_H: f32 = 116.0;
/// Bands under the picture: one `LINE_HINT` caption line, then the shorter
/// `from <app>` line. Together with the pad and the picture they fill the
/// 148 DIP popup exactly, so the source line ends on its bottom edge.
pub const CLIP_PREVIEW_CAPTION_H: f32 = LINE_HINT;
pub const CLIP_PREVIEW_SOURCE_H: f32 = 12.0;
/// Hover/focus cluster: three 24 DIP icon buttons replace the age.
pub const CLIP_ACTION_BTN: f32 = 24.0;
pub const CLIP_ACTION_GAP: f32 = 2.0;
pub const CLIP_AGE_W: f32 = 30.0;
/// Small pin marker left of the age on a pinned row.
pub const CLIP_PIN_MARK: f32 = 12.0;
pub const CLIP_PIN_MARK_GAP: f32 = 4.0;
/// FILE badge on a file-sourced image row (v0.9 §2): a small chip at the
/// right of the row, sized like the shelf's format badge.
pub const CLIP_FILE_BADGE_TEXT: f32 = 9.5;
pub const CLIP_FILE_BADGE_H: f32 = 14.0;
pub const CLIP_FILE_BADGE_R: f32 = 3.0;
pub const CLIP_FILE_BADGE_PAD: f32 = 5.0;
/// Gap between the meta line's text box and the badge.
pub const CLIP_FILE_BADGE_GAP: f32 = 6.0;
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
/// 104 DIP, two rows of 48 DIP — never below the 40 DIP minimum).
pub const CONVERT_OPTIONS_H: f32 = 104.0;
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
// Owner 2026-11-19: the expanded card's shadow read as too harsh. Both layers
// are roughly halved in opacity and layer 1 spreads wider with less downward
// offset, so the falloff is softer instead of a dark band under the card.
// Extent stays inside SHADOW_HALO: 48 * 0.5 + 10 = 34 <= 44.
pub const SHADOW_1_DY: f32 = 10.0;
pub const SHADOW_1_BLUR: f32 = 48.0;
pub const SHADOW_1_A: f32 = 0.22;
pub const SHADOW_2_DY: f32 = 2.0;
pub const SHADOW_2_BLUR: f32 = 8.0;
pub const SHADOW_2_A: f32 = 0.15;
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
/// §7: the wave scrolls one wavelength every 700 ms.
pub const WAVE_MS_PER_LEN: f32 = 700.0;

// --------------------------------------------------------------- timings
/// §7 "Flyout Motion": the card morphs out of the pill rect over 460 ms on an
/// overshoot curve, its content fades in 160 ms later over 280 ms, and it
/// leaves over 260 ms on the standard ease. The card is always laid out at
/// its final size — the morph is a scale/translate of the whole surface, so
/// the interior never re-flows.
pub const FLYOUT_OPEN_MS: u32 = 460;
pub const FLYOUT_CLOSE_MS: u32 = 260;
pub const EASE_OPEN: [f32; 4] = [0.2, 1.1, 0.3, 1.0];
pub const EASE_CLOSE: [f32; 4] = [0.4, 0.0, 0.2, 1.0];
/// Content starts fading in this long after the open begins…
pub const FLYOUT_CONTENT_DELAY_MS: u32 = 160;
/// …and takes this long to land.
pub const FLYOUT_CONTENT_MS: u32 = 280;
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
/// Empty music art tile (v0.11 §2 artboard a-02): flat neutral fill with
/// a gray glyph — no gradient app icon, no glow.
pub const MUSIC_TILE_EMPTY: Color = rgb(0x2D, 0x2D, 0x2D);
/// Glyph on the empty music tile (artboard a-02).
pub const MUSIC_GLYPH_EMPTY: Color = rgb(0x8C, 0x8C, 0x8C);
pub const CALL_GREEN: Color = rgb(0x4A, 0xDE, 0x80);
/// Decline / End red (spec §7).
pub const CALL_RED: Color = rgb(0xC4, 0x2B, 0x1C);
pub const SHELF_BLUE: Color = rgb(0x60, 0xCD, 0xFF);
pub const TIMER_AMBER: Color = rgb(0xFF, 0xB5, 0x47);
/// Timer tile split (v0.11 §3 artboards a-04 / a-05): set, paused and
/// finished keep the bright `TIMER_AMBER` tile with a dark glyph (a-04);
/// a running countdown dims the tile to this warm shade and keeps the
/// glyph amber (a-05), so color alone carries the state and the layout
/// never moves.
pub const TIMER_TILE_DIM: Color = rgb(0x44, 0x39, 0x27);
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
/// v0.10: the rail's right border is white 8 % (black 8 % in light mode),
/// the same hairline the design uses on every surface.
pub const CARD_DIVIDER: Color = white(0.08);
pub const LIGHT_CARD_RAIL: Color = rgb(0xEA, 0xEA, 0xEA);
pub const LIGHT_CARD_DIVIDER: Color = black(0.08);

// Typography (section 4.3)
pub const FONT_FAMILY: &str = "Segoe UI Variable Text";
pub const FONT_FALLBACK: &str = "Segoe UI";
pub const FONT_PILL_LABEL: f32 = 12.0;
pub const FONT_PILL_VALUE: f32 = 12.0;
pub const FONT_FLY_TITLE: f32 = 15.0;
pub const FONT_FLY_SUBTITLE: f32 = 12.0;
/// v0.11 §2/§3: the times line under the wave (design 11 px x 0.8).
pub const FONT_TIME_LABEL: f32 = 8.8;
pub const FONT_BUTTON: f32 = 12.5;
pub const FONT_HINT: f32 = 11.0;
pub const FONT_FILE_NAME: f32 = 12.0;
pub const LINE_PILL: f32 = 16.0;
pub const LINE_FLY_TITLE: f32 = 20.0;
/// v0.11 §2: the times line box (design 14 px x 0.8) — the music progress
/// block is `MUSIC_PROGRESS_H` + this one line.
pub const LINE_TIME: f32 = 11.2;
pub const LINE_HINT: f32 = 14.0;

// Card typography (v0.4; v0.11 §2 scales the music type by 0.8).
pub const LINE_CARD_STATUS: f32 = 16.0;
/// §2: the track title (design 22 px semibold x 0.8).
pub const FONT_MUSIC_TITLE: f32 = 17.6;
pub const LINE_MUSIC_TITLE: f32 = 22.0;
/// §2: `artist <gap> app` under it (design 14 px x 0.8).
pub const FONT_MUSIC_META: f32 = 11.2;
pub const LINE_MUSIC_META: f32 = 15.0;
/// §3 set view readout (design 40 px x 0.8) and its line box; with the
/// caption line below it the pair is exactly the 64 design px stepper row.
pub const FONT_TIMER_SET: f32 = 32.0;
pub const LINE_TIMER_SET: f32 = 36.0;
/// §3 running view: the big centred time (design 56 px x 0.8).
pub const FONT_TIMER_RUN: f32 = 44.8;
pub const LINE_TIMER_RUN: f32 = 50.0;
pub const LINE_CALL_TITLE: f32 = 24.0;
pub const FONT_OPEN_CALL: f32 = 13.0;
/// Call card: the connected box's line 1 (design 16 px) and its meta line
/// (design 14 px), both scaled by 0.8 — every phase draws the same box, so
/// both lines use them.
pub const FONT_CALL_DEVICE: f32 = 13.0;
pub const LINE_CALL_DEVICE: f32 = 17.0;
pub const FONT_CALL_META: f32 = 11.0;
pub const LINE_CALL_META: f32 = 14.0;

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

    /// Linear interpolation between two rects (§7: the flyout morphs from the
    /// pill rect to the card rect).
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
            w: a.w + (b.w - a.w) * t,
            h: a.h + (b.h - a.h) * t,
        }
    }
}

// Layout invariants, checked at compile time in every build: the convert
// stack exactly fills the shared content column, the three cards share one
// inner width, and every grid row keeps the 40 DIP tile minimum (one row
// takes the whole 104 DIP grid, two rows split it 48/48).
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
// The 148 DIP popup exactly fills the pad, the picture and its two caption
// bands, and that height in turn equals the 8 DIP toolbar gap plus the 140 DIP
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

    /// Derived geometry is not exact in f32 once the rail leaves whole
    /// numbers (38.4 DIP), so value guards compare with a 0.01 DIP tolerance.
    fn close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.01,
            "expected {expected}, got {actual}"
        );
    }

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
    /// v0.11 §1: the shared padding is 8 DIP on all four sides, so the
    /// content rect is the card (400×192) minus the 38.4 DIP rail + 1 px
    /// divider minus CARD_PAD — 344.6 × 176 DIP of content.
    #[test]
    fn card_interior_matches_v08() {
        close(CARD_CONTENT_X, 39.4);
        assert_eq!(CARD_PAD, 8.0);
        assert_eq!(CARD_GAP, 8.0);
        let content = card_content_rect(RectD::new(0.0, 0.0, 400.0, 192.0));
        close(content.x, 47.4);
        assert_eq!(content.y, 8.0);
        close(content.w, 344.6);
        assert_eq!(content.h, 176.0);
        assert_eq!(RAIL_INSET, 8.0);
        // The stack is top-aligned at the 8 DIP padding (design: 10 px) and
        // still leaves at least a button-gap of breathing room at the bottom.
        let stack = RAIL_BTN * 5.0 + RAIL_GAP * 4.0;
        close(stack, 169.6);
        assert!(FLY_H - RAIL_INSET - stack >= 8.0, "stack must fit the card");
    }

    /// v0.11 §6: the output sheet covers the content column only — the rail
    /// and its divider stay visible — and its header + rows fit inside the
    /// fixed flyout without resizing it.
    #[test]
    fn output_sheet_fills_the_fixed_flyout() {
        assert_eq!(SHEET_BG, rgb(38, 38, 38));
        assert_eq!(SHEET_MS, 240, "the sheet rides EASE_CONTENT for 240 ms");
        // The panel starts past the rail *and* its divider, so the rail is
        // never covered (the artboard shows it left of the sheet at x48).
        assert_eq!(CARD_CONTENT_X, RAIL_W + RAIL_DIVIDER);
        let header = SHEET_PAD_Y + SHEET_HEADER_H + SHEET_HEADER_GAP;
        let music = header
            + SHEET_ROW_H * SHEET_VISIBLE_MUSIC as f32
            + SHEET_ROW_GAP * (SHEET_VISIBLE_MUSIC as f32 - 1.0);
        let call = header
            + SHEET_ROW_H_CALL * SHEET_VISIBLE_CALL as f32
            + SHEET_ROW_GAP * (SHEET_VISIBLE_CALL as f32 - 1.0);
        assert!(
            music <= FLY_H,
            "header + {SHEET_VISIBLE_MUSIC} music rows must fit: {music}"
        );
        assert!(
            call <= FLY_H,
            "header + {SHEET_VISIBLE_CALL} call rows must fit: {call}"
        );
        // Compile-time layout invariants (they depend on no runtime value).
        const {
            assert!(LINE_SHEET_NAME + LINE_SHEET_SUB <= SHEET_ROW_H);
            assert!(SHEET_TILE <= SHEET_ROW_H);
            assert!(SHEET_TILE <= SHEET_ROW_H_CALL);
        }
        assert_eq!(SHEET_VISIBLE_MUSIC, 4);
        assert_eq!(SHEET_VISIBLE_CALL, 3);
    }

    /// v0.11 §2: the music card's grid inside the shared root rect — the
    /// three rows (art 76.8 / progress block 27.2 / controls 44.8) fill the
    /// 176 DIP root with equal 13.6 DIP gaps, the five controls span the
    /// whole root through four equal gaps, and the output icon button is
    /// the design's 36 x 36.
    #[test]
    fn music_grid_fills_the_shared_interior() {
        close(MUSIC_ROOT_W, 344.6);
        close(MUSIC_ROOT_H, 176.0);
        // Row 1: art + text (design 96 px).
        close(MUSIC_ART, 76.8);
        // Row 2: the 16 DIP wave over the 11.2 DIP times line (design 34 px).
        close(MUSIC_PROGRESS_H, 16.0);
        close(MUSIC_PROGRESS_BLOCK, 27.2);
        // Row 3: the control row (design 56 px).
        close(MUSIC_CONTROLS_H, 44.8);
        close(MUSIC_ROW_GAP, 13.6);
        close(
            MUSIC_ART + MUSIC_PROGRESS_BLOCK + MUSIC_CONTROLS_H + MUSIC_ROW_GAP * 2.0,
            MUSIC_ROOT_H,
        );
        // Five centred controls (shuffle 35.2, skip 44.8, play 60.8 x 44.8).
        close(MUSIC_SHUFFLE_W, 35.2);
        close(MUSIC_SKIP_W, 44.8);
        close(MUSIC_PLAY_W, 60.8);
        close(MUSIC_CTRL_TOTAL, 220.8);
        close(MUSIC_CTRL_GAP, 30.95);
        // The 36 x 36 output icon button (v0.11 §2) top right of the text.
        close(MUSIC_OUTPUT, 28.8);
    }

    /// v0.9 §7: the wave (wavelength 20, amplitude 3, stroke 3, round cap,
    /// 700 ms per wavelength) and the open/close timings come straight from
    /// the brief (v0.11 keeps them); the thumb scales with the card so it
    /// still fits the 16 DIP wave row inside the fixed 176 DIP column.
    #[test]
    fn flyout_motion_and_wave_match_the_v09_brief() {
        assert_eq!(FLYOUT_OPEN_MS, 460);
        assert_eq!(FLYOUT_CLOSE_MS, 260);
        assert_eq!(FLYOUT_CONTENT_DELAY_MS, 160);
        assert_eq!(FLYOUT_CONTENT_MS, 280);
        assert_eq!(WAVE_MS_PER_LEN, 700.0);
        assert_eq!(WAVE_LEN, 20.0);
        assert_eq!(WAVE_AMP, 3.0);
        assert_eq!(MUSIC_WAVE_LEN, WAVE_LEN, "the phase wraps in wavelengths");
        assert_eq!(MUSIC_WAVE_AMP, WAVE_AMP);
        assert_eq!(MUSIC_WAVE_STROKE, 3.0);
        assert_eq!(MUSIC_TRACK_STROKE, 3.0);
        assert_eq!(MUSIC_THUMB_W, 3.2, "design 4 px x 0.8");
        assert_eq!(MUSIC_THUMB_H, 14.4, "design 18 px x 0.8");
        const {
            assert!(
                MUSIC_THUMB_H + 1.0 <= MUSIC_PROGRESS_H,
                "the thumb fits its wave row"
            );
            assert!(
                MUSIC_ROOT_H
                    == MUSIC_ART + MUSIC_PROGRESS_BLOCK + MUSIC_CONTROLS_H + MUSIC_ROW_GAP * 2.0
            );
        }
    }

    /// §3 layout guard: both timer views are rows of the shared 176 DIP
    /// content grid and fill it exactly — the set view 48 + 64 + 36 + 52
    /// design px in three gaps, the running view 48 + 100 + 52 in two — so
    /// the two views can swap without the card ever resizing.
    #[test]
    fn timer_views_fill_the_shared_content_grid() {
        const {
            assert!(TIMER_CONTENT_H == 176.0, "the shared row is 176 DIP");
            // A 0.01 DIP epsilon: the gaps are thirds of the leftover, so the
            // sum is only equal up to float rounding.
            let set =
                TIMER_HEADER_H + TIMER_SET_GAP * 3.0 + TIMER_STEP + TIMER_PRESET_H + TIMER_START_H;
            assert!(set > TIMER_CONTENT_H - 0.01 && set < TIMER_CONTENT_H + 0.01);
            let run = TIMER_HEADER_H + TIMER_RUN_GAP * 2.0 + TIMER_RUN_BLOCK_H + TIMER_RUN_BTN_H;
            assert!(run > TIMER_CONTENT_H - 0.01 && run < TIMER_CONTENT_H + 0.01);
            assert!(TIMER_SET_GAP > 0.0 && TIMER_RUN_GAP > 0.0);
            assert!(TIMER_LEAD <= TIMER_HEADER_H, "the tile fits its row");
            assert!(
                TIMER_WAVE_H < TIMER_RUN_BLOCK_H - LINE_TIMER_RUN,
                "the 56 px time and the wavy bar share the block"
            );
            assert!(LINE_TIMER_SET + LINE_MUSIC_META <= TIMER_STEP);
            assert!(TIMER_PRESETS[0] == 5 && TIMER_PRESETS[1] == 15 && TIMER_PRESETS[2] == 30);
            assert!(FONT_TIMER_SET == 32.0, "design 40 px x 0.8");
            assert!(FONT_TIMER_RUN == 44.8, "design 56 px x 0.8");
            assert!(TIMER_START_H == 41.6, "design 52 px x 0.8");
        }
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

    /// A1 layout guard: at 100 / 125 / 150 / 175 % the physical content rect
    /// is still the card inner minus the rail minus the padding, with every
    /// edge rounded the same way (round-half-up on DIP × scale) so no edge
    /// drifts a pixel between DPIs. The card itself stays 400 × 192 DIP.
    #[test]
    fn card_content_rect_is_card_inner_minus_rail_minus_padding_at_100_125_150_175() {
        let fly = RectD::new(10.0, 20.0, FLY_W, FLY_H);
        let content = card_content_rect(fly);
        let px = |v: f32, scale: f32| (v * scale + 0.5).floor() as i32;
        for dpi in [96u32, 120, 144, 168] {
            let scale = dpi as f32 / 96.0;
            assert_eq!(px(fly.w, scale), px(FLY_W, scale), "card width, dpi {dpi}");
            assert_eq!(px(fly.h, scale), px(FLY_H, scale), "card height, dpi {dpi}");
            assert_eq!(
                px(content.x, scale),
                px(fly.x + CARD_CONTENT_X + CARD_PAD, scale),
                "left edge, dpi {dpi}"
            );
            assert_eq!(
                px(content.right(), scale),
                px(fly.x + FLY_W - CARD_PAD, scale),
                "right edge, dpi {dpi}"
            );
            assert_eq!(
                px(content.y, scale),
                px(fly.y + CARD_PAD, scale),
                "top edge, dpi {dpi}"
            );
            assert_eq!(
                px(content.bottom(), scale),
                px(fly.y + FLY_H - CARD_PAD, scale),
                "bottom edge, dpi {dpi}"
            );
            // The rail column and its padding never eat into the content.
            assert!(px(content.x, scale) >= px(fly.x + CARD_CONTENT_X, scale));
            assert!(px(content.right(), scale) <= px(fly.x + FLY_W, scale));
            assert!(px(content.bottom(), scale) <= px(fly.y + FLY_H, scale));
        }
    }

    /// v0.10 rail: five 28.8 × 28.8 DIP buttons (36 × 36 physical px at the
    /// design's 125 % scale) with 6.4 DIP gaps, 14.4 DIP glyphs and a
    /// 2.4 × 14.4 accent bar centred on the button.
    #[test]
    fn rail_fits_five_tiles_matches_v07() {
        close(RAIL_W, 38.4);
        close(RAIL_BTN, 28.8);
        close(RAIL_GAP, 6.4);
        close(RAIL_GLYPH, 14.4);
        close(RAIL_IND_W, 2.4);
        close(RAIL_IND_H, 14.4);
        close(RAIL_IND_DY, 7.2);
        // Hit targets: 36 × 36 physical px at 125 % (the design scale) and
        // never smaller than the 36 design px the spec asks for.
        close(RAIL_BTN * 1.25, 36.0);
        let stack = RAIL_BTN * 5.0 + RAIL_GAP * 4.0;
        close(stack, 169.6);
        assert!(
            FLY_H - RAIL_INSET - stack >= 4.0,
            "five buttons need at least 4 DIP spare"
        );
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
        // The divider is the design's white 8 % hairline (black 8 % in light).
        assert_eq!(CARD_DIVIDER, white(0.08));
        assert_eq!(LIGHT_CARD_DIVIDER, black(0.08));
    }

    /// v0.11 §4 call card: three rows sized from the design's own px
    /// (60 / 60 / 66 x 0.8) with the two gaps sharing the rest of the 176 DIP
    /// content column, so every phase fills it exactly.
    #[test]
    fn call_card_fills_three_rows_of_the_content_column() {
        close(CALL_CONTENT_H, 176.0);
        close(CALL_HEADER_H, 48.0);
        close(CALL_BOX_H, 48.0);
        close(CALL_BTN_ROW_H, 52.8);
        close(CALL_ROW_GAP, 13.6);
        // The three rows and two gaps fill the column exactly: no dead band.
        close(
            CALL_HEADER_H + CALL_BOX_H + CALL_BTN_ROW_H + CALL_ROW_GAP * 2.0,
            CALL_CONTENT_H,
        );
        close(CALL_LEAD, 48.0);
        close(CALL_OUTPUT_W, 105.6);
        close(CALL_OUTPUT_H, 35.2);
        close(CALL_OPEN_W, 111.2);
        close(CALL_DOT, 12.0);
        close(CALL_DOT_INSET, 12.8);
        close(CALL_BTN_GAP, 8.0);
        close(CALL_ANSWER_GAP, 8.0);
        close(SIGNAL_W, 14.4);
        close(SIGNAL_H, 12.8);
        // Nothing may overflow the row it lives in — compile-time facts.
        const {
            assert!(CALL_LEAD <= CALL_HEADER_H, "avatar must fit the header row");
            assert!(CALL_OUTPUT_H <= CALL_BOX_H, "Output must fit the box");
            assert!(CALL_OPEN_W < CALL_CONTENT_H, "Open must fit the box");
            assert!(SHEET_ROW_H <= CALL_BOX_H, "sheet rows ride the box");
        }
        // Three action buttons with their gaps still fit the content width.
        let content = card_content_rect(RectD::new(0.0, 0.0, FLY_W, FLY_H));
        let each = (content.w - CALL_BTN_GAP * 2.0) / 3.0;
        assert!(each >= 60.0, "action button too narrow: {each}");
        let halves = (content.w - CALL_ANSWER_GAP) * 0.5;
        assert!(halves >= 60.0, "Decline/Answer too narrow: {halves}");
        assert!(CALL_OPEN_W < content.w, "Open Phone Link must fit the box");
        // Design §5: hold is amber, and the sheet rides the card without
        // growing the flyout (its fill is asserted by
        // `output_sheet_fills_the_fixed_flyout`).
        assert_eq!(CALL_HOLD, rgb(0xFF, 0xB5, 0x47));
        assert_eq!(SHEET_BG, rgb(38, 38, 38));
        assert_eq!(SHEET_MS, 240);
    }

    /// Clipboard card uses the same content column as the shelf.
    #[test]
    fn clipboard_card_matches_v07() {
        close(CLIP_INNER_W, 344.6);
        assert_eq!(CLIP_INNER_H, 176.0);
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
    /// 140 list, 34/50 DIP rows with a 2 DIP gap, 26 x 24 filter tabs in a
    /// 2 DIP padded strip, 24 DIP row actions.
    #[test]
    fn clipboard_list_matches_v07() {
        assert_eq!(CLIP_TOOLBAR_H, 28.0);
        assert_eq!(CLIP_TOOLBAR_GAP, 8.0);
        assert_eq!(CLIP_LIST_H, 140.0);
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

    /// Hover preview overlay (spec 7): 450 ms delay, 206 × 148 popup with a
    /// 192 × 116 image area, always inside the 344.6 × 176 content rect.
    #[test]
    fn clipboard_preview_overlay_matches_spec() {
        assert_eq!(CLIP_PREVIEW_MS, 450);
        assert_eq!(CLIP_PREVIEW_W, 206.0);
        assert_eq!(CLIP_PREVIEW_H, 148.0);
        assert_eq!(CLIP_PREVIEW_IMG_W, 192.0);
        assert_eq!(CLIP_PREVIEW_IMG_H, 116.0);
        // The picture column's fit is a compile-time `const _: () = assert!`
        // next to the tokens; what is worth pinning here is where it lands.
        // Horizontal centering, vertical fit: the popup sits on the toolbar's
        // bottom edge and ends exactly at the content rect's bottom, so it
        // covers the toolbar gap and the list and nothing else.
        let x = (CLIP_INNER_W - CLIP_PREVIEW_W) * 0.5;
        let y = CLIP_INNER_H - CLIP_PREVIEW_H;
        close(x, 69.3);
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
    /// toolbar, 8 DIP gap and a 4-column scrolling grid of 78.15 x 66 tiles
    /// with 6 DIP gaps, the viewport filling the rest (140 DIP = two rows).
    #[test]
    fn shelf_grid_matches_v06() {
        close(SHELF_INNER_W, 344.6);
        assert_eq!(SHELF_INNER_H, 176.0);
        assert_eq!(SHELF_TOOLBAR_H, 28.0);
        assert_eq!(SHELF_TOOLBAR_GAP, 8.0);
        assert_eq!(SHELF_VIEW_H, 140.0);
        assert_eq!(SHELF_COLS, 4);
        assert_eq!(SHELF_TILE_H, 66.0);
        assert_eq!(SHELF_TILE_GAP, 6.0);
        close(SHELF_TILE_W, 78.15);
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
    /// 8 + a 104 DIP grid that fills the column (176 exactly), five 62.52 DIP
    /// tiles per row, 8 DIP gaps.
    #[test]
    fn convert_stack_matches_v06() {
        close(CONVERT_INNER_W, 344.6);
        assert_eq!(CONVERT_INNER_H, 176.0);
        assert_eq!(CONVERT_HEADER_H, 28.0);
        assert_eq!(CONVERT_TABS_H, 28.0);
        assert_eq!(CONVERT_OPTIONS_H, 104.0);
        assert_eq!(CONVERT_STACK_GAP, 8.0);
        assert_eq!(CONVERT_STACK_H, 176.0);
        assert_eq!(CONVERT_OPTION_COLS, 5);
        assert_eq!(CONVERT_OPTION_GAP, 8.0);
        close(CONVERT_OPTION_W, 62.52);
        // One row takes the whole grid; two rows split it (48 DIP each) —
        // both checked at compile time with the layout invariants above.
        assert_eq!(CONVERT_OPTIONS_H, 104.0);
        assert_eq!((CONVERT_OPTIONS_H - CONVERT_OPTION_GAP) / 2.0, 48.0);
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
