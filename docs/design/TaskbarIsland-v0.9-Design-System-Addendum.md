# TaskbarIsland Design System — v0.9 addendum

Read `TaskbarIsland-v0.8-Design-System.md` first; everything there still applies (fixed sizes, CARD_PAD 7, icon-first, tooltip rules, §10 collaboration contract). This file adds the Music and Timer cards, finishes the "all five cards use one system" goal, and changes ownership. `design-tokens.json` is updated to `0.9.0-design` (new icons, `components.music`, `components.timer`).

Why v0.9 exists: in v0.8 the Music, Timer and Call cards only received the padding change. The user wanted every card redesigned in the same icon-first style. Artboards: Music, MusicPaused, MusicVolume, MusicNone, MusicTip, TimerIdle, TimerRunning, TimerPaused, TimerDone, Stopwatch (plus Call* from v0.8).

## 1. Ownership for v0.9
| Area | Owner |
|---|---|
| Music card (`activities/music/**`), Call card and pill states (`activities/call/**`), tokens/layout/tooltip, rail, pill, release | **A (Codex)** |
| **Timer card (`activities/timer/**`) moves to B**, Shelf, Convert, Clipboard, engines, installer engine fragment, notices | **B (MiMo)** |
Everything else in v0.8 §10 is unchanged (worktrees `D:\ti-agent-a`, `D:\ti-agent-b`, separate target dirs, dev lock, coordination files, changelog fragments, only A merges and releases). Timer's pill states stay with A (pill is A's); B exposes the timer state through the existing activity interface and A renders the pill.

## 2. Music card (content rect ≈ 328 × 176, column, gap 8)
1. **Top row, 96 high:** album art 96 × 96 (radius 8, inner 1 px border), right column: title 16/20 semibold (1 line, ellipsis), artist 12/16 `#B0B0B0`, then a **source chip** (20 high, radius 10, `rgba(108,180,238,.14)`, 12 px source glyph + app name 11 px, `#6CB4EE`). The chip is a button: tooltip "Open in Chrome" + muted "Brings the tab forward"; it focuses the source app's window. If there is no art, use a gradient tile with the music glyph.
2. **Progress row, 18 high:** elapsed (11 px, tabular) · wavy progress · remaining (`-0:29`). The played part is a sine wave (amplitude 3.6, step 7 DIP, stroke 2.6, accent), the unplayed part a flat 3 px `#505050` line, a 3.5 × 14 white knob at the position. **Playing = the wave drifts** (translate 14 DIP over 1.4 s, looping); **paused = the played part is a flat line** and the drift stops. No equalizer graph, ever. Click/drag on the row seeks (if the source supports it; otherwise the row is not interactive and has no knob).
3. **Controls row, 40 high:** centered transport: Shuffle, Previous, **Play/Pause (64 × 40, radius 10, accent fill, dark glyph)**, Next, Repeat (32 × 32 buttons, gap 6); **Volume** button at the right edge. Clicking Volume swaps the whole row for [speaker button (active)] [4 DIP slider] [value]; clicking the speaker or moving the pointer out for 3 s returns to transport. Active shuffle/repeat use the accent color.
4. **Nothing playing (MusicNone):** one 44 px muted music glyph centered; tooltip "Nothing is playing". No text.
5. Tooltips: Shuffle, Previous, Play / Pause, Next, Repeat, Volume / Hide volume, "Open in <app>" (2nd line "Brings the tab forward").
6. Data: Windows System Media Transport Controls (GSMTC). Art from the session thumbnail; accent from the art (dominant saturated color, fallback `#5AA0D8`, clamp lightness so the play glyph stays readable). Unsupported actions are hidden, not disabled.

## 3. Timer card
Two tabs (segmented control, icon-only, amber accent fill): **Timer** (hourglass) and **Stopwatch** (stopwatch). The tab bar is shown only in the idle/set state and in the stopwatch; running, paused and done views use the whole card.
- **Set (TimerIdle):** row 52 high: [−1 min] **time 44/48 light** [+1 min] [Start 64 × 40 amber]. Below, preset grid 5 × 2 filling the remaining height: 1m, 5m, 10m, 15m, 25m, 30m, 45m, 1h, 90m, 2h. Tapping a preset selects it (amber fill + border) and updates the time; it does not start. Start begins the countdown. Time steps: ±1 min (hold = repeat, accelerating). Last used duration is remembered.
- **Running:** 124 px ring (stroke 7, amber on `#3A3A3A`, round caps) with time 28/32 light inside; right: 2 × 2 tiles, 52 high: **Pause** (amber primary), **Reset**, **+1m**, **+5m**; below them the end time (bell + `15:42`, tooltip "Rings at 15:42"). The ring drains clockwise from 12 o'clock.
- **Paused:** ring and time dimmed; the primary tile becomes **Resume**.
- **Done:** full amber ring pulsing (1 s), bell glyph above `00:00`; tiles: **Dismiss** (primary), **Snooze 5 min** (`+5m`), **Restart**, **Set a new timer**. The pill shows the alert (owner A). The sound is the system notification sound once (setting: off by default for a repeating alarm).
- **Stopwatch:** time `00:12` + small `.48` (36/40 light), buttons Lap (40 × 40), Pause/Start (56 × 40 amber), Reset (40 × 40); below, a scrolling lap list (rows 28, newest first: lap number, split, total; alternating `rgba(255,255,255,.04)`).
- Multiple timers: one at a time in this version. Starting a new timer while one runs asks nothing; Reset first (tooltip on Start explains nothing else).
- Tooltips: Timer, Stopwatch, −1 min, +1 min, Start, "1 minute"…"2 hours", Pause, Resume, Reset, Add 1 minute, Add 5 minutes, Dismiss, Snooze 5 min, Restart, Set a new timer, Lap.
- Accessibility: the running time is exposed to UI Automation as a text element that updates once per second (not per frame); the ring is decorative (`aria-hidden` equivalent).

## 4. Rail
Music and Timer rail tiles now open the new cards (`Music`, `Timer`). Nothing else about the rail changes.

## 5. Clipboard image preview is a bug, not a design gap
The v0.8 artboards (Clip, ClipHover, ClipPreview) already specify real thumbnails and the hover preview. The built v0.8.0 app still shows only an image icon and the dimensions. Treat it as a P0 defect (see Agent B's v0.9 prompt for the diagnosis steps). Acceptance: three different screenshots copied in a row show three visibly different 64 × 40 thumbnails; hovering one for 450 ms shows the 192 × 118 preview.
