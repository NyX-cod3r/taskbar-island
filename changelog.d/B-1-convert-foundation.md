# B1 — Convert foundation: engines, router, options table

- **Four new local engines**: LibreOffice, Poppler, qpdf and Ghostscript join
  FFmpeg/Pandoc/Tesseract. The resolver finds them under `ENGINES_DIR` in the
  documented layouts (`docs/engines.md`), the capability table probes what is
  actually installed, and a missing engine simply hides its tiles — the app
  stays fully offline.
- **The Convert card is data-driven end to end**: one `TABLE` row per tile now
  covers all eight source kinds with the artboard tabs `Convert to · Optimize ·
  Edit · Extract text · PDF tools · Extract · Combine`. New rows: PDF
  (DOCX/ODT/TXT/MD/PNG/JPG + Compress/Rotate/Split/Merge/Make searchable/
  Optimize for web + Copy text/Text/Images/Page→PNG), spreadsheets (PDF/CSV/
  XLSX/ODS/HTML/XLS/TSV/JSON), presentations (PDF/PPTX/PPT/ODP/PNG), legacy
  DOC, group A/B document targets, ZIP "Extract here", and the Several-files
  Combine tab (Merge to PDF, ZIP, batch convert/resize/compress/strip
  metadata).
- **New inputs detected from magic bytes**: HEIC/HEIF photos, OLE2 binaries
  (DOC/XLS/PPT), the ZIP family told apart by entry names (DOCX/XLSX/PPTX/ODS/
  ODP/ODT/EPUB vs plain ZIP), CSV/TSV, and PDF is now a first-class convertible
  card instead of "not supported".
- **Router** gained the new action classes (`PdfConvert`, `PdfTool`,
  `PdfExtract`, `SheetConvert`, `PresentConvert`, `Multi`, `Zip`) with the
  documented candidate orders and one automatic fallback retry.
- **Multi-step pipeline model** (`Step` + `run_pipeline`): one temp workspace
  per job, equal-weight progress that reaches exactly 1.0, cancel stops the run
  before delivery and the workspace drop removes the temp dir — the conversion
  path contains no deletion call at all.
- **Exact §9 tooltip second lines**, byte-for-byte: `via FFmpeg`/`via
  Pandoc`/`via Tesseract`/`via LibreOffice`/`via Poppler`/`via qpdf`/`via
  Ghostscript`, the chains `Tesseract + Pandoc`, `Poppler + Pandoc`,
  `Tesseract + Poppler`, `via LibreOffice · layout may differ` for DOCX from
  PDF, and no second line for Native work.
