# Testing

What is tested in this repository, what is skipped, and why. The current gate
evidence was produced on branch `v0.10` (tag `v0.10.0`) on 2026-11-16 (the v0.9 gates ran
on 2026-11-15); the quoted v0.8.0 logs were rescued from the scratch logs at
the workspace root before those were deleted by the v0.9 cleanup
(`docs/cleanup-plan.md`).

## Rust gates (current, v0.10)

| Gate | Command | Result (2026-11-16) |
| --- | --- | --- |
| Unit/integration tests | `cargo test` | **326 passed, 0 failed, 1 ignored** |
| Clippy | `cargo clippy --all-targets -- -D warnings` | Clean, no diagnostics |
| Format | `cargo fmt --all` (then `--check`) | Clean, no diffs |
| Release build | `tools\build.ps1 -Mode Installer` | Green, manifest gate passed (`exactly one <assembly> root including <msix>`), `dist\TaskbarIsland-Setup.exe` for 0.10.0 |
| Installer gate | `powershell -File tools\tests\install-smoke.ps1` | **8 checked, 0 failed** (stop → install → exe/version → byte-match against `target\release` → launch → pill/tray windows → single-instance) |
| CI tag run | `git push origin refs/tags/v0.10.0` → workflow `release` | Run **43755768572** `success` on tag `v0.10.0` = `f9d6b39`: tag↔`Cargo.toml` check, tests, installer, hash, draft release with 4 assets (published after verification) |
| Ignored test | `convert::jobs::live_external_engines_convert_real_files` | Needs live engines in `%LOCALAPPDATA%\Filee\engines`; run manually when engines are installed |

Toolchain note: builds must use the WinLibs gcc — a broken `C:\MinGW\bin\gcc.exe`
shadows it. `tools\build.ps1` and `tools\cargo-winlibs.cmd` prepend the WinLibs
`mingw64\bin` to `PATH` automatically.

`cargo test` alone is not evidence that a change works: the pill, the tray, the
clipboard listener and the installer only exist in the installed copy, which is
why `install-smoke.ps1` is the release gate.

## Engine golden suite (skip table)

```powershell
$env:TI_ENGINES_DIR = "$env:LOCALAPPDATA\Filee\engines"
cargo test convert::golden -- --nocapture
```

**13 passed, 0 failed, 22 SKIP lines** (re-run on branch `v0.9`, 2026-11-15 —
identical to the v0.8.0 run). The skips are exactly the rows whose engine
payload is absent from this machine; only `ffmpeg`, `pandoc` and `tesseract`
are present, and `tessdata\eng.traineddata` is missing too. They are printed
by the suite, never silent:

| Missing payload | Skipped rows |
| --- | --- |
| LibreOffice | md→pdf, docx→pdf, pdf→docx, pdf→odt, pptx→pdf, pptx→odp, pptx→png, csv→pdf, csv→xlsx (9) |
| Poppler | pdf→txt, pdf→md, pdf→png pages, pdf text, pdf embedded images, pdf page→png, pdf split, pdf merge, merge pdfs (9) |
| qpdf | pdf rotate left, pdf web-optimize (2) |
| Ghostscript | pdf compress (1) |
| Tesseract language data | pdf make searchable (1) |

The 13 rows that can run with the present payloads all pass. The app hides or
disables the options that need a missing engine and never invents success.

## Soak (v0.9, 60 minutes)

`powershell -File tools\soak.ps1 -Minutes 60` samples the *installed* release
process once a minute — working set, private bytes, handles, threads, CPU
seconds consumed in the interval and `Responding` — and writes
`target\soak\soak-0.9.0-<stamp>.csv`. The app is not touched during the run:
this measures the resting, event-driven cost, so a leak would show as a trend
in handles or private bytes.

Run of 2026-11-15, `target\soak\soak-0.9.0-20261115-203008.csv`, installed
release, one process the whole time (pid 9032, no restart):

| Measure | First sample (20:31:08) | Last sample (21:30:09) | Min / max over the run |
| --- | --- | --- | --- |
| Working set | 70.96 MB | 71.05 MB | 69.75 / 71.56 MB |
| Private bytes | 54.97 MB | 54.91 MB | 54.74 / 55.11 MB |
| Handles | 510 | 503 | 497 / 520 |
| Threads | 20 | 18 | — |
| `Responding` | True | **60 of 60 samples True** (0 unresponsive) | — |

- **CPU: 14.937 s consumed over the 60 minutes** = 0.249 s/sample =
  **0.41 % of one core** — the resting, event-driven cost (no timers running,
  nothing live).
- **No growth trend.** Private bytes end 0.06 MB *below* where they started
  and never leave the 54.74–55.11 MB band; handles end 7 lower after peaking
  at 520 mid-run; working set oscillates in a 1.8 MB band (first three samples
  average 70.93 MB, last three 71.31 MB — inside the band, with the run's
  maximum at sample 6). A leak would show as a monotonic climb in private
  bytes or handles; neither appears.

## Card UI screenshots (v0.9)

`powershell -File tools\tests\ui-smoke.ps1` photographs the *installed* card
UI into `target\soak\ui\`: 13 PNGs (six activities in dark and light, plus one
mid-morph frame) and `pins.log`, the app's own `activity.show:` trail proving
which pin produced which shot.

How it drives the app (the script documents this in its header): the wheel only
cycles while the card is closed, so each state is *close → notch → click*; the
notch is a `WM_MOUSEWHEEL` posted straight to the island HWND rather than a
synthesized mouse event, because the message normally goes to the **focus**
window and a mouse-event run silently skipped notches (two shots came out
byte-identical). Every notch is read back from the app's log, so each file is
named after the activity actually on screen — a run with a wrong label is
impossible to miss, and `pins.log` shows one `activity.show` line per posted
notch (66 lines for 61 notches + 5 restore notches).

| File | Shows |
| --- | --- |
| `00-morph-midopen.png` | Card at t≈150 ms of the 460 ms open: laid out at final size but **content still transparent** (160 ms hold before the 280 ms fade) — spec 7's content delay, with the desktop bleeding through |
| `01-auto-dark/light.png` | Auto with nothing else live resolves to the **shelf** (it holds 1 file), so it is pixel-identical to the shelf shot in light |
| `02-music-*` | Music empty state, two-line text, and the §4 output chip (`Speak…`) at the end of the controls row |
| `03-call-*` | Call empty state (`No active call` / `Open Phone Link`) |
| `04-shelf-*` | Shelf with its one file tile and the `1` count |
| `05-timer-*` | §6 stepper: `− 15:00 +`, "15 minutes · hold to repeat", full-width amber Start |
| `06-clipboard-*` | Clipboard list, 30/30, filter tabs, pause and clear |

Both themes are the real thing: the card follows the app theme (light cards in
the `-light` shots) while the pill stays dark because it samples the *taskbar*
tone — the taskbar is dark on this machine. The owner's theme is restored in a
`finally` block, so an aborted run cannot leave the machine in light mode.

## What is exercised manually (v0.9)

- Clipboard file images: a copied image file appears in **All** and **Images**
  with a thumbnail and the `FILE` badge (spec 2).
- Output picker: chip label follows the real default device; the sheet slides
  up inside the fixed flyout; picking a device changes the Windows default;
  the list refreshes when a device connects (spec 4).
- Music two-line text and the timer stepper, including hold-to-repeat (specs
  5 and 6).
- Flyout open/close motion and the wave (spec 7).

Captured rather than remembered: `tools\tests\ui-smoke.ps1` photographs all six
cards in dark and light plus a mid-morph frame into `target\soak\ui\` (see
*Card UI screenshots* above), and `tools\soak.ps1` records the 60-minute
numbers above. Those are machine-read screenshots, so the *human-eye* judgement
of the new motion and wave is still listed as not tested.

## Not tested (v0.9)

Stated honestly so nothing is claimed that was not run:

- **A real incoming, outgoing and ended Phone Link call.** The detector and
  the UIA actions exist and are unit-tested, but no phone is linked on this
  machine, so **call support is not claimed** until the owner runs one call
  and sends the `--diag-call` log (`%LOCALAPPDATA%\TaskbarIsland\diag\`).
- **Real audio-device churn** (plugging a headset in mid-open): the watcher is
  `IMMNotificationClient`-based and event-driven, but no device was attached
  or removed during the v0.9 run.
- **DPI matrix 100 / 125 / 150 %, light and dark, auto-hide taskbar.** Run for
  v0.8.0 (`docs/release/v0.8.0/test-report.md` §5, §6, §9) and **not re-run
  after the v0.9 UI changes** — changing the system scale programmatically is
  not safe to script on the owner's machine. Must be re-run by hand before
  1.0.0.
- **Clean-VM install** (Windows Sandbox absent on this machine).
- **CI's first run on a `v<x.y.z>` tag** was still pending when this section
  was written; it has since executed — see the *CI tag run* row above
  (`v0.9.0` run 43745736845 and `v0.10.0` run 43755768572, both `success`).
- **The SignPath flow** — written but never executed; every signing step in
  `docs/signing.md` is still `[AFTER APPROVAL]`.
- **Real Explorer drag-out of a shelf tile** (DoDragDrop cannot be scripted
  reliably) and **human-eye checks** of the new motion, sheet and wave.
- The one ignored test (`live_external_engines_convert_real_files`) and the
  22 golden rows above.

## Card UI screenshots (v0.10)

`powershell -File tools\tests\ui-smoke.ps1` re-run against the installed
0.10.0 release (2026-11-16): **13 PNGs + `pins.log`** (61 notches, final pin
`None`) in `target\soak\ui-v0.10\` - the slim rail with every card in dark and
light plus the mid-morph frame. Machine-read, so the *human-eye* judgement of
the rail and the new call card is still listed as not tested.

## What is exercised manually (v0.10)

- The idle call card (header, "No active call" box, mic/speaker squares, "Open
  Phone Link") and the music card's widened output chip (122.6 DIP) -
  photographed; both are visible in the shots above.
- The slim rail's five tiles, accent bar and 38.4 DIP width - photographed.

## Not tested (v0.10)

Stated honestly so nothing is claimed that was not run:

- **Everything under *Not tested (v0.9)* above still applies**: a real Phone
  Link call, real device churn, the DPI matrix, clean-VM install, SignPath,
  Explorer drag-out and human-eye checks. (CI's tag run is no longer in this
  list — it ran green, in the gates table above.)
- **Live-call card states**: the connected box with signal bars, the in-box
  Output chip, incoming Decline/Answer, three equal Hold / End call / Mute
  buttons and amber "On hold" - no call could be placed on this machine, so
  only the idle card was photographed. Hold/Resume is unit-tested (UIA press,
  400 ms probe, honest disabled state) but **not claimed on a live call**.
- **The 60-minute soak was not re-run for 0.10.0.** The v0.9 soak above
  covers the same event-driven core; the 0.10 delta is layout-only (slim
  rail, call card). Re-run before 1.0.0.
- The one ignored test (`live_external_engines_convert_real_files`) and the
  22 golden rows above.

## Release-gate evidence (v0.8.0, quoted from the deleted scratch logs)

From `rv8-*` (release worktree `D:\ti-release-v08`, 2026-11-08/09):

- `cargo fmt --check` → one diff found in `src/main.rs` (`installer_version`
  concat), fixed by `cargo fmt` (rv8-fmt-apply: clean).
- `cargo clippy --all-targets -- -D warnings` → `Finished dev profile in 4.03s`,
  no diagnostics (rv8-clippy-err.txt).
- `cargo build --release` → `Finished release profile in 39.73s`
  (rv8-release-err.txt).
- `cargo test` → **187 passed, 0 failed, 1 ignored** (rv8-test-out.txt), then
  **188 passed, 0 failed, 1 ignored** after the last fix (rv8-test2-out.txt).

From the main-tree logs (superseded by the current 325-test baseline):

- `build-err.txt` — v0.7.0 dev build with 139 warnings (pre-burn-down).
- `clippy-err.txt` — the pre-burn-down state: 182 clippy errors (debt later
  burned to zero in commit `393f52c`).
- `test-out.txt` / `final-test-out.txt` — 187 then **188 passed, 0 failed,
  1 ignored**.

These logs were deleted because every number in them is reproduced by running
the gates again; the quoted results above keep the historical record.
