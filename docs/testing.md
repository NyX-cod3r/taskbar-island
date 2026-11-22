# Testing

What is tested in this repository, what is skipped, and why. The current gate
evidence was produced on branch `v0.11.0` (tag pending) on 2026-11-19 (the v0.10
gates ran on 2026-11-16 and the v0.9 gates on 2026-11-15); the quoted v0.8.0
logs were rescued from the scratch logs at
the workspace root before those were deleted by the v0.9 cleanup
(`docs/cleanup-plan.md`).

## Rust gates (current, v0.11)

| Gate | Command | Result (2026-11-19) |
| --- | --- | --- |
| Unit/integration tests | `cargo test` | **338 passed, 0 failed, 1 ignored** |
| Clippy | `cargo clippy --all-targets -- -D warnings` | Clean, no diagnostics |
| Format | `cargo fmt --all` (then `--check`) | Clean, no diffs |
| Release build | `tools\build.ps1 -Mode Installer` | Green, manifest gate passed (`exactly one <assembly> root including <msix>`), `dist\TaskbarIsland-Setup.exe` for 0.11.0 |
| Installer gate | `powershell -File tools\tests\install-smoke.ps1` | **8 checked, 0 failed** (stop → install → exe/version → byte-match against `target\release` → launch → pill/tray windows → single-instance) |
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

## §7 embed spike (v0.11, Stage A feasibility)

`tools\spike-embed\` answered "can the pill be a `WS_CHILD` of `Shell_TrayWnd`
so the taskbar's XAML bridge can never paint over it?" **before** any app code
changed. Full numbers and the verdict are in `tools\spike-embed\RESULTS.md`;
the record here:

- **Runs**: `log-smoke.txt` / `log-freshshell.txt` / `log.txt` clean embeds
  (`z='HWND_TOP (above bridge first try)'`, 120 s stable, exit 0);
  `log-fullrun1.txt` scenarios 1–3 **18/18 PASS**, scenario 4 and 5 **FAIL**
  (`zok=0 green=False`, `failures=2`, `blink-count=6`, re-embed 544 ms
  external / 0 ms internal); `log-run3-sc123.txt` scenarios 1–3 **18/18
  PASS** (newer `hwnd-top` z-heal: 1 heal, `after2=[]`, `zok=1` held to end
  of log), log ends mid-scenario-4 so sc4-off / 5 / 6 never re-ran there.
- **Root cause of the run-1 failures**: `hWndInsertAfter=bridge` lands the
  child *below* the bridge (it names the window that ends up above), so the
  `insert-after-bridge` heal fired 334x/82 s and never stuck. `HWND_TOP`
  heals in one shot and stays — that is the method Stage A ships.
- **Re-embed**: parent death takes the child with it on every Explorer
  restart; re-create on `TaskbarCreated` measured 0 ms internal latency
  (544 ms external is Explorer's own restart time).
- **Not tested from the spike**: fullscreen (scenario 6), the auto-hide-off
  step re-run after the heal fix, multi-monitor/DPI re-embed, any soak past
  the spike's 120 s self-terminate cap.

## Live verification (v0.11)

Measured on the running builds at 1920 x 1200 physical @ 125 % DPI
(`target\shot.ps1` captures physical pixels; measurement scripts in `target\`):

- **Shared content rect (section 1)**: `--debug-layout` draws exactly one cyan
  box per card. On the music card it measured x 1177..1609, y 902..1122 =
  432 x 221 px = 344.6 x 176 DIP - the art tile starts flush inside it
  (1180/903), the output chip ends exactly on its right/top edge (1609/902),
  the wave line spans the full interior (1180..1607) and the play button's
  bottom lands on the cyan bottom (1120-1122 vs 1122). The old private
  `MUSIC_INNER` magenta box is deleted.
- **Pill placement, both alignments (section 8)**: `TaskbarAl=1` (centered)
  -> pill fill x 15..294 = exactly 12 DIP from the screen's left edge, in the
  empty band left of the centered icon group; `TaskbarAl=0` (left) -> pill fill
  x 1229..1508 = 280 px (224 DIP) ending 10 px = **8 DIP** before
  `TrayNotifyWnd.left` (1519), never overlapping the tray. Each flip of the
  registry value produced exactly one debounced scan and one 200 ms slide.
- **Alignment-slide debounce fix**: Windows slides the icon group into its new
  alignment for ~550 ms (measured with a 100 ms frame sampler), while the
  structure debounce was 150 ms - the first scan read a half-way group
  (`290,1279`, left band 212 DIP < 224 pill) and Auto fell back to the tray
  with no later event to correct it. `TaskbarAl` flips now use a one-shot
  900 ms debounce (`design::ALIGN_DEBOUNCE_MS`) and a structure event never
  shortens a running alignment wait; verified both directions land on the
  settled group (`547,1373` / `0,839`).
- **Squeeze notice**: no longer fires on an alignment flip (it was armed by
  the mid-slide scan); a full 1 -> 0 -> 1 cycle logged zero `tray.notice`
  lines.
- **Explorer restart**: killing and restarting explorer.exe re-embedded the
  pill under the new `Shell_TrayWnd` (`pill.embed: ... taskbar=0xc0380`) and
  the placement was still correct (pill at 15..294 afterwards).
- **Idle CPU (installed 0.11.0 release)**: **0 ms / 10 s** steady (47 ms in
  the first 10 s after launch is start-up warm-up), WS 82.3 MB / private
  56.2 MB.
- **Gates**: see the table above - 338/0/1, clippy 0, fmt 0, installer green,
  install-smoke 8/8.
- **Pill never hides (section 7)**: with Start open (Ctrl+Esc, menu visibly
  covering the left half) the pill measured 1229..1508 - identical before,
  during and after - and the same holds for taskbar clicks (the accidental
  Quick Settings openings during this session left it untouched too). There
  is no dedicated visibility-change counter in the code: Stage A gave the
  pill no hide path at all (nothing in `window.rs` calls `ShowWindow(SW_HIDE)`
  on it; it rides `Shell_TrayWnd`), so zero changes is structural rather than
  counted. The spike's own `blink-count` record is in
  `tools\spike-embed\RESULTS.md`.
- **`ui-smoke.ps1` adapted to Stage A**: `FindByClass` prefers top-level
  windows, which after embedding resolves to the *card host* (its region is
  `NULLREGION` while the card is closed) and the run died with "pill region
  is empty after bootstrap". The script now finds the pill under the taskbar
  (`FindPill`) and unions both windows' regions in screen coordinates
  (`ScreenRects`): the pill child always carries the pill's rounded region,
  the host carries the card + halo region only while open (measured:
  `NULLREGION` closed, 66 rects open). The full run then passed.

## Card UI screenshots (v0.11)

`powershell -File tools\tests\ui-smoke.ps1` re-run against the installed
0.11.0 release (2026-11-19): **13 PNGs + `pins.log`** in
`target\soak\ui-v0.11\` - the morph mid-frame, then Auto / Music / Call /
Shelf / Timer / Clipboard in dark and light, 61 notches, final pin `None`,
theme restored. The shots were inspected: music shows the dimmed
"Nothing playing" state on the shared grid, timer the stepper + presets +
full-width Start, call the in-box `Open Phone Link` with three equal
buttons, clipboard unchanged, and Auto honestly resolving to Shelf (files
shelved). Human-eye judgement of the new cards is still listed as not
tested.

## Owner bug fixes (v0.11.0, 2026-11-21/22)

Two bugs from the owner's test of the installed build, measured and fixed:

- **Bug 1 - delayed island on auto-hide reveal.** Root cause confirmed by
  geometry: the pill child's rect is parent-relative (`window - bar`), so
  any `sync_anchor` that ran while the bar was tucked baked
  `top = 843 - 1198 = -355`; on reveal the child rode the parent up 58 px
  and landed above the parent's client top, clipped until the next 2 s
  `guard_pill` tick (timer-phase variance = "sometimes delayed, sometimes
  instant"). Fix: `SetWinEventHook` on
  `EVENT_OBJECT_LOCATIONCHANGE | SHOW | HIDE` filtered to `Shell_TrayWnd`
  (`move_hook_proc`, `anchor.rs`) posts `WM_APP_TASKBAR_MOVED`; the
  handler re-anchors and re-aligns the child; `sync_pill_child` now runs
  at the end of every `sync_anchor` visible path; startup gates on
  `anchor::is_allowed()` instead of `anchor.visible` so the island starts
  while the bar is hidden. Live evidence: the hook fired (`re-aligned
  after taskbar move` in the log during an Explorer restart), origin
  alignment held (`pill.Top == card.Top == 843`) across both restarts and
  124 samples, the island survived both restarts, and the old ±40 px
  slide oscillation did not reappear (only 2 legitimate post-restart
  slides). **Not proven live: a natural hide -> reveal cycle - this
  environment cannot hide the taskbar at all** (three forcing methods
  failed: cursor jiggle/injected sweeps, `TaskbarAutoHide=1` + Explorer
  restart, and direct `SetWindowPos` on `Shell_TrayWnd` - Explorer
  re-asserts; StuckRects3 byte8 = 0x02 and `ABS_AUTOHIDE` = 1 confirm
  auto-hide is configured ON while the bar sits at 1140..1200
  permanently). Needs one check on the owner's machine.
- **Bug 2 - performance ("measure everything").** Two measured hot spots
  fixed: (a) `toast.rs::read_window` now gates the `dump_nodes` subtree
  walk (<= 400 cross-process UIA nodes) behind
  `looks_like_toast(class, width, height)`, so ordinary window opens no
  longer pay a 500 ms UIA timeout; (b) `registry_watcher` reads
  `taskbar_alignment()` after each wake and ignores sibling writes under
  `Explorer\Advanced` - only a real `TaskbarAl` flip requests the 900 ms
  align wait + rescan (structure events still arrive via UIA in 150 ms).

### Measurements (installed 0.11.0, fixed build)

- **Idle**: 16-47 ms / 10 s (0.16-0.47 % of one core), WS 69-72 MB,
  private 55-67 MB, 23-29 threads, 555-697 handles.
- **Window churn** (5 windows opened/closed): 625 ms / ~9 s,
  **zero** `toast.read timed out` lines (the old log had them regularly).
- **Card open** (synthetic pill click, card verified open via its region):
  **0 ms / 10 s**; 0 ms / 10 s again after Escape-close.
- **Music playing** (Chrome publishing a real GSMTC session, looping local
  WAV, resting pill with wave animation): **656 ms / 10 s (6.56 %)** -
  the documented 80 ms wave tick, unchanged; **0 ms / 5 s** after the
  session stops. (A PowerShell `WinRT MediaPlayer` does *not* register a
  GSMTC session on this build - the pill stayed on the empty state -
  Chrome does; `target\audio-test.html` + `target\gen-wav.ps1` reproduce
  it.)
- **Alignment registry filter**: a sibling write under
  `Explorer\Advanced` produced 0 new log lines, 0 slides, no pill movement;
  each `TaskbarAl` flip produced exactly 1 debounced `anchor.slide`, 0
  `tray.notice` lines, origin alignment held throughout, and the value
  ended back at 0 (`target\verify-align-flip.ps1`, ALL PASS).
- **UIA timeouts in the whole session**: 2 total, both from the pre-fix
  portion of the log (`phonelink.read` at startup, one early
  `toast.read`); zero after the fix landed.

## Not tested (v0.11)

Stated honestly so nothing is claimed that was not run:

- **Everything under *Not tested (v0.10)* and *Not tested (v0.9)* still
  applies**: a real Phone Link call and live Hold/Resume, real device churn,
  the DPI matrix, clean-VM install, SignPath, Explorer drag-out and human-eye
  card checks.
- **A natural auto-hide hide -> reveal cycle was not proven live for the
  bug-1 fix** (see above): this environment cannot hide the taskbar by any
  forcing method, so the hook's behavior on a real reveal rests on the
  Explorer-restart evidence (the same window-move event path), the
  geometry reasoning and the 124 aligned samples. Owner machine check
  pending.
- **DPI 100 / 150 % and light theme were not re-run for v0.11.** All live
  measurements above are 125 % dark. The card grid is pure layout math, but
  the pixel claims do not transfer across DPI untested.
- **The 60-minute soak was not re-run for 0.11.0** (the v0.9 soak stands; the
  0.11 delta is layout + placement, and placement is event-driven - one scan
  per taskbar event).
- **The idle runner animation from the design PDFs was never implemented in
  any version** (artboards only, no code in git history) and is out of scope
  for 0.11.0 by owner decision - it needs its own y-bump later.
- The one ignored test and the 22 golden engine rows above.

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
