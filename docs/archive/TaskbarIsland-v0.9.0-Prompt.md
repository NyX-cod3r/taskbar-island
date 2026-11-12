# TaskbarIsland v0.9.0 — single-agent build prompt

You are the only agent working on this repo for v0.9.0. There is no second agent, so ignore everything about two agents, ownership splits, coordination files, `from-A.md`/`from-B.md`, the dev lock, and the "do not edit the other agent's files" rules in the older documents. Everything else in them still applies.

Read first, completely: `TaskbarIsland-v0.8-Design-System.md` (fixed sizes, CARD_PAD 7, icon-first rule, tooltips, exact tooltip copy), `TaskbarIsland-v0.9-Design-System-Addendum.md` (Music and Timer specs; ignore its §1 ownership table), `design-tokens.json` (0.9.0-design). The artboards in the design folder are the visual spec: Music, MusicPaused, MusicVolume, MusicNone, MusicTip, TimerIdle, TimerRunning, TimerPaused, TimerDone, Stopwatch, Call*, CallPill*, Clip*, Shelf*, Convert*.

## 0. Goal and constraints
v0.9.0 finishes "every card uses the same icon-first system": redesign Music and Timer, check Call against its artboards, fix the Clipboard image-preview bug, then test everything and release.
Fixed: pill 224 DIP wide; card 400 × 192 DIP, centered, bottom 6 DIP above the taskbar; inner padding 7 (`card_content_rect()`); no scale on hover; Windows-style corners; icon-first (text only for names, previews, numbers, format names); tooltips only through the shared system (450 ms delay, 300 ms repeat window); fully offline, no telemetry, no auto-update; local engines only.
Remote: `https://github.com/NyX-cod3r/taskbar-island.git` (the user confirmed it is theirs; GitHub account NyX-cod3r; commit author name Satyam). Never print or commit tokens, engine binaries, installers or user data.

## 1. Setup (10 minutes)
1. `git fetch --all --tags`. Confirm tag `v0.8.0` exists and `main` contains it. If not, stop and tell me; do not rebuild history.
2. Create `release/v0.9.0` from `main`, push it. Work in one worktree, `D:\ti-agent-a`, branch `agent/v0.9.0` from `release/v0.9.0`. `CARGO_TARGET_DIR=C:\build\ti`. Delete the old second-agent worktree and `D:\ti-agent-b` only if I confirm; do not touch `D:\dynamic\taskbar-island`.
3. `gh --version` and `gh auth status`. If `gh` is missing, tell me (`winget install GitHub.cli`, then `gh auth login`); continue with everything except the release step.
4. Run `cargo build` and `cargo test` once on the untouched baseline and record the result in `docs/release/v0.9.0/test-report.md`. Update `docs/CONTRACTS.md` only if you find it out of date.
5. Commit in small, reviewable commits on your branch; open one PR per phase into `release/v0.9.0` and merge it yourself after the gate passes. No force-push on shared branches.

## 2. Phase 1 (P0) — Clipboard image previews do not show
Symptom from the shipped app: image rows show only a generic icon and `573 × 353`, `615 × 342`, `37 × 13`; images cannot be told apart. Spec (artboards Clip, ClipHover, ClipPreview): 50 DIP image rows with a 64 × 40 thumbnail, bold dimensions, muted `FORMAT · size`; hover preview 192 × 118 after 450 ms, overlay inside the card, caption `W × H · FORMAT · size` plus `from <app>` (process of `GetClipboardOwner`).
Find the cause with evidence, not guesses:
1. Reproduce with Win+Shift+S, Snipping Tool, PrintScreen, browser "Copy image", Paint. In a debug build log (never pixel data) the clipboard formats (`EnumClipboardFormats`, including registered names like `PNG`), the chosen format, decode result, thumbnail size and store result.
2. Decode: support `PNG`, `CF_DIBV5`, `CF_DIB`, `CF_BITMAP`. Check header size, `BI_BITFIELDS`, 32 bpp with and without alpha (premultiplied or straight), bottom-up and top-down, palette, stride, `GlobalLock`/`GlobalSize`. Fall back to `GetDIBits` from `CF_BITMAP`.
3. Thumbnail: is it generated, where is it stored, is it encrypted (DPAPI) and decrypted on read, is its key written to the item record, is generation skipped for some formats or small images?
4. UI: does the row renderer have an image branch that draws the thumbnail, or always the icon? Is the row 50 DIP high? Stale virtualization cache? Is the bitmap created on the right thread and kept alive?
5. Migration: items from older builds have dimensions but no thumbnail. Generate lazily from the stored image (neutral placeholder meanwhile); if the stored image is gone show a dimmed icon with the tooltip "Preview not available".
6. Images smaller than the thumbnail show at natural size on a checkerboard; larger ones scale to fit (letterbox, never stretch).
Tests that must exist: (a) integration test setting `CF_DIBV5`, `CF_DIB` and `PNG` images on the real clipboard, waiting for capture, asserting the stored thumbnail has the right size and is not blank (pixel variance); (b) hand-built DIB decode tests (24/32 bpp, bitfields, top-down, palette, odd widths, corrupt); (c) UI render test painting an image row offscreen: three different test images give three different thumbnail hashes and the area is not the icon; (d) 37 × 13 draws at natural size. Write root cause and fix in `docs/qa/clipboard-preview.md` with before/after screenshots. Do not fix it by drawing a generic picture; thumbnails must come from the real image.
Acceptance: copy three different screenshots in a row, see three visibly different thumbnails; hover shows the preview.

## 3. Phase 2 — Music card (addendum §2; artboards Music*)
- Layout through `card_content_rect()`. Top row 96 high: art 96 × 96 (radius 8), title 16/20 semibold, artist 12/16 `#B0B0B0`, source chip (20 high, radius 10, `#6CB4EE` on `rgba(108,180,238,.14)`) that opens/focuses the source app (tooltip "Open in <app>", muted "Brings the tab forward"). Progress row 18 high: elapsed, wave, remaining (`-0:29`). Controls row 40 high: Shuffle, Previous, Play/Pause (64 × 40, radius 10, accent), Next, Repeat; Volume at the right edge swaps the row for [speaker][4 DIP slider][value], auto-return after 3 s outside.
- Wavy progress: sine amplitude 3.6, step 7 DIP, stroke 2.6, accent; unplayed is a flat 3 px `#505050` line; 3.5 × 14 white knob. Playing: the wave drifts 14 DIP per 1.4 s loop. Paused: played part is flat. No equalizer graph. Animate only while the card is visible and music plays (no timers otherwise). Snap to the pixel grid at 125%/150%.
- Data: Windows media session (GSMTC): title, artist, thumbnail, position/duration with a local clock between updates, state, shuffle/repeat and capabilities (hide unsupported controls, do not disable), source AUMID → friendly name/icon. Seek only if `PlaybackPositionEnabled`, otherwise no knob.
- Accent from art: downsample to 32 × 32, dominant saturated color, ignore near-gray, clamp lightness so the play glyph keeps ≥ 4.5:1 contrast; fallback `#5AA0D8`; cache per track.
- Volume: master volume of the default render endpoint (`IAudioEndpointVolume` + callback); not per-app.
- Nothing playing: one 44 px muted music glyph, tooltip "Nothing is playing".
- Tests: wave generator (lengths, endpoints, flat when paused), accent extraction (gray, very light, very dark, single color), time formats (`3:07`, `-0:29`, hours), capability hiding, layout at 100/125/150/175%, a simulated media-session provider so UI states are testable without a player.

## 4. Phase 3 — Timer card (addendum §3; artboards Timer*, Stopwatch)
- Two icon tabs (Timer hourglass, Stopwatch) in a segmented control with amber fill, shown only in Set and Stopwatch; Running, Paused and Done use the full card.
- Set: row 52 high [−1 min] time 44/48 light [+1 min] [Start 64 × 40 amber]; preset grid 5 × 2 filling the rest: 1m 5m 10m 15m 25m 30m 45m 1h 90m 2h. A preset selects (amber fill + border) and updates the time; Start begins. ±1 min with hold-to-repeat, max 99:59:59, last duration remembered.
- Running: 124 px ring (stroke 7, amber on `#3A3A3A`, round caps, from 12 o'clock, drains clockwise), time 28/32 light inside; right 2 × 2 tiles 52 high: Pause (primary), Reset, +1m, +5m; below, bell + end time (tooltip "Rings at 15:42"). Paused: dimmed, primary becomes Resume. Done: pulsing ring (1 s) with bell above `00:00`; tiles Dismiss (primary), Snooze 5 min, Restart, Set a new timer; system notification sound once; the pill shows the alert (priority: incoming call > active call > timer alert > clipboard `+N` > music > idle).
- Stopwatch: `00:12` + small `.48`, buttons Lap, Pause/Start, Reset, lap list rows 28 (newest first: number, split, total), scrollable.
- Logic: monotonic clock (`Instant`); store the absolute end time; recompute after sleep/wake; a timer that ended during sleep shows Done on wake. UI updates once per second, ring smooth only while visible, zero timers when nothing runs. A running timer survives app restart; a stopwatch does not. One timer at a time.
- Tests: state machine tables (set, run, pause, resume, reset, +1m/+5m, snooze, restart, beyond max), injected clock (sleep, wall clock moving backwards), lap math, formats (`04:12`, `1:05:09`, `00:12.48`), persistence across restart, layout at all DPIs, exact tooltip strings.
- UI Automation: the running time is a text element updated once per second; the ring is decorative.

## 5. Phase 4 — Call card check (artboards Call*, CallPill*)
Compare the shipped Call UI with every Call artboard and fix differences (sizes, tooltip copy, states). Re-test real Phone Link items that were PARTIAL/NO-GO in v0.8 and update `docs/spikes/phone-link.md`. Caller data stays in memory only; logs mask numbers.

## 6. Phase 5 — Regressions
Run every Shelf and Convert flow at padding 7 and fix anything off-spec: tile sizes, tooltip placement, clipped text, hidden single tabs, Done view "no gain" for PDF compress, engine-missing option hiding.

## 7. Phase 6 — Whole-app gate (write `docs/release/v0.9.0/test-report.md`)
1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, dependency denylist test (no network crates), engine tests with `TI_ENGINES_DIR` set (skipped, never failed, when an engine is missing; list which).
2. Manual at 100/125/150% scaling, taskbar bottom and auto-hide, light and dark theme: five rail tiles; wheel on the pill cycles five activities; every card exactly 400 × 192, centered, 6 DIP above the taskbar; padding 7 in every card (measure two edges per card); every icon shows its tooltip after about 450 ms.
3. Per card: Music (play, pause, seek, volume, source chip, nothing playing), Timer (set, preset, ±, start, pause, resume, reset, +1m, +5m, done, snooze, stopwatch laps), Call (simulator and real if available), Shelf (40 files, scroll, Clear), Convert (one per file kind), Clipboard (three image thumbnails, hover preview, 7-day expiry with injected clock, 30 limit, pin, clear).
4. 30-minute soak with simulators: no memory or handle growth, idle CPU near 0 with all cards closed.
5. Installer: upgrade over 0.8.0 (settings, clipboard history, Shelf contents survive), custom engine selection, uninstall.
Screenshots into `docs/release/v0.9.0/screens/`.

## 8. Phase 7 — Release v0.9.0
1. Version `0.9.0` in `Cargo.toml`, `Cargo.lock`, installer `AppVersion`, resource version info, About text; the `version_consistency` test must pass.
2. `CHANGELOG.md` `## 0.9.0 (<today>)`: Music card redesign (wavy progress, source chip, volume), Timer card redesign and stopwatch, Call card aligned to the design, Clipboard image previews fixed, all cards on the shared icon-first system.
3. `cargo build --release --locked`; build the installer; SHA-256. Check size against GitHub's 2 GB per-asset limit; split if needed and tell me. Unsigned build: mention SmartScreen in the release notes.
4. Install it silently over 0.8.0 on this PC (`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART` for Inno), confirm `DisplayVersion = 0.9.0`, About shows 0.9.0, app runs, engines selection respected.
5. `git tag -a v0.9.0 -m "TaskbarIsland v0.9.0"`, push branch and tag, `gh release create v0.9.0 <installer> <sha256 file> --title "TaskbarIsland v0.9.0" --notes-file docs/release/v0.9.0/RELEASE_NOTES.md`, then `gh release view v0.9.0 --json url,assets`, download the asset and compare the hash. Merge `release/v0.9.0` into `main` (PR, after checks), then remove the working branches.
6. Final report to me in chat: release URL, installer name and size, SHA-256, tests run and skipped, anything not done. Do not claim anything you did not run.

## 9. Do-not list
No network access, telemetry or auto-update. No text labels on buttons. No equalizer graph. No scale on hover. No silent changes to Windows notification settings or the default audio device. No pushing to `main` before the release step. No "done" without evidence (command output, screenshots, test names).
