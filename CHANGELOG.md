# Changelog

All notable changes to TaskbarIsland are recorded here. The format follows
Keep a Changelog; versions follow the owner's x.y.z scheme (the app stays on
0.x until the owner approves 1.0.0).

## 0.8.0 (2026-11-11)

### Added

- **Clipboard history activity (new).** A fifth activity captures text,
  links, code, colours, images and file lists through the real clipboard
  listener and keeps an encrypted, local-only history. Rows have a display
  type re-derived every frame (link, code, colour, image, files, text), mixed
  34/50 DIP heights, image thumbnails on a checkerboard, the clipboard owner
  as an optional `from <app>` line, and a 450 ms hover preview (206 x 150)
  that replaces the generic row tooltip. Captures expire after 7 days and the
  history is capped at 30 entries; the compact pill shows a transient `+N`
  badge for the batch that just landed.
- **Convert card (new).** A data-driven Convert card for eight source
  kinds (PDF, Word, spreadsheet, presentation, image, audio/video, text,
  archive) with the artboard tabs *Convert to / Optimize / Edit / Extract
  text / PDF tools / Extract / Combine*. Four new local engines join
  FFmpeg/Pandoc/Tesseract: **LibreOffice, Poppler, qpdf and Ghostscript**.
  Multi-step jobs run in a per-job workspace with equal-weight progress,
  cancellation, collision-safe delivery (`stem (2).ext`), and no deletion or
  overwrite of the user's original. Batch jobs cover image/PDF merge, ZIP
  create and extract (zip-slip, entry/size/ratio caps), batch
  convert/resize/compress/strip-metadata and native CSV to JSON. Options
  that need more input render at 40 % with a tooltip explaining what is
  missing.
- **Honest Phone Link call handling.** The Call activity derives its state
  offline from the per-user Phone Link package and its
  `DeviceMetadataStorage.json` (`"IsLinked"`): *not installed*, *not signed
  in*, *phone not connected* or *available*. When the phone cannot be used,
  the card is a single muted icon whose tooltip names the missing condition —
  no fake signal bars and no controls that have nothing to drive them. A live
  call keeps its ticking mm:ss clock, real microphone/speaker mute, and
  priority on the pill over every other activity; clicking the call pill
  opens the Call card, and the idle "Open Phone Link" button launches the
  real app.
- **Pill priority and live clocks.** The pill resolves the highest-priority
  live state independently of the open card (live call > finished timer >
  music > running timer > clipboard pop > non-empty shelf > idle). An open
  card is never yanked away by a background change; wheel/rail switches are
  one-shot user actions. Call, timer and music clocks/animations tick only
  while their state is on screen.
- **Design-system foundation.** v0.8 tokens are checked in and validated at
  test time; every card lays out in one shared `card_content_rect()`
  (400 x 192 flyout, 57 DIP rail column, 7 DIP card padding), with
  compile-time invariants for the stack heights, shelf rows and option rows.
  Version 0.8.0 lives once in `Cargo.toml` and is asserted everywhere by
  `version_consistency`; the offline source audit still passes.
- **Installer engine components.** The Inno Setup installer now offers each
  engine as a sized component (Core required; FFmpeg, Pandoc, Tesseract +
  languages, Poppler, qpdf, Ghostscript, LibreOffice) and can add or remove
  engines later from the tray's "Manage engines..." flow. Engines are
  embedded in the installer, copied only when ticked, and an engine whose
  payload is missing is never offered, packed or deleted. An explicit
  `/COMPONENTS` list from a scripted install now wins over the remembered
  selection; uninstall removes the install, shortcut, Run value and engine
  registry but keeps the user's data folders.
- **Third-party notices.** `THIRD_PARTY_NOTICES.md` ships with every install
  and lists all seven engines with version, license and source, including
  the FFmpeg GPL-3.0 build note, per-language tessdata licensing and
  **Ghostscript's AGPL-3.0**.

### Changed

- **Card padding 12 → 7 DIP** and all three cards now share the same content
  column instead of their own paddings. Shelf tiles 62 → 66 DIP, shelf
  toolbar gap 10 → 8, convert option grid 86 → 106 DIP with 8 DIP gaps,
  clipboard toolbar gap 10 → 8, clipboard list 142 DIP; each stack sums to
  exactly 178 DIP.
- **Icon-first UI**: the compact pill and card leading tiles use the four
  activity identity icons (rendered natively by Direct2D) instead of flat
  accent tiles; music still shows the real album cover when the session
  provides one.
- **Tooltips** appear after the 450 ms delay (immediate for a neighbouring
  control), flip to the left of their control when the tip would clip the
  screen edge, and cover the new rail, option, row-action and timer-button
  targets. The open card owns the wheel: only the shelf/clipboard lists
  scroll; other wheels over an open card do nothing.
- **Call idle state** is reduced to the honest muted icon described above;
  idle cards no longer draw signal bars.
- **LibreOffice is unticked on every fresh install** (it is the largest
  engine and has no setup-type entry); Ghostscript is kept with its AGPL
  notice in `THIRD_PARTY_NOTICES.md`.
- The tray menu can open the engine maintenance flow, and the installer
  upgrade path keeps the user's existing engine selection.

### Known limitations

- **PDF → DOCX and Office-family conversions are approximate.** LibreOffice
  re-lays the document; complex layouts can differ. Tooltips say
  `via LibreOffice · layout may differ` where that applies.
- **Call control depends on Phone Link.** The A2 spike returned NO-GO for
  call detection, notification/banner suppression and call actions without a
  linked phone to validate against; audio is PARTIAL. Release builds ship no
  call simulator and no fake answer/decline/end controls. Real incoming-call
  validation was not possible on this machine and is recorded as not run.
- **Engine conversions that need LibreOffice, Poppler, qpdf, Ghostscript or
  `tessdata` are skipped** on a machine where those payloads are absent
  (this release's gate run skipped 22 golden rows). The app stays fully
  offline and simply hides tiles whose engine is missing.
- The installer is unsigned, so Windows SmartScreen may warn on first run.
- The app is tested at 100 %, 125 % and 150 % scaling with the taskbar at the
  bottom (including auto-hide). A live scaling change while the app is
  running is not applied until the app restarts; Windows sends no
  `WM_DPICHANGED` to this window.
- The <15 MB RAM target is not reachable with the mandated DirectComposition
  + Direct2D stack on this GPU; the measured floor is roughly 64 MB private /
  81 MB working set with a card open, and idle CPU is still ~0.
