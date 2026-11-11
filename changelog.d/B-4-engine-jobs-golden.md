# Convert: four new engines, batch jobs, golden suite (B4)

## Added
- **Ghostscript runner** (`gswin64c.exe`): argv always starts with `-dSAFER`
  then `-dBATCH -dNOPAUSE -dQUIET`; PDF compress uses the `/ebook` preset and
  keeps the original with a "No gain — kept the original" note when the
  result is not smaller.
- **qpdf runner**: rotate left/right (`--rotate=+90:1-z`), optimize for web
  (`--linearize`), merge with selected.
- **Poppler runner**: text extraction with blank-page (scan) detection,
  every-page raster to a `<name>_pages` folder (`pdftoppm -r 150`),
  embedded images (`pdfimages -all`), split into pages, merge
  (`pdfunite`), OCR chain to Tesseract; sibling tools resolve next to the
  located executable.
- **LibreOffice runner**: the exact spec argv (headless, no restore/lock/logo,
  `--safe-mode`, private `-env:UserInstallation` profile with macros and
  link updates disabled), one job at a time behind a process-wide lock,
  180 s timeout; HTML → PDF uses the `writer_web_pdf_Export` filter.
- **Multi-file jobs** (`convert/multi.rs`): merge images into one native PDF,
  merge PDFs (Poppler or qpdf), ZIP from a selection, extract-here with
  zip-slip and entry/size/ratio caps, batch convert/resize/compress/strip
  metadata, native CSV → JSON.
- **Golden suite** (§9.2): 13 tests, one per options-table row, running the
  real engines from `ENGINES_DIR` on in-test fixtures. Each run asserts the
  output lands beside the source, the original stays bit-identical, magic
  bytes and non-zero size hold, no temp files or engine workspaces are left
  behind, and nothing is overwritten on a name collision. Rows whose engine
  is missing print `SKIP golden <row>: …` and pass, per spec.

## Changed
- **Hybrid card kind**: 2+ convertible files open the "Several files" card,
  an all-PDF batch stays on the PDF card, a single file keeps its own card.
- **Needs dimming**: an option that needs more input than it has (merge with
  one PDF, zip with one file, …) renders at 40 % opacity with a tooltip that
  says what is needed ("needs 2+ PDFs", "needs 2+ files") and ignores clicks.
- `router::route_source` reorders candidates by source container: markup goes
  to Pandoc first, HTML to LibreOffice's web filter, everything else to
  LibreOffice; markup → PDF runs the Pandoc → DOCX → LibreOffice chain.

## Offline / safety
- New runners build every command as an argv array (hostile file names stay
  one item; a leading dash gets a `.\` prefix), never a shell string; GS
  always runs with `-dSAFER`.
- The HTML golden fixture points at a remote image and runs under Pandoc's
  `--sandbox` — no network is opened (offline source audit still passes).
- Conversion never deletes or overwrites: staging temps are collision-safe
  `stem (2).ext` deliveries and cleanup happens only via workspace drop.
