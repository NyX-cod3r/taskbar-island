# TaskbarIsland v0.8.0 — test report

Date: 2026-11-11
Branch under test: `agent-b/v0.8.0` at `3b8e3cd` (installer-selection fix), plus this docs
commit; the final release tag is `v0.8.0`. The 44/44 installer result is from those bytes.
Builds: `target\release\taskbar-island.exe` for the manual smoke; `target\debug\taskbar-island.exe`
for the 30-minute soak and the debug-only call simulator.

The 125 %, 100 % and 150 % smoke runs used a release build that carried temporary
tooltip log lines. Those lines were removed afterwards, `src\window.rs` was verified
byte-identical to `HEAD`, the gates below were re-run, and the shipped installer is built
from that clean tree. No behavioural code differed between the smoke runs and the release.

## Verdict

**GO — release the tested scope.** Every gate that could be executed passed: the three Rust
gates (272 passed / 0 failed / 1 ignored), the engine golden suite (13 passed / 22 skipped),
the full smoke at 100 %, 125 % and 150 % (including auto-hide), and the installer harness
(44/44). Two items were **not run** and are not claimed: a real incoming Phone Link call (no
linked phone; the A2 spike returned NO-GO) and a clean-machine Windows Sandbox install
(Sandbox is not available on this PC). The 30-minute soak was flat for 29 of 30 samples; the
final sample's one-off handle/memory spike is documented in §10 and could not be reproduced.
The Phone Link items are **NOT RUN / NO-GO** as explained in §7 and are not counted as passes.

## 1. Machine

| Item | Value |
| --- | --- |
| OS | Windows 11 Home Single Language 10.0.26300 (build 26300) |
| CPU | 13th Gen Intel Core i5-13500H |
| GPU | Intel Iris Xe Graphics, driver 32.0.101.5768 |
| RAM | 15.7 GB |
| Display | 1920 × 1200 physical, taskbar at the bottom |
| Scales tested | 100 % (96 DPI), 125 % (120 DPI), 150 % (144 DPI) |
| Toolchain | rustc / cargo 1.99.1, Inno Setup 6.7, PowerShell 7.6.9 |

## 2. Gates

Run at `0ff10b2` with the WinLibs `windres` toolchain on `PATH`:

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | **PASS** (no output) |
| `cargo clippy --all-targets -- -D warnings` | **PASS** (finished clean) |
| `cargo test` | **PASS — 272 passed, 0 failed, 1 ignored** (12.04 s) |
| `version_consistency` | PASS — `Cargo.toml`, installer `AppVersion` and resource `FileVersion` all `0.8.0` |
| `cargo_lock_has_no_network_crates` (dependency denylist) | PASS |
| `windows_features_do_not_include_networking` | PASS |
| `sources_do_not_open_sockets_or_http` (offline source audit) | PASS |
| `conversion_path_never_deletes_files` | PASS |

The one ignored test is the pre-existing manual-only engine probe, unchanged by v0.8.0.

## 3. Engine golden suite (Agent B's tests, run once with `TI_ENGINES_DIR`)

Command:

```
$env:TI_ENGINES_DIR = 'C:\Users\satya\AppData\Local\Filee\engines'
cargo test convert::golden -- --nocapture
```

Result: **exit 0 — 13 passed, 0 failed, 22 SKIP lines** (260 filtered out, 2.53 s).

The 22 skips are exactly the rows whose engine payload is absent from this machine's
`ENGINES_DIR` (only `ffmpeg`, `pandoc` and `tesseract` are present; `tessdata\eng.traineddata`
is missing too). They are printed by the suite, not hidden:

| Missing payload | Skipped rows |
| --- | --- |
| LibreOffice | md→pdf, docx→pdf, pdf→docx, pdf→odt, pptx→pdf, pptx→odp, pptx→png, csv→pdf, csv→xlsx (9) |
| Poppler | pdf→txt, pdf→md, pdf→png pages, pdf text, pdf embedded images, pdf page→png, pdf split, pdf merge, merge pdfs (9) |
| qpdf | pdf rotate left, pdf web-optimize (2) |
| Ghostscript | pdf compress (1) |
| Tesseract language data | pdf make searchable (1) |

The 13 rows that can run with the present payloads all passed. The app hides or disables
the options that need a missing engine and never invents success.

## 4. Manual smoke — 125 % (primary desktop configuration)

Release build, scale 125 % (DPI 120), taskbar bottom, dark theme. The scripted smoke
reported `SMOKE-PASS all checks`.

| Check | Measured evidence |
| --- | --- |
| Pill inside the taskbar | **280 × 50 px** = 224 × 40 DIP, at `(1224,1145)-(1504,1195)` |
| Card size | **500 × 240 px** = 400 × 192 DIP |
| Card position | `(1114,892)-(1614,1132)`, centered over the pill, **delta 0** |
| Card gap | **8 px** = 6 DIP above the taskbar |
| Nothing clipped | every UIA node inside the measured card; `clip outside=0` |
| Rail tiles | all five switch the card (Music, Call, Shelf, Timer, Clipboard) |
| Wheel over pill | exact cycle `None → Clipboard → Timer → Shelf → Call → Music → None` (five activities) |
| Tooltip | visible after **460 ms** (`TaskbarIslandTooltip`, UIA anchor checked) |
| Shelf | 10 items, wheel scroll, toolbar visible; Convert opens, Close exits, reopen works; Select all → Remove selected → `shelf.json == []` |
| Clipboard | live capture, `+N` pop, card with the captured rows |
| Timer | Pause / Resume / Stop all act; ended/cancelled states correct |
| Idle CPU | **16 ms / 10 s** (≈0 % of a core) with the card closed |
| Memory / handles at the end | WS **81 MB**, private **64 MB**, **450 handles**, 15 threads (release QA image with logging) |

## 5. DPI matrix

The same scripted smoke was run at each scale with the app restarted at that scale.
Geometry is reported in physical pixels; the DIP target is identical at every scale.

| Scale | Taskbar rect | Island rect | Pill | Card rect | Card size | Gap | Center delta | Tooltip |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 100 % | `(0,1152)-(1920,1200)` | `(1214,914)-(1734,1200)` | 224 × 40 px | `(1274,954)-(1674,1146)` | **400 × 192** | **6 px** (6 DIP) | 0.5 px | 516 ms |
| 125 % | `(0,1140)-(1920,1200)` | `(1039,843)-(1689,1200)` | 280 × 50 px | `(1114,892)-(1614,1132)` | **500 × 240** (400 × 192 DIP) | **8 px** (6.4 DIP) | 0 px | 460 ms |
| 150 % | `(0,1128)-(1920,1200)` | `(861,771)-(1641,1200)` | 336 × 60 px | `(951,831)-(1551,1119)` | **600 × 288** (400 × 192 DIP) | **9 px** (6 DIP) | 0.5 px | 479 ms |

At every scale: taskbar at the bottom edge, every rail tile switched, the wheel over the
pill reached all five activities, every UIA node stayed inside the card, and the card was
on-screen above the taskbar. The 1 px difference in the gap is the unavoidable rounding of
6 DIP × 1.25 = 7.5 px; the measured window edges are pixel-snapped.

Screenshots: `p100-pill.png`, `p100-card.png`, `p100-wheel-*.png`, `p100-tooltip.png`,
`p150-*` in `screens/`. The 125 % shots are `01`–`21`.

## 6. Themes

| Check | Result |
| --- | --- |
| Windows dark theme | Pill and card dark material, text legible (`01`–`21`) |
| Windows light theme | Pill keeps the Windows dark taskbar material; the card switches to the light surface (`22-pill-light.png`, `23-card-light.png`) |
| Restore | `SystemUsesLightTheme` restored to `0` (dark) and the card re-checked |

The theme follows `SystemUsesLightTheme` (taskbar theme), deliberately not
`AppsUseLightTheme`, so the card matches the surface it sits on.

## 7. Phone Link and calls

The Phase A2 spike is **NO-GO** for call detection, banner suppression and call actions
without a linked real phone; audio is **PARTIAL**. This machine has no linked phone, so the
real-call checklist (ringing, answer, switch output, end; decline; missed) was **NOT RUN**.
It is not claimed as a pass.

What was verified instead:

- Offline provider states: `NotInstalled`, `NotSignedIn`, `NotConnected`, `Available`,
  derived only from the per-user Phone Link package and `DeviceMetadataStorage.json`
  `"IsLinked"`. "Connected" is never asserted from weak evidence.
- Idle card: one muted icon whose tooltip names the missing condition; **Open Phone Link**
  launches the real app. No fake signal bars or dead controls.
- Live call (debug-only simulator): pill shows **"Simulated call"** with a ticking `mm:ss`
  clock and priority over every other activity; the card shows the active-call state,
  microphone/speaker mute buttons and the source. `24-pill-call-simulated.png`,
  `25-card-call-simulated.png`.
- The simulator is compiled out of release builds (`#[cfg(debug_assertions)]`,
  `TBI_SIMULATE_CALLS`) and no fake call control ships in the release binary.
- Caller data is never logged or stored. The existing number-masking tests pass.

## 8. Clipboard

| Check | Evidence |
| --- | --- |
| Live capture | Text copied while the app ran appeared in the card with the `+N` pop |
| Display types | link / code / colour / image / files / text re-derived per frame |
| 7-day expiry (injected clock) | `retention_boundaries`: item at 7 days − 1 s survives, 7 days + 1 s is removed; `clock_backwards_never_expires_early`: a rolled-back clock never expires early and the high-water stamp still gives a new copy its full 7 days |
| 30-entry limit | `thirty_first_evicts_oldest_unpinned`: the 31st copy evicts exactly the oldest unpinned row and keeps pinned rows; `all_pinned_store_refuses_new_copies`; `pin_is_refused_at_twenty_nine` |
| Encryption at rest | Store on disk is DPAPI-encrypted (2,358-byte `store.dat`; no plaintext in the file) |
| Pinning / clear | `pinned_rows_outlive_retention`, `clear_keeps_pinned_rows`, `duplicates_move_to_top_and_reset_age` |

## 9. Auto-hide taskbar (125 %)

Run with the bar set to auto-hide. The app was started while the bar was revealed, because
`Anchor::visible` is false when the taskbar is fully off-screen and the app deliberately
exits rather than drawing a detached pill. It was then left running while the bar retracted.

| Check | Measured evidence |
| --- | --- |
| Hidden bar | `Shell_TrayWnd` `(0,1198)-(1920,1258)` (2 px lip); island unchanged at `(999,843)-(1649,1200)` |
| Hidden pill | window region `(1184,1145)-(1464,1195)` = 280 × 50 px, fully on-screen; no clip |
| Revealed bar | tray `(0,1140)-(1920,1200)`; island and region **identical** to the hidden state (no jump, no re-anchor) |
| Rail tiles | all five switch the card with the bar hidden: Music, Timer, Call, Shelf, Clipboard |
| Wheel | exact `Call → Shelf → Timer → Clipboard → None → Music` cycle, **0 failures** |
| Card | expected `(1074,892)-(1574,1132)`; border scan **500 × 239 px = 400 × 191 DIP** (1 px is border antialiasing; width exactly 500 px), in the window region |
| Gap / centering | 8 px = 6.4 DIP above the revealed taskbar; center delta 0 px |
| Clipping | every UIA node inside the card; **outside = 0** |
| Tooltip | `TaskbarIslandTooltip` visible after **473 ms** (class enumerated; `FindWindowW` cannot see this layered owned popup) |
| Restore | auto-hide switched off; tray back to `(0,1140)-(1920,1200)` |

Screenshots: `26-autohide-pill-hidden.png`, `27-autohide-pill-revealed.png`,
`28-autohide-card.png` (Music card for a clean border), `28b-autohide-tooltip.png`.

## 10. 30-minute soak

Debug build + `TBI_SIMULATE_CALLS=1` + `TBI_LOG=1`, one sample per minute and one clipboard
copy per minute; the simulator cycles 12 s on / 8 s off. Run 14:45:52 → 15:16:10 (30 rows).

| Metric | Minutes 1–29 | Minute 30 |
| --- | --- | --- |
| Working set | 78.7 → 70.1 MB after the first call card closed, then 70.1 → 70.7 MB (±0.3) | 82.8 MB |
| Private | 57.1–57.6 MB, flat | 59.8 MB |
| Handles | 441–451, 447 baseline | 594 |
| Threads | 13–19, 15 baseline | 20 |
| CPU per sample | 31–312 ms | 1,219 ms |
| Responding | True in every row | True |

29 of 30 samples are flat. The minute-30 sample (+147 handles, +2.2 MB working set,
+2.5 MB private, +5 threads, ≈1.2 s CPU) was a one-off at the end of the run. A targeted
90 s probe with four clipboard captures did **not** reproduce it: handles went
392 → 398 (+5–6 transient) → 389, working set +0.6 MB, private +0.2 MB, 31 rows, process
responsive. The spike is therefore reported as observed and not explained by capture load;
it is not claimed to be leak-free. The soak log file was not retained (the harness temp file
was gone afterwards), so "no panics" rests on every sample reporting `Responding=True`,
not on log lines. CSV: `soak-0.8.0.csv` in the session temp folder.

## 11. Installer

Built once from `release/v0.8.0` at `3b8e3cd` (the installer-selection fix below) and
published from those exact bytes:

| Item | Value |
| --- | --- |
| Installer | `dist\TaskbarIsland-Setup.exe`, **136,260,210 bytes** |
| Installer SHA-256 | `092fb592f88cd56dcc35a2d7e544796919b880849394dd1ea9509b0cacd380ba` |
| Payload | `dist\taskbar-island-0.8.0.exe`, **3,695,104 bytes** |
| Payload SHA-256 | `202045f52850672a225bc060317c23c42b8efa084fd41c9b2a4ff11d3246bef2` |
| Harness result | **44 checked, 0 failed — `INSTALLER-TEST-PASS`** |

The harness ran the silent install, launch, uninstall and reinstall loop with
`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /NOUPDATE` and the real release exe.

- **Upgrade path:** this PC had the earlier 0.8.0 build installed (no 0.7.x installer
  exists), so the upgrade was tested in place over that build, not over a different
  version. `DisplayVersion = 0.8.0`, `FileVersion = 0.8.0`, `ProductVersion = 0.8.0`.
  There is no About dialog in the app; the version surfaces are the file/uninstall
  versions and the installer's `AppVerName`.
- **Custom components:** `/COMPONENTS=core,ffmpeg` installed only `engines\ffmpeg`;
  `pandoc` and `tesseract` were removed and the engine registry recorded
  `ffmpeg=1 pandoc=0 tesseract=0`. The first harness run exposed a real bug here: on an
  upgrade the remembered engine selection overrode an explicit `/COMPONENTS` list.
  `3b8e3cd` makes an explicit list win; the installer was rebuilt and the 44/44 result is
  from the rebuilt bytes.
- **Shortcuts / autostart:** Start-menu shortcut target is the installed exe; the Run key
  points at the installed exe. Both are removed by uninstall and restored by reinstall.
- **User data:** `shelf.json`, `settings.json` and the DPAPI clipboard `store.dat` were
  byte-identical before/after the custom install, the uninstall and the default reinstall.
  Uninstall removes the install directory, shortcut, Run value and engine registry, and
  keeps `%LOCALAPPDATA%\TaskbarIsland` and `%APPDATA%\TaskbarIsland` deliberately.
- **Reinstall:** the default selection restores `ffmpeg,pandoc,tesseract`, the shortcut,
  autostart and the engine registry. Launch shows the pill + hidden tray windows, a second
  launch keeps a single process, and the run had no panic lines.
- **Clean machine:** **not run.** `WindowsSandbox.exe` is absent, so no Windows Sandbox
  install was possible. This is stated here rather than implied.

The installer remains unsigned, so SmartScreen may warn on first run. Neither the app nor
the installed copy makes network calls; only Setup's opt-in self-update check contacts
GitHub, and `/NOUPDATE` skips it (used by this harness).

## 12. Screenshots

`screens/` contains **40** captures: the pill/card states, Shelf scroll/Convert, timer
running/paused, the clipboard pop/card, all six wheel stops, tooltips, light theme, the
debug-only call simulator (`24`, `25`), the auto-hide set (`26`, `27`, `28`, `28b`) and the
100 %/150 % DPI captures. `sheet-*.png` contact sheets were review-only and are removed;
the individual captures remain.

## 13. What is not claimed

- A real incoming Phone Link call — not run (no linked phone; spike NO-GO).
- Conversions whose engines are absent — 22 golden rows skipped, listed in §3.
- A clean-machine install in Windows Sandbox — status recorded in §11.
- Live DPI change without restart — not supported by the app (no `WM_DPICHANGED`), verified
  by restarting at each scale.
- The <15 MB RAM target — not met with the mandated rendering stack; measured floor is
  reported honestly in the release notes.
