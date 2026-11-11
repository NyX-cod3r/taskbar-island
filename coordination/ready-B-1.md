# ready-B-1 — Convert foundation (no UI)

Phase B1 of `TaskbarIsland-v0.8-AgentB-MiMo-Prompt.md` §8, on branch
`agent-b/v0.8.0`. Scope: resolver for the four new engines, capability probe,
pipeline model, router entries, options data table, unit tests with a fake
engine, `docs/engines.md`, list of missing engine files.

## What landed

- `src/engines/probe.rs` — resolver: `locate_engine` finds LibreOffice,
  Poppler, qpdf and Ghostscript in the documented layouts (exact, wrapper
  folder, extra nesting) with temp dirs + dummy exes; absent ⇒ `None`.
  `native_capabilities()` unchanged; `EngineId` +4, `FileKind` +Pdf/
  Spreadsheet/Presentation/Multi.
- `src/engines/mod.rs` — `Action` +7 groups (`PdfConvert`, `PdfTool`,
  `PdfExtract`, `SheetConvert`, `PresentConvert`, `Multi`, `Zip`);
  `DocumentFormat` +Pdf/Doc (ALL = 10), `TargetFormat` +Sheet/Present,
  timeouts, `EngineError::Cancelled` handling.
- `src/engines/router.rs` — candidate orders for every new action class,
  Native-first rules (sheet JSON → Native), one automatic retry on the second
  candidate (15 tests).
- `src/engines/output.rs` — `Workspace` (Drop = temp-dir removal), sheet/
  present target plumbing, preview naming.
- `src/convert/options.rs` — the full `TABLE` (~48 new rows across doc-pdf/
  doc-doc, zip-extract, PDF 6+7+4, Spreadsheet 8, Presentation 5, Multi 6),
  group A/B document targets, `source_applies()` (GIF-only MP4, ZIP-only
  Extract, group-B reduce), `is_own_format` for sheets/presentations,
  `tab_strip_hidden()`, `SourceKind::from_source`, `full_caps()` with all 8
  engines, `engine_hint()` §9 copy byte-for-byte (13 tests).
- `src/convert/detect.rs` — `SourceFormat` +12 variants (Heic/Heif/Doc/Xls/
  Xlsx/Ods/Csv/Tsv/Ppt/Pptx/Odp/Zip) with label/badge/icon arms;
  `ProbeKind` +Sheet/Present/Zip (PDF is convertible now — its "not
  supported" notice is gone); sniffing for HEIF brands, OLE2, the ZIP family,
  CSV/TSV; `file_kind()` → Pdf/Spreadsheet/Presentation (7 tests).
- `src/convert/jobs.rs` — `Step` + `run_pipeline` over `Workspace`:
  equal-weight progress marks `[1/3, 2/3, 1.0]` + a final `1.0` on delivery,
  cancel before/between/inside a step ⇒ `EngineError::Cancelled`, `finish`
  never runs, temp dir gone via `Workspace` Drop only (test 9). No
  `remove_*`/`DeleteFile*` call exists in the conversion path.
- `src/window.rs` — Convert-card mappings: `file_kind` for the four new
  kinds, `source_kinds` via `SourceKind::from_source`, stale
  "PDF input isn't supported" notice removed. (Full card UI is B2/B4.)
- `docs/engines.md` — layouts, licenses, process rules, missing-file list.
- `docs/convert-options.md` — the B1 spec (§2 table, §3 routing, §6 tests).
- `changelog.d/B-1-convert-foundation.md`.

## Commands run and results

| Command | Result |
| --- | --- |
| `cargo fmt --check` | exit 0 (after one `cargo fmt` normalisation) |
| `cargo clippy --all-targets -- -D warnings` | exit 0, 0 warnings (4 fixed: dead `tab_strip_hidden` → allow until B2, `type_complexity` → `StageRun` alias, `enum_variant_names` → renamed `Action::Multi`/`Action::Zip`) |
| `cargo test` | **204 passed, 0 failed, 1 ignored** (the `#[ignore]` live-engine test) |
| dependency denylist (inside `cargo test`) | `offline::tests::cargo_lock_has_no_network_crates`, `windows_features_do_not_include_networking`, `sources_do_not_open_sockets_or_http` all pass |
| `conversion_path_never_deletes_files` (inside `cargo test`) | pass — scanner sees no deletion call in `mod.rs`/`detect.rs`/`jobs.rs` production code |
| §6 tests 1–5, 7, 9 | all pass: options 13, detect 7, router 15, probe 7, pipeline 1 |

## Missing engine files for the owner (from `docs/engines.md`)

1. `tesseract\tessdata\eng.traineddata` (+ `osd.traineddata` recommended) —
   otherwise every OCR route is hidden ("0 OCR language(s)").
2. **LibreOffice** → `libreoffice\program\soffice.exe` (+ program files).
3. **Poppler** → `poppler\Library\bin\` with `pdftoppm.exe`, `pdftotext.exe`,
   `pdfimages.exe`, `pdfseparate.exe`, `pdfunite.exe` + DLLs.
4. **qpdf** → `qpdf\bin\qpdf.exe`.
5. **Ghostscript** → `ghostscript\bin\gswin64c.exe` + `lib\` + `Resource\`.

All five go under `ENGINES_DIR = %LOCALAPPDATA%\Filee\engines`. Absent engines
are not an error: their tiles hide, golden tests (§6/10, B4) print a skip line.

## Open items

- Ghostscript AGPL-3.0 keep/drop — owner decision (affects the Compress tile
  and the installer fragment; release notes will flag it).
- `IconId` keys for the two new tabs: `Compress` reused on PDF tools, `Plus`
  on Combine until the artboards provide keys (§7 open question 2).
- B2 needs `docs/CONTRACTS.md` (A-owned) + the artboards found in
  `D:\dynamic\.recon\boards\board0..6.png`.
