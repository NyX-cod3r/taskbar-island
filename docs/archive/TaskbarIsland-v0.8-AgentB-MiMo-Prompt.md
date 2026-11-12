# Agent B (opencode, MiMo) — Convert, PDF/Office engines, Clipboard previews

You are Agent B. Another agent (Agent A, Codex) works on the same repo at the same time on the shared foundation, Phone Link calls, and the release. You must not interrupt each other. Read `TaskbarIsland-v0.8-Design-System.md` and `design-tokens.json` first, completely; §10 is your working contract. The artboards in the design folder are your visual spec (Shelf*, Convert*, Clip*).

## 0. Your scope
1. **Padding:** adopt `card_content_rect()` in Shelf, Convert, Clipboard (inner padding 12 → 7 DIP, about −40%); body areas fill the freed space.
2. **Clipboard proper previews** (images can be told apart, links, colors, code, files).
3. **PDF and Office conversions:** add engines LibreOffice, Poppler, qpdf, Ghostscript to the engine layer, and add every common conversion from §4 that does not exist yet (if one already works, keep it).
4. Installer engine selection fragment and third-party notices.
You do NOT touch: tokens, layout, tooltip, rail, pill, Music/Timer/Call, audio, notifications, the main installer script, CI, versions, releases. Those are Agent A's. Anything you need from them goes under *Requests* in `coordination/from-B.md`.

## 1. Hard constraints
Pill and card sizes fixed. Fully offline: engines are local executables found in `ENGINES_DIR`; no downloads, no network, no telemetry. Icon-first UI; tooltips via the shared system only; exact tooltip strings from the design system §9. Output rules from v0.6 still apply (written next to the original, never overwrite, atomic temp-then-rename, original untouched, Recycle Bin undo).

## 2. Setup
1. Wait for Agent A's `docs/CONTRACTS.md` PR on `release/v0.8.0` before touching UI code (it names the tokens/layout/tooltip APIs). Until then work only on the non-UI parts of §4 (engines, router, tests) inside your own modules.
2. Worktree and branch as design system §10.1, `CARGO_TARGET_DIR=C:\build\ti-b`, `TASKBARISLAND_DATA_DIR` set to your own folder.
3. Create `coordination/from-B.md`. Use changelog fragments `changelog.d/B-*.md`.
4. **Engines folder.** `ENGINES_DIR = <PATH-I-GIVE-YOU>`. The three existing engines are there (`pandoc`, `ffmpeg`, `tesseract` with `tessdata`). Inspect it and report what else is present. The four new engines need to be placed there by me (you cannot download them; the app is offline). Expected layout, tell me exactly what is missing:
`libreoffice\program\soffice.exe` (portable/extracted LibreOffice) · `poppler\Library\bin\pdftoppm.exe, pdftotext.exe, pdfimages.exe, pdfseparate.exe, pdfunite.exe` (+ DLLs) · `qpdf\bin\qpdf.exe` · `ghostscript\bin\gswin64c.exe` (+ `lib`, `Resource`).
If a layout differs, make the resolver tolerant (search known subfolders), then document the final layout in `docs/engines.md`.

## 3. Engine layer additions
Extend the existing `Engine` trait and router (v0.6 rules stay: Native first when it can do the job, otherwise the lowest static cost; automatic single retry on the next candidate; an engine that is not installed hides its options, no error dialogs).
- New engines: `LibreOffice`, `Poppler`, `Qpdf`, `Ghostscript`.
- **Pipelines:** a job may be a chain of steps (for example Markdown → DOCX with Pandoc, then DOCX → PDF with LibreOffice). Model it as `Vec<Step>` with one temp directory per job, one progress bar over all steps (equal weights unless a step reports real progress), cancel kills the current process tree and removes the temp directory.
- **Process hygiene for all engines:** no shell, argv arrays only; a file name that starts with `-` is passed as `.\name`; `CREATE_NO_WINDOW`; Job Object with KILL_ON_JOB_CLOSE; captured output capped; low priority class; per-job timeout (LibreOffice 180 s, others scaled by size).
- **LibreOffice specifics:** run `soffice.exe --headless --norestore --nolockcheck --nologo --nodefault --safe-mode -env:UserInstallation=file:///<private temp profile> --convert-to <filter> --outdir <tmp> <input>`. A private profile per job (or a pooled one) so a running LibreOffice on the user's PC never conflicts. Disable macros and external-link updates in that profile. Cold start is slow (seconds): show the normal progress stages, and keep at most one LibreOffice job at a time.
- **Ghostscript specifics:** always `-dSAFER -dBATCH -dNOPAUSE -dQUIET`; never run PostScript from untrusted input without `-dSAFER`; AGPL, see §6.
- **Scanned-PDF detection:** run `pdftotext` on the first pages; if the text layer is nearly empty, route PDF → text/DOCX through OCR (`pdftoppm -r 300 -png` → Tesseract → Pandoc/qpdf) and show `Tesseract + Poppler` in the tooltip.
- **HEIC/HEIF:** decode through Windows WIC in the Native engine (works only when the OS "HEIF Image Extensions" is installed); probe once at startup; if neither WIC nor FFmpeg can decode HEIC, HEIC files simply get no Convert menu.
- **GPU encoding (optional, last):** FFmpeg `h264_nvenc`/`h264_qsv`/`h264_amf` only after a 1-second self-test encode succeeds and is cached; otherwise `libx264 -preset veryfast -crf 23`.

## 4. The conversions to support (add what is missing, keep what exists)
Tiles per type follow the artboards (5 columns, 2 rows, at most 10 tiles per tab). All options live in the single options data table (`FileKind → Tab → [Option]`), never scattered in UI code. Never offer the source's own format.

| Source | Targets / actions | Engine |
|---|---|---|
| **DOCX, DOC, ODT, RTF** | **PDF**, MD, HTML, TXT, EPUB, ODT, RTF, TEX, DOC/DOCX | PDF and DOC via LibreOffice, the rest Pandoc |
| **MD, HTML, TXT, TEX, EPUB** | **PDF** (Pandoc → DOCX → LibreOffice; HTML directly with the `writer_web_pdf_Export` filter), DOCX, HTML, EPUB, TXT | Pandoc (+ LibreOffice for PDF) |
| **PDF** | **DOCX** (`--infilter="writer_pdf_import"`, tooltip "layout may differ"; scanned → OCR route), ODT, TXT (`pdftotext -layout`), MD (text only), PNG/JPG (every page, `pdftoppm -r 150`, output folder `<name>_pages`) | LibreOffice, Poppler, Tesseract |
| **PDF tools** | Compress (Ghostscript `/ebook`; if the result is not smaller keep the original and report "no gain" in the Done view), Rotate left/right (qpdf `--rotate=+90:1-z`), Split into pages, Merge with selected (Poppler `pdfunite` or qpdf), Make searchable (scanned), Optimize for web (qpdf `--linearize`) | Ghostscript, qpdf, Poppler, Tesseract |
| **PDF extract** | Copy text, Text, Embedded images (`pdfimages -all`), Page → PNG | Poppler |
| **XLSX, XLS, ODS, CSV** | **PDF**, CSV, XLSX, ODS, HTML, XLS, TSV, JSON (CSV → JSON built in) | LibreOffice (+ Native for JSON) |
| **PPTX, PPT, ODP** | **PDF**, PPTX/PPT/ODP, slides → PNG (via PDF then Poppler) | LibreOffice (+ Poppler) |
| **Images** (JPG, PNG, WebP, GIF, BMP, TIFF, ICO, AVIF, **HEIC**, **SVG**) | existing list plus **HEIC → JPG/PNG**, **SVG → PNG/PDF** (resvg-class crate, offline), PNG → JPG on a white background when transparent, multi-size ICO (16–256), image → PDF | Native / FFmpeg |
| **Several selected files** | Merge images/PDFs into one PDF (shelf order), ZIP, batch convert, batch resize/compress/strip metadata | Native / Poppler |
| **ZIP** | Extract here (zip-slip safe, size and ratio caps), ZIP from a selection. 7z/RAR are out of scope | Native |
| **Video** | existing list plus clear "MP4" target (`-movflags +faststart`), Remux, MP3/WAV/M4A extraction, GIF from video, GIF → MP4, Mute, 1080/720/480p, Compress | FFmpeg |
| **Audio** | existing list plus MP3 (`libmp3lame -q:a 2`) for every non-MP3 source, Normalize, Mono, Smaller bitrate | FFmpeg |

Not supported (do not show, do not shell out to anything else): MOBI/AZW3, 7z/RAR, DWG, password-protected PDFs/Office files (no password prompt in this version). A protected file simply fails with the amber failure icon and the tooltip "Password protected".

Every option gets: id, icon key from the token icons, tooltip text (exact copy from design system §9 or the artboard), `needs` (engines), job builder. Tests assert every option is routable by at least one engine definition and no tab has more than 10 options.

## 5. Convert UI to build from artboards
- Shared shell for all kinds: header 28 (thumb 24, name, close), tabs 28 (**hidden when there is only one tab**), option grid fills the rest. Kinds and tabs: Image (Convert to, Optimize, Edit, Extract text), Video (Convert to, Optimize, Edit, Extract), Audio (Convert to, Optimize), Document (Convert to, Extract), **PDF (Convert to, PDF tools, Extract)**, **Spreadsheet (Convert to, one tab, hidden)**, **Presentation (Convert to)**, **Several files (one tab, header shows a stack of thumbs and "N files")**.
- Option tiles are text (format names) or an 18 px icon. A tile that needs more input than it has (for example "Merge with selected" with one PDF selected) is shown at 40% opacity with a tooltip that says what is needed.
- Option grid: 5 columns, gap 8, rows share the free height equally.
- Convert, Converting, Done, ConvertHover behave as in v0.6 with the new padding. The Done view shows `size · −NN%` (green) or `+NN%` (amber); Compress "no gain" shows an amber check.

## 6. Installer fragment and licenses
- Provide `installer/engines.<ext>` as an include fragment that Agent A's main script pulls in: component list (Core is A's; yours: FFmpeg, Pandoc, Tesseract + language sub-components with English default, Poppler, qpdf, Ghostscript, LibreOffice), each with its disk size computed from the folders, a default selection (all ticked except LibreOffice, which is the largest: ticked by default but clearly sized), file lists and destination `{app}\engines\<name>`. Also the "Add or remove engines later" modify flow.
- `THIRD_PARTY_NOTICES.md`: each engine's name, version, license and where the source is available. Known licenses: Pandoc GPL-2.0, FFmpeg LGPL/GPL (depends on build, read it from `-version`), Tesseract Apache-2.0, LibreOffice MPL-2.0, Poppler GPL-2.0/3.0, qpdf Apache-2.0, Ghostscript AGPL-3.0. They are separate programs launched as processes, not linked. **Tell me in your report that Ghostscript is AGPL** (and why that matters if I ever sell the app) so I can decide whether to keep it.

## 7. Clipboard previews (artboards Clip, ClipHover, ClipPreview)
Goal: every item can be told apart at a glance.
- **Row types:** text (single-line preview), code/command (mono; heuristic: multi-line or starts with a known command/keyword), **link** (domain in blue + muted path), **color** (swatch + hex in mono + `rgb(...)`; accept `#RGB`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `hsl()`), **image** (50 DIP row, 64 × 40 thumbnail, bold dimensions like `573 × 353`, muted line `PNG · 48 KB`), **file(s)** (file icon or a small thumbnail for a single image, first name + `+N` chip).
- Images smaller than the thumbnail show at natural size on a checkerboard (so a 37 × 13 image is obviously tiny). Images larger than the thumbnail scale to fit (cover with a letterbox, never stretch).
- **Hover preview (after 450 ms):** overlay inside the card (206 DIP wide): for images a large preview (192 × 118) with one caption line `615 × 342 · PNG · 212 KB`; for long text the first ~200 characters. Also record the source app (GetClipboardOwner → process → product name) and show it in the caption (`from Snipping Tool`); it stays local.
- **Thumbnails:** generated once at capture on a worker thread, stored at 128 px (for 2× DPI) beside the item, cached in memory (30 items max). Never decode full images on the UI thread.
- Row list: 34 DIP rows, image rows 50, gap 2, virtualized, vertical scroll only. Click copies back (ignore our own update), Enter the same, double-click or Shift+Enter copy and paste (non-activating flyout; send Ctrl+V only if the previous window is still foreground).
- Retention 7 days, max 30 items, pin exemption, pause capture, clear-history two-step confirm, privacy opt-outs, DPAPI encryption at rest: all as in the v0.7 prompt (they exist already; keep behavior, add the previews; migrate old items by generating missing thumbnails lazily).
- Tests: type detection table (text/code/link/color/file/image) including edge cases (`#fff`, `HTTP://`, URLs with spaces, 1-pixel images, huge images, corrupt clipboard bitmaps), thumbnail size and caching, preview overlay appears after 450 ms and never leaves the card, retention/capacity rules unchanged.

## 8. Phases (push each; tell A via your coordination file)
- **B1** (no UI; can start immediately): resolver for the four new engines, capability probe, pipeline model, router entries, options data table, unit tests with a fake engine, `docs/engines.md`, list of missing engine files for me.
- **B2** (after A's CONTRACTS PR): adopt `card_content_rect()` in Shelf/Convert/Clipboard; fill-height bodies; Shelf tile 66; option grid 8 gap; all existing cards visually match the artboards at the new padding.
- **B3**: clipboard previews and preview overlay.
- **B4**: new engines' jobs end to end (one real golden test per row of §4 where the engine exists), PDF/Office/sheet/presentation/multi UI.
- **B5**: installer fragment, notices, `changelog.d` fragments, final self-test.
Each phase ends with `coordination/ready-B-<phase>.md` containing the commands you ran and their results.

## 9. Testing (you own tests for your area; "properly" means all of this)
1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, dependency denylist (no network crates).
2. **Golden conversion tests** gated by `TI_ENGINES_DIR`; generate fixtures in the test itself (a tiny DOCX built with Pandoc from Markdown, a PNG made in code, a 1-page PDF made with LibreOffice or Ghostscript, a 2-second tone with FFmpeg), convert, and assert: output exists next to the source, original unchanged and bit-identical, correct magic bytes, non-zero size, no temp leftovers, no overwrite on name collision. Tests are skipped (not failed) when an engine is missing, and print which ones were skipped.
3. Router tests: every row of §4 maps to an expected engine order; missing engine hides options; fallback to the second candidate on failure; cancel kills processes and removes temp files.
4. Security tests: hostile names (leading `-`, spaces, quotes, very long, Unicode), zip-slip entries, zip bombs, oversized images, a PDF that makes Ghostscript run (must be sandboxed by `-dSAFER`), HTML with remote images (must not trigger network; LibreOffice with external links disabled).
5. Visual: screenshots of every card at 100/125/150% scaling compared to the artboards (measure: padding, tile sizes, text clipping), saved to `docs/qa/B/`.
6. Do not run the real app unless needed; take the lock first (design system §10.3). Do not rely on screenshots alone for behavior.

## 10. Do-not list
Do not edit Agent A's files or the shared interfaces. Do not push to `main` or `release/v0.8.0` directly; open PRs. Do not commit engine binaries or fixtures bigger than 100 KB. Do not add network access. Do not show a Convert option the router cannot run. Do not overwrite or delete the user's originals. Do not hardcode paths from my machine; use `ENGINES_DIR`. Do not mark a phase done without the ready file and evidence.
