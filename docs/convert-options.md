# Convert options table v0.8 — design draft (B1)

Design-system §4 (conversions), §5 (kinds/tabs), §9 (exact tooltip copy) mapped
onto the existing data table in `src/convert/options.rs`. **This is the spec the
B1 code implements**; the card never invents a tile, everything is one row in
`TABLE`.

Grounded in the current v0.7 code (read-only, from the shared tree):

- `options.rs` today: `OptionDef { kind, tab, action, format, icon, label }`,
  `TABLE`, `tabs()`, `options()`, `action_available()`, `engine_hint()`,
  `routable()`, own-format filtering via `SourceKind`.
- `engines/mod.rs` today: `EngineId { Native, Ffmpeg, Pandoc, Tesseract }`,
  `FileKind { Image, Video, Audio, Document, Other }`, `Action` (12 groups).
- `engines/router.rs` today: `route(kind, action, caps) -> Vec<Candidate>`,
  static cost ranks, Native-first, `run_with_fallback` = one automatic retry on
  the next candidate.
- `engines/probe.rs` today: `locate_engine()` with three hardcoded paths;
  `CapabilityTable` per engine.
- `convert/detect.rs` today: `SourceFormat` has `Pdf`, `Svg` but `kind()` maps
  them to `Unsupported`; no Office/sheet/presentation/zip formats at all.

## 1. Model deltas (what B1 adds)

| Area | Today | v0.8 |
| --- | --- | --- |
| `EngineId` | Native, Ffmpeg, Pandoc, Tesseract | + `LibreOffice`, `Poppler`, `Qpdf`, `Ghostscript` (`ALL` = 8) |
| `FileKind` | Image, Video, Audio, Document, Other | + `Pdf`, `Spreadsheet`, `Presentation`, `Multi` (several selected files) |
| `SourceFormat` | images incl. `Svg`/`Pdf`, video, audio, 8 document formats | + `Heic`, `Heif`, `Doc`, `Xls`, `Xlsx`, `Ods`, `Csv`, `Tsv`, `Ppt`, `Pptx`, `Odp`, `Zip` |
| `Tab` | ConvertTo, Optimize, Edit, Extract, ExtractText | + `PdfTools`, `Combine` (labels exactly `PDF tools`, `Combine` per §9) |
| `Action` | 12 image/video/audio/document groups | + `PdfConvert`, `PdfTool`, `PdfExtract`, `SheetConvert`, `PresentConvert`, `MultiAction`, `ZipAction` |
| `OptionDef` | kind/tab/action/format/icon/label | + `id: &'static str` (kebab, unique, used for UIA name + tests); availability stays router-driven |
| Jobs | one process per job (`jobs.rs`) | `Vec<Step>` pipeline, one temp dir per job, one progress bar over all steps, cancel kills the process tree + removes the temp dir |
| `locate_engine` | 3 fixed paths | tolerant search of known sub-folders per engine (see `docs/engines.md`) |

Open dependency question: SVG rasterisation needs a resvg-class crate (offline,
pure Rust). Must pass the dependency denylist test; if it does not, SVG tiles
are dropped and reported.

## 2. The table

Tile = what is drawn; tooltip = `label` + engine second line from §9; `needs` =
engines whose absence hides the tile; `builder` = job builder (pipeline) in
`jobs.rs`. "Cap" column: tiles before own-format/source filtering (hard limit
10 per tab, 5 columns × 2 rows).

### Image — kind `Image` (JPG, PNG, WebP, GIF, BMP, TIFF, ICO, AVIF, HEIC, SVG)

| Tab | Cap | Tiles (in display order) | needs | builder |
| --- | --- | --- | --- | --- |
| Convert to | 9 (+1 for GIF source) | PNG · JPG · WebP · AVIF · GIF · BMP · TIFF · ICO · PDF · *(GIF source only: MP4)* | Native (FFmpeg fallback for decode-only formats; HEIC via WIC, fallback FFmpeg; SVG via resvg) | `image_convert(fmt)` — single step |
| Optimize | 7 | Compress · Resize 75% · Resize 50% · Resize 25% · Fit to 1080p · Thumbnail 256 px · Remove metadata · Web-ready WebP | Native | `image_optimize(o)` |
| Edit | 7 | Rotate left · Rotate right · Flip horizontal · Flip vertical · Grayscale · Black & white · Crop to square | Native | `image_edit(e)` |
| Extract text | 6 | Copy text · Save as TXT · Save as Markdown · Save as DOCX · Searchable PDF · OCR language *(picker, `action: None`)* | Tesseract (+ Pandoc for MD/DOCX) | `image_ocr(out)` — chain for MD/DOCX: Tesseract → Pandoc, tooltip chain `Tesseract + Pandoc` |

HEIC note: if neither WIC (probe once at startup) nor FFmpeg decodes HEIC, the
HEIC *source* gets no Convert menu at all (tiles are not shown broken).

### Video — kind `Video`

| Tab | Cap | Tiles | needs | builder |
| --- | --- | --- | --- | --- |
| Convert to | 7 | MP4 · MKV · WebM · MOV · AVI · GIF · WMV | FFmpeg | `video_convert(fmt)` — MP4 adds `-movflags +faststart` |
| Optimize | 6 | Compress · Resize to 1080p · 720p · 480p · Remove audio · Remux | FFmpeg | `video_optimize(o)` |
| Edit | 5 | Rotate left · Rotate right · Speed 0.5× · 1.5× · 2× | FFmpeg | `video_edit(e)` |
| Extract | 5 | Audio MP3 · Audio WAV · Audio M4A · Frame at midpoint · GIF (first 5 s) | FFmpeg | `video_extract(x)` |

GPU encoding (optional, last): `h264_nvenc`/`h264_qsv`/`h264_amf` only after a
1-second self-test encode succeeds and is cached; otherwise
`libx264 -preset veryfast -crf 23`.

### Audio — kind `Audio`

| Tab | Cap | Tiles | needs | builder |
| --- | --- | --- | --- | --- |
| Convert to | 8 | MP3 · WAV · FLAC · M4A · OGG · OPUS · AIFF · WMA | FFmpeg | `audio_convert(fmt)` — MP3 uses `libmp3lame -q:a 2` |
| Optimize | 4 | Smaller bitrate · Mono · Normalize loudness · Trim silence | FFmpeg | `audio_optimize(o)` |

### Document — kind `Document`

Two source groups drive which tiles appear (beyond own-format hiding):

- **Group A** — DOCX, DOC, ODT, RTF (word-processing).
- **Group B** — MD, HTML, TXT, TEX, EPUB (markup/plain).
- **ZIP** — special, see §2.6.

| Tab | Cap | Tiles | Applies to | needs | builder |
| --- | --- | --- | --- | --- | --- |
| Convert to | 10 | PDF · MD · HTML · TXT · EPUB · ODT · RTF · TEX · DOCX · DOC | A: all 10 · B: **PDF, DOCX, HTML, EPUB, TXT only** | PDF → LibreOffice; DOC/DOCX → LibreOffice; rest → Pandoc | `doc_convert(fmt)` — B→PDF is a 2-step pipeline Pandoc → DOCX → LibreOffice; HTML → PDF uses LibreOffice `writer_web_pdf_Export` directly |
| Extract | 2 | Plain text · Embedded images | both | Pandoc (text) / Native (images) | `doc_extract(x)` |

Exactly 10 tiles at cap (5×2 grid); own-format hiding keeps visible ≤ 9.

### PDF — kind `Pdf` (new; today PDFs report "This type can't be converted yet.")

| Tab | Cap | Tiles | needs | builder |
| --- | --- | --- | --- | --- |
| Convert to | 6 | DOCX · ODT · TXT · MD · PNG · JPG | DOCX/ODT → LibreOffice; TXT/MD/PNG/JPG → Poppler (MD chain: Poppler + Pandoc) | see below |
| PDF tools | 7 | Compress · Rotate left · Rotate right · Split into pages · Merge with selected · Make searchable · Optimize for web | GS / qpdf / qpdf / Poppler / Poppler+qpdf / Tesseract+Poppler / qpdf | see below |
| Extract | 4 | Copy text · Text · Embedded images · Page → PNG | Poppler | `pdf_extract(x)` |

Builders and behaviour:

| Tile | Steps (`Vec<Step>`) | Notes |
| --- | --- | --- |
| DOCX | LibreOffice `--infilter="writer_pdf_import" --convert-to docx` | tooltip second line `via LibreOffice · layout may differ` (§9) |
| ODT | LibreOffice `writer_pdf_import` → odt | |
| TXT | Poppler `pdftotext -layout` | |
| MD | Poppler `pdftotext` → Pandoc | chain tooltip `Poppler + Pandoc`; text only |
| PNG / JPG | Poppler `pdftoppm -r 150 -png` | **every page**, output folder `<name>_pages` |
| Compress | Ghostscript `/ebook` (`-dSAFER -dBATCH -dNOPAUSE -dQUIET`) | if result ≥ original: keep original, Done view shows amber check + "no gain" |
| Rotate left/right | qpdf `--rotate=+90:1-z` / `--rotate=-90:1-z` | |
| Split into pages | Poppler `pdfseparate` | |
| Merge with selected | Poppler `pdfunite`, fallback qpdf | tile at 40% opacity with "needs 2+ PDFs" tooltip when < 2 selected |
| Make searchable | Poppler `pdftoppm` → Tesseract per page (`pdf` out) → `pdfunite` | chain tooltip `Tesseract + Poppler` |
| Optimize for web | qpdf `--linearize` | |
| Copy text / Text | Poppler `pdftotext` → clipboard / `.txt` | |
| Embedded images | Poppler `pdfimages -all` | |
| Page → PNG | Poppler `pdftoppm -f N -l N -r 150` | |

**Scanned-PDF detection** (runs once when the PDF is probed): `pdftotext` on the
first pages; text layer nearly empty ⇒ TXT/MD/DOCX route through OCR
(`pdftoppm -r 300 -png` → Tesseract → Pandoc) and the tooltip shows
`Tesseract + Poppler` (§4). Same for "Make searchable".

### Spreadsheet — kind `Spreadsheet` (XLSX, XLS, ODS, CSV) — one tab, hidden

| Tab | Cap | Tiles | needs | builder |
| --- | --- | --- | --- | --- |
| Convert to | 8 | PDF · CSV · XLSX · ODS · HTML · XLS · TSV · JSON | all except JSON → LibreOffice; **JSON → Native** (CSV → JSON built in) | `sheet_convert(fmt)` |

Own-format hiding: CSV source shows 7, XLSX source shows 7, etc.

### Presentation — kind `Presentation` (PPTX, PPT, ODP) — one tab, hidden

| Tab | Cap | Tiles | needs | builder |
| --- | --- | --- | --- | --- |
| Convert to | 5 | PDF · PPTX · PPT · ODP · PNG (slides) | all → LibreOffice; PNG slides → LibreOffice then Poppler (PDF → pages) | `present_convert(fmt)` — PNG is a 2-step pipeline |

### Several files — kind `Multi` (2+ selected) — one tab, hidden

Header shows a stack of thumbs + `N files`. Merge order = shelf order.

| Tab | Cap | Tiles | needs | builder |
| --- | --- | --- | --- | --- |
| Combine | 6 | Merge to PDF · ZIP · Batch convert · Batch resize · Batch compress · Batch strip metadata | Merge: images → Native, PDFs → Poppler/qpdf; ZIP → Native; batch → per-file router | `multi_merge()`, `multi_zip()`, `multi_batch(plan)` |

`Merge to PDF` needs ≥ 2 files (40% opacity otherwise). "ZIP from a selection"
(§4) is the `ZIP` tile here.

### ZIP source — kind `Document`, special-cased

Single tab (`Extract`), tab visible, one tile: **Extract here** — Native,
zip-slip safe, size and ratio caps, no overwrite. 7z/RAR out of scope.
(Artboard for a ZIP card is missing; if the artboard shows no ZIP card, this
row is dropped — see open questions.)

### Never shown (§4)

MOBI/AZW3, 7z/RAR, DWG, password-protected PDF/Office: **no tiles, no probing,
no shell-out**. A protected file that is attempted anyway fails with the amber
failure icon and tooltip `Password protected`.

## 3. Routing (candidate order per action class)

v0.6 rules unchanged: Native first when it can do the job, else lowest static
cost; one automatic retry on the next candidate; not-installed ⇒ option hidden.

| Action class | Candidate order |
| --- | --- |
| image convert/optimize/edit | Native (0) → Ffmpeg (10) |
| image OCR → TXT/clipboard | Tesseract |
| image OCR → MD/DOCX | Tesseract → Pandoc (chain) |
| HEIC decode | Native (WIC) → Ffmpeg |
| SVG → PNG/PDF | Native (resvg) |
| video/audio * | Ffmpeg |
| doc → MD/HTML/TXT/EPUB/TEX/RTF/ODT | Pandoc |
| doc → PDF (group B) | Pandoc → LibreOffice (chain) |
| doc → PDF/DOC/DOCX | LibreOffice |
| pdf → DOCX/ODT | LibreOffice |
| pdf → TXT/MD/PNG/JPG, extract * | Poppler (MD: Poppler → Pandoc chain) |
| pdf compress | Ghostscript |
| pdf rotate / linearize | qpdf |
| pdf split / merge | Poppler → qpdf |
| pdf searchable / scanned OCR | Poppler → Tesseract (chain) |
| sheet → anything except JSON | LibreOffice |
| sheet → JSON | Native |
| presentation → * | LibreOffice (PNG slides: LibreOffice → Poppler chain) |
| multi merge images / zip / batch | Native (batch delegates per file) |
| zip extract | Native |

Missing engine ⇒ `routable()` false ⇒ tile hidden, never an error dialog.

## 4. Tooltip copy (§9, exact)

- Tab labels: `Convert to · Optimize · Edit · Extract text · PDF tools · Extract · Combine`.
- Engine second lines: `via FFmpeg`, `via Pandoc`, `via Tesseract`,
  `via LibreOffice`, `via Poppler`, `via qpdf`, `via Ghostscript`;
  chains: `Tesseract + Pandoc` (image OCR → MD/DOCX), `Poppler + Pandoc`
  (PDF → MD), `Tesseract + Poppler` (scanned-PDF OCR route, "Make searchable").
- DOCX from PDF: `via LibreOffice · layout may differ`.
- Native work: **no** second line.
- Tile tooltips = the `label` column above (existing v0.7 labels kept); exact
  per-tile copy gets reconciled with the artboards when they arrive.

## 5. Visibility rules

1. Own format never offered (`is_own_format`, now incl. Pdf/Sheet/Presentation).
2. Tile hidden when no candidate engine is installed (`routable`).
3. Tile shown at 40% opacity when it needs more input than exists (Merge with
   < 2 PDFs, batch with 1 file) with a tooltip saying what is needed.
4. Tab hidden when it has zero tiles; **single tab ⇒ hidden** (§5: Spreadsheet,
   Presentation, Several files).
5. Unsupported sources never open the Convert card (no tile, no process).
6. Order inside a tab is `TABLE` order (grid fills 5 columns left→right).

## 6. Tests B1 must add (fake engine, no real exes)

1. **Routable**: every `TABLE` row is routable by ≥ 1 engine definition under
   full capabilities (fake `CapabilityTable` with all 8 engines).
2. **Cap**: no `(FileKind, Tab)` has > 10 tiles (pre-filter count), for every
   kind/tab pair including `Multi` and the GIF-source variant.
3. **Own format**: for every `SourceKind` value, its own format tile is absent.
4. **Tabs match §5**: kind → tab list is exactly the artboard list; single-tab
   kinds report hidden.
5. **Missing engine hides**: remove LibreOffice ⇒ PDF→DOCX/ODT, all sheet, all
   presentation tiles gone; remove Poppler ⇒ TXT/pages/extract gone; remove
   qpdf ⇒ rotate/linearize gone; remove Ghostscript ⇒ compress gone; remove
   Tesseract ⇒ all OCR tiles gone.
6. **Fallback**: each action class retries on the second candidate exactly once.
7. **Hints**: `engine_hint()` strings equal §9 copy byte-for-byte.
8. **Resolver**: `locate_engine` finds each new engine in the documented
   layouts (exact, wrapper folder, extra nesting) using temp dirs + dummy exes;
   absent ⇒ `None`.
9. **Pipeline**: cancel removes the temp dir; equal-weight progress sums to 1.0.
10. Golden conversions gated by `TI_ENGINES_DIR` (skip + printed skip line when
    an engine is missing) — B4.

## 7. Open questions (need owner/artboard input)

1. **Artboards are missing** — tile tooltips, the ZIP card, and the tab icon
   set (`PdfTools`, `Combine` need icon keys; `IconId` lives in `view.rs`,
   A-owned, so new variants are a §10.7 Request to A unless an existing icon
   fits) cannot be finalised without them.
2. `IconId` additions for the two new tabs: request `Wrench` (PDF tools) and
   `Layers` (Combine) from A, or reuse `Compress`/`Plus` — artboard decides.
3. Exact second line for Presentation → PNG slides (LibreOffice + Poppler
   chain is not among §9's three chains; propose `via LibreOffice · via Poppler`).
4. Whether ZIP gets its own card kind or folds into Document-Extract (§2.9).
5. Ghostscript keep/drop (AGPL, §6 of the brief) — owner decision, affects the
   Compress tile and the installer fragment.
