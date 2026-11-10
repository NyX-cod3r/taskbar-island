# Installer: engine component fragment, sizes, modify flow (B5)

## Added
- **`installer/engines.iss`** — the include fragment the main installer
  pulls in from its `[Components]` section. It owns everything
  engine-related: the component list with per-engine disk sizes
  (`{#EngineSize…}`), payload file lists to `{app}\engines\<name>`, and
  the "add or remove engines later" modify flow (`SelectEngine`,
  `PreselectEngines`, `RecordEngines`, `RemoveDeselectedEngines`,
  `KeepSetupCopy` moved here from the main script). The main script keeps
  Core only.
- **Presence-driven components**: `scripts/engine-includes.ps1` now
  probes the payload for all seven engines (recursive executable-name
  probe, so a portable zip with an extra wrapper directory still counts)
  and writes `scripts/generated/engine-sizes.iss` — sizes for every
  engine plus a `Have*` define for each of the four new ones found. An
  engine whose payload is missing is not offered, not packed, and never
  touched by the modify flow, so a build made without it cannot delete an
  earlier install of it.
- **`THIRD_PARTY_NOTICES.md`** ships with every install (`{app}`): name,
  version, license and source for all seven engines, with the FFmpeg
  build-specific GPL-3.0 note, the per-language tessdata licensing note
  and Ghostscript's **AGPL-3.0** note.

## Changed
- `scripts/installer.iss` shrank to Core + app-level events: engine
  components, file lists and the engine registry/modify code all moved
  into the fragment (engine `[Code]` was already generic over engine
  names; the four new engines are guarded by `#ifdef Have*`).

## Owner decisions (2026-11-10)
- **LibreOffice starts unticked** on every fresh install: it is the
  largest engine and has no `Types:` entry at all, so no setup type
  selects it; it is clearly sized in its description and can be added
  any time via tray → "Manage engines...".
- **Ghostscript is kept** despite AGPL-3.0, documented in
  `THIRD_PARTY_NOTICES.md`, which ships with the installer.

## Verified
- `engine-includes.ps1` against the real payload (FFmpeg 260 MB /
  Pandoc 225 MB / Tesseract 160 MB, no `Have*`, 0 OCR languages) and
  against a fake 7-engine payload (all four `Have*` defined, wrapper-dir
  Poppler found, 2 OCR languages generated).
- ISCC 6.7.3 compiles both paths: real payload (base-3 components,
  241 s) and fake full payload (every `#ifdef` branch, all four new
  engines' files packed, LibreOffice component present without `Types`).
- `update-smoke.iss` still compiles: it includes only `update.iss` /
  `update-config.iss`, no overlap with the fragment.
- Gates: `cargo fmt --check` 0, `cargo clippy --all-targets -- -D
  warnings` 0, `cargo test` 256 passed / 0 failed / 1 ignored.
