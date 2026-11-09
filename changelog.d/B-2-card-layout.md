# B2 — Cards on the shared content rect: tokens, fill-height, tab strip, format chip

- **One content column for all three cards.** Shelf, Convert and Clipboard now
  lay out inside the shared `card_content_rect()` (12 DIP `CARD_PAD` on all four
  sides of the 400 × 192 flyout) instead of their own 12 DIP paddings — inner
  329 × 178 everywhere. Hit-testing (`shelf_viewport`, `shelf_track`,
  `clipboard_viewport`) derives from the same rect, so clicks and draws can no
  longer drift apart.
- **Design-token numbers adopted**: shelf toolbar gap 10 → 8, tile 62 → 66
  (74.25 × 66 cells, 72 DIP rows, viewport 142 = two rows), convert stack gap
  10 → 8, option grid 86 → 106 with an 8 DIP gap and 59.4 DIP columns, clipboard
  toolbar gap 10 → 10 → 8 with a 142 DIP list. Every card's stack now sums to
  exactly 178 DIP.
- **Body fills the rest (design §1).** The convert stack is top-aligned
  (header 28 → tabs 28 → grid) and the option grid takes whatever height is
  left; the shelf viewport is `inner.h - toolbar - gap` rather than a constant.
  Option tiles grow with the free height: one row = 106 DIP, two rows = 49 DIP
  each, never below the 40 DIP minimum (§5 "min 40 high, grows with free
  height").
- **§5 single-tab rule wired**: `options::tab_strip_hidden()` now drives the
  card — Spreadsheet, Presentation and Several files (and any card collapsed to
  one tab by a missing engine) draw no 28 DIP strip and the grid takes the
  space. The active tab still resolves from the full list, so switching back to
  a multi-tab kind is unaffected.
- **Header format chip**: the Convert header shows the source format ("JPG",
  "PDF") as a rounded 15 DIP chip in the format colour with white bold 10.5 px
  text between the file name and the close glyph (design §7 chip); the name
  truncates before it.
- **Layout invariants checked at compile time** (`const _: () = assert!(…)` in
  `design.rs` and `render.rs`): stack exactly fills the content column, the
  three cards share one inner width, two shelf rows fit the viewport, option
  rows keep the 40 DIP minimum, and the working/done layouts fit the 178 DIP
  column — they run in every build instead of only under `cargo test`.
- Tests updated to the token numbers; `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` (206 passed,
  0 failed, 1 ignored) all green.
