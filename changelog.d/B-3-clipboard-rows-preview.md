# Clipboard: typed rows, variable layout, 450 ms hover preview (B3)

## Added
- History rows now have a display type re-derived every frame (spec 7):
  **link**, **code**, **color**, **image**, **files** and plain text. The
  persisted `ClipKind` stays a hint only, so rows written by older builds
  classify too. Color rows draw a swatch with `#RRGGBB` in mono plus a muted
  `rgb(...)` line; link rows show the domain in the link accent with the path
  muted after it; code rows use the Cascadia Mono family.
- Image rows are 50 DIP tall with a 64 × 40 thumbnail over a light
  checkerboard (small images at their natural size, larger ones cover-cropped,
  never stretched) and bold `573 × 353` dimensions above a muted
  `PNG · 48 KB` line. Text/file/link/code/color rows stay 34 DIP, so the list
  uses mixed-height virtualization (`clip_strides` + variable `clip_scroll`).
- **Hover preview overlay**: after 450 ms of steady hover on a row, a 206 × 150
  panel appears inside the card, horizontally centered and seated on the
  toolbar's bottom edge (it covers the toolbar gap and the list, never a
  control). It shows the 192 × 118 picture (or the full text capped at 200
  characters), a `615 × 342 · PNG · 212 KB` caption and the `from <app>` line.
- Row source app: capture records the clipboard owner (`GetClipboardOwner` →
  process image name → friendly `Snipping Tool`/`Edge`/…), stored as optional
  `sourceApp` in the encrypted store; older rows simply have none.

## Changed
- Thumbnails are requested at an explicit box: 128 px for rows and shelf
  tiles, 384 px for the hover preview, with separate caches and a size-salted
  bitmap id so the two never collide. The preview holds only the live file.
- The hovered history row no longer shows the generic tooltip; its richer
  preview replaces it (keyboard focus on a row still carries the row tip).

## Offline / safety
- All decoding stays on the thumbnail worker; the UI thread never decodes.
- No new network surface; the offline source audit still passes.
