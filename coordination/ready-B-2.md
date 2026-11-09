# ready-B-2 — Card layout on the shared content rect

Phase B2 of `TaskbarIsland-v0.8-AgentB-MiMo-Prompt.md`, on branch
`agent-b/v0.8.0`. Scope: adopt `card_content_rect()` in all three cards,
fill-height layout per the design doc, the §5 single-tab rule, the header
format chip, and the design-token numbers.

## What landed

- `src/design.rs`
  - `SHELF_PAD` / `CONVERT_PAD` / `CLIP_PAD` (12 DIP each) removed; the three
    inner sizes are now `FLY - CARD_CONTENT_X - CARD_PAD * 2` = **329 × 178**
    and share one width.
  - Tokens: `SHELF_TOOLBAR_GAP` 10 → 8, `SHELF_TILE_H` 62 → 66 (→
    `SHELF_TILE_W` 74.25, `SHELF_ROW_H` 72, `SHELF_VIEW_H` 142),
    `CONVERT_STACK_GAP` 10 → 8, `CONVERT_OPTIONS_H` 86 → 106,
    `CONVERT_OPTION_GAP` 6 → 8 (→ `CONVERT_OPTION_W` 59.4),
    `CLIP_TOOLBAR_GAP` 10 → 8 (→ `CLIP_LIST_H` 142).
  - **Module-level `const _: () = assert!(…)` block** (6 invariants, runs in
    every build): stack == inner height, options height derives from the
    column, the three cards share one inner width, two shelf rows fit the
    viewport, and both the one-row (106) and two-row (49) option grids keep the
    40 DIP minimum. The same assertions were removed from the tests (they were
    tripping `clippy::assertions_on_constants`).
  - Tests updated: `clipboard_card_matches_v07` 319/168 → 329/178,
    `clipboard_list_matches_v07` gap 10/list 130 → 8/142,
    `shelf_grid_matches_v06` 12/319/168/10/130/62/71.75 → 329/178/8/142/66/
    74.25 + `SHELF_ROW_H` 72, `convert_stack_matches_v06` 86/10/162/59 →
    106/8/178/59.4 with the row-split checks referenced to the compile-time
    block.
- `src/render.rs`
  - `draw_card`: Convert and Clipboard build their area from the shared
    `content` rect (+ `input.top_dy`) instead of their own pads;
    `draw_shelf_card` derives `inner` from `view::content_rect(fly)` and the
    viewport from `inner.h - toolbar - gap` (fill-height, no constant).
  - `draw_convert_choice`: stack **top-aligned** (`stack_y = area.y`, design §1
    "body fills the rest"); **conditional tab strip** — `strip_h = 0` when
    `c.tabs` is empty, so the grid starts right under the header and fills to
    `area.bottom()`; **dynamic option rows** — `rows = visible.div_ceil(5)`,
    `tile_h = (grid.h - gap*(rows-1)) / rows` clamped to the 40 DIP minimum
    (one row 106, two rows 49); **header format chip** — `measure(badge,
    10.5, 600, 14) + 12` wide, 15 DIP tall, radius 4, `c.badge_color` fill,
    white bold centred text, name right edge shrinks by `badge_w + 12`.
  - **Module-level const-assert block** (5 invariants) moved out of `mod
    tests` (fixes `clippy::assertions_on_constants`) with the stale "168 DIP
    column" comment corrected to 178.
- `src/view.rs` — `clipboard_viewport` derives from
  `design::card_content_rect(flyout)` (test switched to `CARD_PAD`);
  `#[allow(dead_code)]` removed from `ConvertBody.badge` (now drawn).
- `src/window.rs` — tab-strip wiring §5: `all_tabs` keeps the full list for
  active-tab resolution, `tabs` is emptied when `mixed ||
  options::tab_strip_hidden(kind, caps, sources)`; `shelf_viewport` /
  `shelf_track` derive from `view::content_rect(self.flyout)`.
- `src/convert/options.rs` — `#[allow(dead_code)]` removed from
  `tab_strip_hidden` (now consumed).
- `changelog.d/B-2-card-layout.md`.

## Commands run and results

| Command | Result |
| --- | --- |
| `cargo fmt --check` | exit 0 (after one `cargo fmt` normalisation) |
| `cargo clippy --all-targets -- -D warnings` | exit 0, 0 warnings (the 4 dead consts + 3 `assertions_on_constants` from the previous run are fixed by the module-level assert blocks) |
| `cargo test` | **206 passed, 0 failed, 1 ignored** (was 204 before B2; no tests removed) |
| dependency denylist (inside `cargo test`) | pass — offline/feature/socket scanners unaffected by B2 |

## Open question for the owner — artboard hint copy not implemented

The seven artboards (`.recon\boards\board0..6.png`) carry hint/descriptor copy
that the design doc does not list in §4's allowed-text rules ("file names,
previews, numbers, format names on option tiles, one-line status chips") and
that `design-tokens.json` does not define. **Per the design doc being the
structural authority, these strings were NOT implemented in B2.** If the owner
wants them, they are a small follow-up; exact strings as drawn:

1. Shelf empty state: `Drop files here`
2. Shelf under a drag: `Hold Ctrl while dragging to convert`
3. Shelf tile hover line: `Drag out to copy or move. Ctrl+click to select. Click to open.`
4. Shelf with a selection: `1 selected · drag out, or press Convert`
5. Convert grid under a drag: `Drop the file on an option, or click one`
6. Convert during a drag: `Release to convert. Your original stays put.`
7. Done card (board3): `Saved next to the original` — this one is plain status
   copy and fits §4; it lands with the Done-state work in B4 if approved.

Tile descriptor lines under the format name (board4: `Lossless`, `Small`,
`Document`, `Icon`, …) are likewise superseded by §6 "format names on option
tiles" — B2 draws the format name only.

## Still open (carried from B1)

- Ghostscript AGPL-3.0 keep/drop — owner decision (Compress tile + installer
  fragment + release notes).
- Golden engine tests (§6/10, B4) skip until LibreOffice, Poppler, qpdf,
  Ghostscript and `tessdata\eng.traineddata` land in `ENGINES_DIR`.
- Clipboard artboards missing (boards cover Shelf/Convert only); Clip* UI keeps
  the existing card style.
- A's tooltip/pill API (`agent-a/v0.8.0` @ `14ac00a`) not yet merged — needed
  for B3 row/option tooltips.
