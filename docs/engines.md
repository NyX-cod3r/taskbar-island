# Engines

TaskbarIsland runs conversions by launching **local executables** as child
processes. There is no network access, no download and no telemetry: every
engine must already be present on disk under `ENGINES_DIR`.

## Where the engines come from

| Build | `ENGINES_DIR` resolves to |
| --- | --- |
| Installed (shipped) | `{app}\engines` next to `taskbar-island.exe` |
| Dev / test | `$TBI_ENGINES_DIR` if set, else `%LOCALAPPDATA%\Filee\engines` |

The resolver (`src/engines`) tolerates small layout differences by probing
known sub-folders instead of assuming one exact path (see
"Expected layout" below), so a portable zip extracted with or without an extra
wrapper directory still works.

## Inventory (checked 2026-11-08)

Root inspected: `C:\Users\satya\AppData\Local\Filee\engines` — 280 files,
644 MB.

| Engine | Present | Executable | Version | License |
| --- | --- | --- | --- | --- |
| FFmpeg | yes | `ffmpeg\bin\ffmpeg.exe` (+ `ffprobe`, `ffplay`, DLLs) | 9.0.2 full build (gyan.dev), `--enable-gpl --enable-version3` → **GPLv3** | GPL-3.0 (this build) |
| Pandoc | yes | `pandoc\pandoc.exe` (235 MB, single file + MANUAL/COPYING) | 3.12 | GPL-2.0 |
| Tesseract | yes | `tesseract\tesseract.exe` + ~70 DLLs | 5.4.0.20240606 (leptonica 1.84.1) | Apache-2.0 |
| Tesseract data | **MISSING** | `tesseract\tessdata\*.traineddata` | — | see below |
| LibreOffice | **MISSING** | `libreoffice\program\soffice.exe` | — | MPL-2.0 |
| Poppler | **MISSING** | `poppler\Library\bin\pdftoppm.exe` … | — | GPL-2.0/3.0 |
| qpdf | **MISSING** | `qpdf\bin\qpdf.exe` | — | Apache-2.0 |
| Ghostscript | **MISSING** | `ghostscript\bin\gswin64c.exe` | — | **AGPL-3.0** |

Nothing else exists in the root (plus an empty `.downloads\` folder). A
machine-wide search for `soffice.exe`, `pdftoppm.exe`, `qpdf.exe`,
`gswin64c.exe` and `*.traineddata` on C:, D: and E: found none — the four new
engines have to be placed into `ENGINES_DIR` by the owner; the app never
downloads them.

## Expected layout

Each engine folder may be extracted exactly as below or inside one extra
wrapper directory (the resolver searches known sub-folders):

```
ENGINES_DIR\
├── ffmpeg\bin\ffmpeg.exe (+ DLLs)
├── pandoc\pandoc.exe
├── tesseract\
│   ├── tesseract.exe (+ DLLs)
│   └── tessdata\
│       ├── eng.traineddata     ← required for OCR
│       └── osd.traineddata     ← optional (orientation/script detection)
├── libreoffice\program\soffice.exe (+ LibreOffice program files)
├── poppler\Library\bin\
│   ├── pdftoppm.exe  pdftotext.exe  pdfimages.exe
│   ├── pdfseparate.exe  pdfunite.exe  (+ DLLs)
├── qpdf\bin\qpdf.exe
└── ghostscript\bin\
    ├── gswin64c.exe
    ├── lib\        (Ghostscript libraries)
    └── Resource\   (Init, Font, ICC profiles …)
```

### Missing right now (what the owner needs to drop in)

1. **`tesseract\tessdata\eng.traineddata`** — without it OCR reports
   "0 OCR language(s)" and every OCR route (scanned PDF → text/DOCX, "Make
   searchable", image → text) is hidden. `osd.traineddata` recommended too.
   (Source: the `tessdata_fast` / `tessdata_best` repositories, `eng.traineddata`.)
2. **LibreOffice** — portable/extracted so that
   `libreoffice\program\soffice.exe` exists.
3. **Poppler** — Windows build so that
   `poppler\Library\bin\pdftoppm.exe`, `pdftotext.exe`, `pdfimages.exe`,
   `pdfseparate.exe`, `pdfunite.exe` and their DLLs exist.
4. **qpdf** — so that `qpdf\bin\qpdf.exe` exists.
5. **Ghostscript** — so that `ghostscript\bin\gswin64c.exe` exists with its
   `lib\` and `Resource\` folders.

An engine that is absent is not an error: its options are simply hidden from
the UI (design system §3 rules), and golden tests skip it with a printed skip
line.

## Process rules (all engines)

- No shell ever: argv arrays only, `CREATE_NO_WINDOW`.
- A file name starting with `-` is passed as `.\name`.
- Job Object with `KILL_ON_JOB_CLOSE`; captured output capped; low priority
  class; per-job timeout (LibreOffice 180 s, others scaled by input size).
- One temp directory per job; results are written next to the original
  (never overwrite, atomic temp-then-rename, original untouched).
- Cancel kills the process tree and removes the temp directory.

Engine-specific rules (LibreOffice private profile, Ghostscript `-dSAFER`,
scanned-PDF OCR routing, HEIC/WIC probing) are documented with the code in
`src/engines` once B1 lands.
