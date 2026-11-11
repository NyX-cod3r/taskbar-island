# ready — B3 (clipboard rows, variable layout, hover preview)

Phase: **B3 (spec §7)** — complete.
Branch: `agent-b/v0.8.0`.

## What landed
- `view::ClipRowKind` — display type re-derived every frame (text / code /
  link / color / image / files); persisted `ClipKind` is only a hint.
- Mixed-height rows: `CLIP_ROW_H` 34, `CLIP_ROW_IMG_H` 50, strides 36/52,
  gap 2; `clip_strides` + variable `clip_scroll` used by every scroll site
  and `reveal_clip_row`.
- Image rows: 64 × 40 thumb over the checkerboard (natural size when small,
  cover-crop when large, never stretched), bold dimensions + muted
  `PNG · 48 KB`.
- **Hover preview** (450 ms): 206 × 150 panel, horizontally centered, seated
  on the toolbar's bottom edge so it covers the toolbar gap + list only;
  192 × 118 image band, caption `615 × 342 · PNG · 212 KB`, `from <app>`
  source line, text rows preview as capped copy (200 chars).
- Two-size thumbnail pipeline: `ThumbRequest{path,max}`, `PREVIEW_PX 384`,
  size-salted `art_id`, separate `clip_preview_art` cache holding only the
  live preview; decode always on the worker.
- Source app: `GetClipboardOwner` → process stem → friendly name, stored as
  optional `sourceApp` (lenient JSON, older rows unaffected).

## Gates
- `cargo fmt --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo test` — **215 passed / 0 failed / 1 ignored** (offline denylist
  test included).

## Evidence (tests)
- `design::tests::clipboard_preview_overlay_matches_spec` — 450 ms, 206 × 150,
  x = 61.5, y = `CLIP_TOOLBAR_H`, band sums, bottom alignment.
- `view::tests::preview_overlay_geometry_stays_inside_the_card` — placement
  inside the content rect.
- `view::tests::hover_preview_delay_body_and_source` — 449/450 boundary,
  image caption string, text cap, blob path, `from <app>` propagation.
- `view::tests::clip_rows_virtualize_thirty_mixed_items` — mixed strides,
  content height, image row at the end of the scroll.
- `thumb::tests::preview_decode_is_bigger_and_gets_its_own_art_id` — 128 vs
  384 px decodes with distinct bitmap ids.
- `clipboard::store::tests::json_round_trips_items_and_high_water_mark` +
  legacy-row test — `sourceApp` round-trip and leniency.

## Open questions
- Clipboard artboards were absent, so the overlay placement follows the
  design tokens (geometry tests pin the interpretation). Visual review
  pending owner.
