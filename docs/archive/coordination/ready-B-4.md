# ready — B4 (engine jobs end to end + golden suite + card kind)

Phase: **B4 (prompt §4/§5/§9)** — complete.
Branch: `agent-b/v0.8.0`.

## What landed

### New engine runners
- `src/engines/ghostscript.rs` — `gswin64c.exe`, argv **always starts with
  `-dSAFER`** then `-dBATCH -dNOPAUSE -dQUIET`, `/ebook` compress preset with
  the §4 **no-gain contract** (result not smaller ⇒ original kept, Done view
  reports "No gain — kept the original", empty output path + note).
- `src/engines/qpdf.rs` — rotate (`--rotate=+90:1-z`), linearize for web,
  merge terminator handling.
- `src/engines/poppler.rs` — `pdftotext -layout` (blank-page detection
  separates scans from text pages), `pdftoppm -r 150` raster folder
  `<name>_pages`, `pdfimages -all`, `pdfseparate` split, `pdfunite` merge,
  sibling tools resolved next to the located executable, OCR language
  defaults to `eng`.
- `src/engines/libreoffice.rs` — exact §3 argv
  (`--headless --norestore --nolockcheck --nologo --nodefault --safe-mode
  -env:UserInstallation=file:///<private temp profile> --convert-to <filter>
  --outdir <tmp> <input>`), **one LO job at a time** (process-wide lock),
  180 s timeout, profile settings disable macros and link updates, HTML →
  PDF uses the `writer_web_pdf_Export` filter, leading-dash inputs prefixed
  `.\`, private profile URL uses forward slashes only.

### Routing, batch and delivery
- `router::route_source` — source-container re-ordering (markup → Pandoc,
  HTML → LO web filter, everything else → LO), PDF/Office/sheet/present arms,
  whole-batch bypass for merge/zip/extract.
- `src/convert/multi.rs` (new) — merge images (native one-PDF-per-image) and
  merge PDFs (Poppler or qpdf), ZIP from a selection, extract-here with
  zip-slip + entry/total/ratio caps, `batch_action` maps
  (batch convert/resize/compress/strip metadata), native CSV → JSON.
- Pandoc → DOCX → LibreOffice → PDF chain for markup PDF (LO gates the route).
- Collision-safe delivery: `stem (2).ext` naming, staging temp
  `.taskbar-island-<pid>-<n>.tmp` in the destination folder, folder outputs
  (`_pages`, images) delivered beside the source.

### UI (§4/§5)
- **Hybrid card kind**: `card_kind()` — 2+ convertible files → the
  "Several files" card, an all-PDF batch stays on the **PDF** card, a single
  file keeps its own card (wired at all four call sites; the "different file
  types" Notice, `mixed_kinds` and its tests were removed).
- **Needs dimming**: `needs_hint()` ("needs 2+ PDFs" / "needs 2+ files"),
  `ConvertOptionView.needs`, `design::CONVERT_NEEDS_A = 0.4` applied before
  drag-dim, tooltip shows the needs line instead of the engine hint, clicks
  on needs tiles are ignored (hits kept so the tooltip still shows).

### Tests (§9)
- **§9.2 golden suite** — `src/convert/golden.rs`, 13 tests, one per §4 row
  plus the collision test. Fixtures are built in-test (Pandoc DOCX/PPTX,
  FFmpeg tone/clip, pdf-writer text PDF, hand-made PNG/JPEG/MD/HTML/ZIP);
  every run asserts: output beside source, original bit-identical, magic
  bytes, non-zero size, no `.taskbar-island*` temps beside the fixtures, no
  engine workspace outlasting the run, no overwrite on collision. A missing
  engine **prints `SKIP golden <row>: …` and passes** (§9.2).
- **§9.4** — zip bomb refused before writing
  (`zip_bombs_are_refused_before_anything_is_written`), zip-slip + hostile
  names (`zip_slip_and_hostile_names_are_rejected`), hostile names as single
  argv items (`hostile_file_names_stay_single_argv_items` in engines/mod.rs,
  GS-specific twin in ghostscript.rs), HTML remote image runs under Pandoc
  `--sandbox` with no network, GS argv starts `-dSAFER`, LO profile disables
  macros/link updates.

## Gates
- `cargo fmt --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo test` — **256 passed / 0 failed / 1 ignored**.

## Golden evidence (`cargo test convert::golden -- --nocapture --test-threads=1`)
Ran on this machine (FFmpeg + Pandoc + Native present):
office `docx→md`, `docx→odt` · markup `md→docx`, `md→html`,
`html→docx (remote image)` · sheet `csv→json` · image `png→jpg` (white
corner asserted), `png→pdf`, `png→ico` · multi `merge images`, `zip
selection`, `batch convert`, `batch resize` · `extract zip` · video `remux
(stream copy)`, `mp4→mkv`, `mp3 from video`, `gif from video`, `mkv→mp4`
(collision → `clip (2).mp4`) · audio `wav→mp3`, `wav→flac`, `normalize` ·
`a_collision_never_overwrites` (sentinel survives, `photo (2).jpg`).

SKIP lines (engines the owner has not dropped into `ENGINES_DIR` yet):
- `LibreOffice is not installed` — docx→pdf, md→pdf, pdf→docx, pdf→odt,
  csv→pdf, csv→xlsx, pptx→pdf, pptx→odp, pptx→png
- `Poppler is not installed` — pdf→txt, pdf→md, pdf→png pages, pdf text,
  pdf embedded images, pdf page → png, pdf split, merge pdfs
- `qpdf is not installed` — pdf rotate left, pdf web-optimize
- `Ghostscript is not installed` — pdf compress
- `Tesseract is installed but has no language data
  (tesseract\tessdata\eng.traineddata)` — pdf make searchable

## Open items
- **Owner**: drop `libreoffice\`, `poppler\`, `qpdf\`, `ghostscript\` and
  `tesseract\tessdata\eng.traineddata` into
  `C:\Users\satya\AppData\Local\Filee\engines` — the skipped rows run
  automatically, no code change.
- **Owner**: Ghostscript is **AGPL-3.0** — keep or drop (B5 notices work
  either way).
- B4 screenshots (`docs/qa/B/`) deferred to the visual QA pass.
