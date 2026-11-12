# ready — B5 (installer engine fragment, notices, changelog)

Phase: **B5 (prompt §6)** — complete.
Branch: `agent-b/v0.8.0`.

## What landed

### `installer/engines.iss` (new include fragment)
- Pulled in once by `scripts/installer.iss` from `[Components]`; the
  fragment switches to `[Files]` and `[Code]` on purpose (ISCC processes
  includes inline — verified against Inno Setup 6.7.3), and the main
  script reopens `[Files]`/`[Code]` afterwards without conflict.
- **Components** with per-engine disk sizes in every description:
  FFmpeg, Pandoc, Tesseract (+ OCR language sub-components, English
  default), Poppler, qpdf, Ghostscript — all `Types: full custom` — and
  **LibreOffice with no `Types:` line**, so it starts unticked in every
  type (owner decision 2026-11-10; it is the largest engine and is
  clearly sized: "…, largest engine").
- **File lists** ship each engine folder as-is to `{app}\engines\<name>`
  (the exact layout the resolver probed at build time), tesseract's
  `tessdata\*` excluded from the base entry and added per language
  component; `THIRD_PARTY_NOTICES.md` always ships to `{app}`.
- **Modify flow** ("Add or remove engines later"): tray →
  "Manage engines..." reruns the setup copy beside the app; a fresh
  install keeps the type defaults, a modify run restores the user's
  registry choices (`HKCU\Software\TaskbarIsland\Engines`), the
  selection is re-recorded after install, and deselected engines are
  removed — but **only for engines this build ships** (`#ifdef Have*`),
  so a build without an engine's payload never touches an earlier
  install of it.

### `scripts/engine-includes.ps1`
- Probes all seven engines by folder (recursive executable-name probe:
  `soffice.exe`, `pdftoppm.exe`, `qpdf.exe`, `gswin64c.exe` — wrapper
  directories count), computes integer-MB sizes, and writes
  `scripts/generated/engine-sizes.iss` with `EngineSize*` for all seven
  plus `Have*` for each new engine found.
- OCR language discovery unchanged (still `tessdata`-driven).

### `scripts/installer.iss`
- Reduced to Core: `[Components]` = core + `#include "..\installer\engines.iss"`,
  `[Files]` keeps only the exe, `[Code]` keeps only the app-level events
  (`InitializeWizard`, `CurStepChanged`, `CurUninstallStepChanged`) over
  `#include "update.iss"`. Header comment fixed to 0.8.0.

### `THIRD_PARTY_NOTICES.md`
- All seven engines: name, version, license, source, shipped files.
  Versions verified against the binaries on this machine (FFmpeg
  9.0.2 full build, Pandoc 3.12, Tesseract 5.4.0.20240606).
- FFmpeg build-specific GPL-3.0 note (from `-version` configuration),
  per-language tessdata licensing, Poppler build-dependent license, and
  the **Ghostscript AGPL-3.0** note with the owner's keep decision
  (2026-11-10) recorded.

## Self-test evidence
- `engine-includes.ps1` real payload → 7 sizes, **no `Have*`** (the four
  new engines are absent), 0 OCR languages.
- `engine-includes.ps1` fake 7-engine payload (incl.
  `poppler\wrapper\Library\bin\pdftoppm.exe` and 2 tessdata files) →
  all four `Have*` defined, wrapper-dir Poppler detected, language
  components generated for `eng`/`deu`.
- **ISCC real payload**: full compile OK (241 s) — components sized,
  base-3 engines packed, LibreOffice absent (no `HaveLibreOffice`).
- **ISCC fake full payload**: full compile OK — all four new engines'
  files packed (evidence: compression list includes
  `poppler\wrapper\…\pdftoppm.exe`, `qpdf\bin\qpdf.exe`,
  `ghostscript\bin\gswin64c.exe`, `libreoffice\program\soffice.exe`),
  LibreOffice component compiled without `Types`, doubled `[Code]`
  (fragment + main) and reopened `[Files]` accepted.
- `scripts/tests/update-smoke.iss` unaffected (includes only
  `update.iss`/`update-config.iss`).

## Gates
- `cargo fmt --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo test` — **256 passed / 0 failed / 1 ignored**.

## Open items
- **Owner**: drop `libreoffice\`, `poppler\`, `qpdf\`, `ghostscript\`
  and `tesseract\tessdata\eng.traineddata` into
  `C:\Users\satya\AppData\Local\Filee\engines` — the next installer
  build picks them up automatically (no code change), the golden SKIP
  rows start running, and the wizard gains the sized LibreOffice /
  Poppler / qpdf / Ghostscript checkboxes.
- B-scope phases B1–B5 are now all complete; B4 screenshots
  (`docs/qa/B/`) remain deferred to the visual QA pass.
