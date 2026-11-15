# Changelog

All notable changes to TaskbarIsland are recorded here. The format follows
Keep a Changelog; versions follow the owner's x.y.z scheme (the app stays on
0.x until the owner approves 1.0.0).

## 0.10.0 (2026-11-16)

### Changed

- **Slim activity rail (design section 1).** The side rail narrows to 48
  design px (38.4 DIP): five 28.8 x 28.8 r6 buttons with 14.4 px glyphs,
  6.4 DIP gaps, a 2.4 x 14.4 accent bar and a 1 px white 8 % divider. The
  content column widens to 346.6 DIP - shelf tiles become 78.65, convert
  options 62.92 and the music chip 122.6 so it still fills exactly what the
  transport row leaves. Tokens and `design-tokens.json` follow the rail.
- **Call card (design section 2).** Three equal rows (header / connected box
  / action row) fill the fixed 178 DIP content column with 48.67 DIP rows and
  16 DIP gaps: the header keeps the caller over its sub line and the
  right-aligned state word, the connected box shows the device name over its
  meta line with signal bars (14.4 x 12.8) and an in-box green Output chip
  (105.6 x 35.2), and the action row is three equal Hold / End call / Mute
  buttons. Incoming calls get an equal Decline / Answer pair, and the idle
  card offers "Open Phone Link". The Output chip opens the same output sheet
  0.9.0 shipped ("Call audio", 44 px rows), sliding up inside the fixed
  flyout - it never resizes.
- **Honest Hold.** Hold/Resume drives Phone Link's own UIA button: the
  control stays disabled with "Hold isn't available for this call" until a
  400 ms probe has seen Phone Link's own Hold button *for this call*, "On
  hold" appears only once Phone Link confirms a Resume button exists, and the
  state is dropped the moment the call ends. A held state is amber.

### Added

- New glyphs for the call card: PhoneEnd (hang-up handset), Headphones,
  ChevronUp and PhoneDevice, baked into `src/icons.rs` from the design SVG
  paths.

## 0.9.0 (2026-11-15)

### Added

- **Output picker (spec 4).** A chip with the device icon, the real default
  device name (ellipsized) and a chevron sits at the end of the music
  controls row (105 x 36) and between mute and End on the call card
  (150 x 44). It opens an output sheet that slides up *inside* the same
  flyout (240 ms, `EASE_CONTENT`, full flyout size, `rgb(38,38,38)`, 12/16
  padding, 44 px rows with a 28 x 28 tile, name, sub line and a check on the
  current device) - the flyout never resizes. The music list is every active
  render endpoint plus "Cast to a device..." (sub: "Phone, tablet or TV"); the
  call list is the Bluetooth hands-free endpoint, the PC speakers and
  microphone, and Phone. Choosing a device sets the default for all three
  roles through `IPolicyConfig::SetDefaultEndpoint` (failures fall back to
  `ms-settings:sound`), the call row also sets the communications default,
  and Phone hands the call over through a UIA invoke
  (`Phone` / `Transfer to phone` / `Transfer call`) with an honest
  "Open Phone Link to transfer" fallback. An `IMMNotificationClient` watcher
  refreshes the list when devices appear or the default changes - no polling.
- **Music two-line text (spec 5).** Line 1 is the title (16 px, semibold,
  ellipsis); line 2 is the artist at 78 % white, a 12 px gap and the source
  app at 52 %, resolved from the session's `SourceAppUserModelId` (browsers
  report their page, e.g. YouTube Music, Spotify, Chrome). Art stays 72 x 72
  at radius 8; the freed space holds the wavy progress bar, the controls row
  and the output chip.
- **Timer stepper (spec 6).** The 5/10/15 presets are gone. The set view is
  `-` (56 x 56) / big MM:SS time / `+`, range 1..180 minutes, default 15,
  hold-to-repeat (first repeat 420 ms, then every 110 ms, released on mouse
  up, leave or capture loss) and a full-width amber Start button. The running
  view is ring tile (56 x 56), name and status, time left at the right, the
  wavy bar with elapsed and total, then pause (48 x 40), -1 min, +1 min and
  stop (40 x 40). One shell height serves both views, so the flyout never
  resizes when the countdown starts; the pill shows the time left and pulses
  amber when it ends.
- **Flyout motion (spec 7).** The card morphs out of the pill rect over
  460 ms on `cubic-bezier(.2,1.1,.3,1)` and leaves over 260 ms on
  `cubic-bezier(.4,0,.2,1)`; content holds for 160 ms and then fades in over
  280 ms. The card is laid out at its final size throughout - only the
  surface's scale and translate move - so nothing inside re-flows mid-open,
  and the window region plus hit test follow the painted rect, so the
  un-painted area around a half-open card never swallows a click. Closing on
  outside click, Esc or 700 ms after the pointer leaves is unchanged.
- **Wave and progress per spec 7.** Wavelength 20, amplitude 3, stroke 3 with
  round caps, one wavelength every 700 ms (was 1.2 s), track 3 px at white
  22 %, thumb 4 x 18 white. The music and timer wave rows grew to 20 DIP so
  the taller thumb sits inside its own row.
- **Split installers, single-source identity, CI draft release.** The core
  setup (`TaskbarIsland-Setup.exe`: our executable plus `LICENSE`/`NOTICE`,
  nothing else) is separate from the optional **Converters pack**
  (`TaskbarIsland-Converters-Setup.exe`, third-party engines only), so the
  artifact that gets code-signed contains only our own binaries; the tray's
  "Manage engines..." now prefers the pack and still falls back to the
  pre-0.9 setup copy. `version` and `[package.metadata] product-name` in
  `Cargo.toml` feed the exe's VERSIONINFO (`build.rs` renders
  `assets/app.rc`) and Inno Setup (`/DMyAppVersion`, `/DMyAppName`), with
  `version_tests` failing the suite if a literal copy reappears. Bumped to
  0.9.0.
- **`.github/workflows/release.yml`.** On a `v<x.y.z>` tag a clean runner
  builds the core installer from source, runs the tests, **fails when the tag
  disagrees with `Cargo.toml`**, hashes the installer and uploads the setup,
  its SHA-256, the update payload and `manifest.txt` to a **draft** release -
  never publishing and never using a stored secret.
- **Release and signing docs.** `README.md` (with the *Code signing policy*:
  author / reviewer / approver roles, the no-network privacy statement,
  SmartScreen explanation and a SignPath Foundation line kept as a pending
  placeholder), `docs/signing.md` (every SignPath step marked
  `[AFTER APPROVAL]`) and `docs/release-process.md`
  (bump -> test -> merge -> tag -> CI draft -> sign -> owner publishes).
- **License audit.** The core installer ships no third-party code and every
  Rust dependency is OSI-approved; the only item whose terms are not
  verifiable from the file alone (per-language Tesseract `tessdata`) stays
  pack-only. Recorded in `THIRD_PARTY_LICENSES.md`.
- **GCC 16 / binutils manifest conflict workaround** (sourceware 34362):
  `tools\build.ps1` moves the toolchain's `default-manifest.o` aside and
  gates every release build on the exe carrying exactly one `<assembly>` root
  including our `<msix>` element (sparse-package identity, spec 3 Source B).
- **Soak harness** (`tools\soak.ps1`): samples working set, private bytes,
  handles, threads, per-interval CPU and responsiveness of the installed
  process into `target\soak\*.csv`.

### Changed

- **Call sources (spec 3).** One call-source state machine behind four honest
  sources, a `--diag-call` log (`%LOCALAPPDATA%\TaskbarIsland\diag\`) that
  masks phone numbers, and Answer/Decline/End driven through UIA or an honest
  fallback message instead of a demo state.
- **Clipboard file images (spec 2).** `CF_HDROP` payloads are classified by
  content, so a copied image file lands in *Images* with a thumbnail and the
  `FILE` badge instead of being filed as plain text.
- Tooltips flip to the left of their control when the tip would run past the
  right edge of the screen, so rail and right-hand card controls keep their
  copy on screen.

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
