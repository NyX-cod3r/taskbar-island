# TaskbarIsland v0.8.0

TaskbarIsland is a small, entirely offline companion that lives in the Windows
taskbar. This release adds the Clipboard activity, a full Convert card with
four new local engines, honest Phone Link call handling, and the v0.8 design
system (shared card geometry, padding 12 → 7 DIP, icon-first UI).

Everything the app does runs on your PC. The app has no network access, no
telemetry and no auto-update. Setup itself checks the published release
manifest when it reaches the Ready page and, if a newer version exists, asks
before downloading it; `/NOUPDATE` skips that check entirely.

## Downloads

| File | What it is |
| --- | --- |
| `TaskbarIsland-Setup.exe` | Per-user installer (no UAC). Installs the app and the engine components you tick. |
| `taskbar-island-0.8.0.exe` | Standalone executable, for a portable/manual copy. |
| `manifest.txt` | Version and SHA-256 manifest for the release payload. |

SHA-256:

```
092fb592f88cd56dcc35a2d7e544796919b880849394dd1ea9509b0cacd380ba  TaskbarIsland-Setup.exe
202045f52850672a225bc060317c23c42b8efa084fd41c9b2a4ff11d3246bef2  taskbar-island-0.8.0.exe
```

The installer is **unsigned**, so Windows SmartScreen may show
"Windows protected your PC" on first run. Choose **More info → Run anyway**;
the hashes above are the way to verify the download. Do not run a build whose
hash does not match.

## What's new

### Clipboard history (new activity)

Copy text, links, code, colours, images or file lists as usual. The island
captures them into an encrypted, local-only history (Windows DPAPI), with a
transient `+N` badge on the pill for the batch that just landed. The card
shows typed rows, image thumbnails and a 450 ms hover preview. Entries expire
after **7 days**, the history is capped at **30 items**, and everything can be
cleared from the card.

### Convert card (new)

A data-driven Convert card for PDF, Word, spreadsheet, presentation, image,
audio/video, text and archive sources, with *Convert to*, *Optimize*, *Edit*,
*Extract text*, *PDF tools*, *Extract* and *Combine*. **LibreOffice, Poppler,
qpdf and Ghostscript** join FFmpeg, Pandoc and Tesseract. Jobs run locally in
a per-job workspace, report real progress, can be cancelled, and never delete
or overwrite your original (collisions become `stem (2).ext`).

### Call activity, honestly connected to Phone Link

The Call card now derives its state offline from Phone Link's own per-user
data: *not installed*, *not signed in*, *phone not connected* or *available*.
When the phone cannot be used, the card is one muted icon whose tooltip names
the missing condition — no fake controls. A live call keeps its ticking
`mm:ss` clock, real microphone/speaker mute, and priority on the pill. The
idle **Open Phone Link** button launches the real app.

### Design system and pill behaviour

The v0.8 tokens are checked in and validated at test time. Every card shares
one content column: 400 × 192 DIP, centered over the pill with a 6 DIP gap,
7 DIP padding, 57 DIP rail. The compact pill and card leading tiles use the
four activity identity icons, rendered natively by Direct2D; album art still
appears when a session provides it. Tooltips appear after ~450 ms, flip away
from the screen edge, and cover the new controls. The open card owns the
wheel: lists scroll, everything else is ignored.

## Install, upgrade, uninstall

- **Fresh install:** run `TaskbarIsland-Setup.exe`, choose the engines you
  want (LibreOffice is unticked by default because of its size), and optionally
  *Start when I sign in*. The app installs per user under
  `%LOCALAPPDATA%\Programs\TaskbarIsland` — no administrator rights.
- **Upgrade:** running the new installer over an existing install keeps your
  settings, shelf and engine selection.
- **Uninstall:** Settings → Apps → TaskbarIsland. The app, shortcuts and
  autostart entry are removed; **your user data in `%APPDATA%\TaskbarIsland` is
  kept** (shelf and clipboard history) so a reinstall finds it again. Delete
  that folder yourself if you want a clean slate.
- **Engines:** each engine is an optional installer component. An engine whose
  payload is missing is not offered and not packed; the app hides conversions
  that need it. The tray menu's engine maintenance flow can add or remove
  engines later.

## Known limitations

- **PDF → DOCX and other Office-family conversions are approximate.**
  LibreOffice re-lays out the document; complex layouts can differ. The
  affected options say `via LibreOffice · layout may differ`.
- **Call control depends on Phone Link.** The v0.8 spike returned NO-GO for
  call detection, banner suppression and call actions without a linked phone
  to validate against; audio is PARTIAL. Release builds ship no call
  simulator and no fake answer/decline/end controls. A real incoming-call run
  was not possible on the release machine and is recorded as **not run** in
  the test report.
- **Engine conversions that need LibreOffice, Poppler, qpdf, Ghostscript or
  `tessdata` are skipped** when those payloads are not installed. The gate run
  for this release skipped 22 golden rows for that reason.
- **DPI changes while the app is running** are not applied until restart;
  Windows sends no `WM_DPICHANGED` to this window. The app is tested at 100 %,
  125 % and 150 % with the taskbar at the bottom, including auto-hide.
- **RAM target:** the mandated DirectComposition + Direct2D stack keeps the
  process well above the old 15 MB target on this GPU (~64 MB private / ~81 MB
  working set with a card open). Idle CPU remains ~0.
- **Ghostscript is AGPL-3.0.** It is an optional component, installed only if
  you tick it, and its source is available from the Ghostscript project. See
  [`THIRD_PARTY_NOTICES.md`](https://github.com/NyX-cod3r/taskbar-island/blob/v0.8.0/THIRD_PARTY_NOTICES.md)
  for every bundled engine, its version, license and source, including the
  FFmpeg GPL-3.0 build and per-language tessdata licensing. Installing
  TaskbarIsland does not change any Ghostscript license obligation you
  already had; if you redistribute an installer with Ghostscript included,
  the AGPL-3.0 terms apply to that component.

## Verification

Full evidence, measurements and screenshots:
[`docs/release/v0.8.0/test-report.md`](https://github.com/NyX-cod3r/taskbar-island/blob/v0.8.0/docs/release/v0.8.0/test-report.md).

The release gate ran `cargo fmt --check`, `cargo clippy --all-targets --
-D warnings`, `cargo test` (272 passed, 0 failed, 1 ignored), the offline
source audit and the dependency denylist. Manual smoke passed at 100 %, 125 %
and 150 % scaling, and the 30-minute soak stayed flat for memory and handles.
