# Taskbar modes

**Which pill mode shipped in v0.11:** v0.11.0 ships the **embedded** pill — the island's pill is a
`WS_CHILD` window re-parented into `Shell_TrayWnd` (§7 Stage A), so it lives inside the taskbar's
own child list, hides and shows with the bar, never fights the taskbar for z-order, and needs no
separate top-level window at rest. The embedded child is created once at startup and healed by the
2 s z-order guard (a dead pill after an Explorer restart is re-embedded on `TaskbarCreated`, a live
one is only re-raised when a taskbar sibling sits above it). If the taskbar refuses the embed
(`SetParent`/`SetWindowPos` sequence fails), the app falls back to the **separate** top-level pill —
the card window draws the pill itself exactly as before Stage A — and logs `pill.embed: refused`.
The card/flyout itself is and remains a separate top-level popup in both modes.

## Modes (v0.12.0)

v0.12.0 adds three things on top of v0.11's always-embedded pill: a **`Taskbar mode`
choice** (Auto / Always embedded / Always separate) with **safe mode** under another
taskbar tool, and **edge awareness** — the island now follows the taskbar to the top
edge, and pauses with a notice card when the bar sits on a left or right edge. The
`Island position` setting remains dropped (Auto-only placement, owner decision); the
flyout stays 400×192 on every edge.

### `Taskbar mode` and safe mode (§4)

Right-click the tray icon → **Taskbar mode**: **Auto** (default), **Always embedded**,
**Always separate**, remembered in `settings.json` under `taskbarMode` and applied
immediately (no restart). Detection of TranslucentTB / Windhawk / ExplorerPatcher /
StartAllBack / TaskbarX / RoundedTB runs at launch (ToolHelp process + Explorer module
snapshot) and later via a WMI process-creation event — no polling. In **Auto**, a
detected tool drops the pill out of the bar (the card window draws it as a top-level
window, the pre-0.11 arrangement) and raises one tray notice per process; embedding into
a bar another tool has re-styled is the configuration that produced Event Viewer
1000/1002 plus a WER CrashDump. **Always embedded** overrides the safety net (the
owner's call), **Always separate** never embeds. Detection does not un-detect within a
session — a bouncing pill would re-hand Explorer a child window on every bounce.
Full detail, the material × pill-mode look test and the crash evidence to watch:
`docs/compat.md`.

### Edges (§2 top/bottom, §3 left/right)

The edge comes from `SHAppBarMessage(ABM_GETTASKBARPOS)` with a rect fallback, re-read
on `LOCATIONCHANGE` / `WM_SETTINGCHANGE` / `TaskbarCreated` / display-DPI change, each
debounced 150 ms (`design::EDGE_DEBOUNCE_MS`, `EDGE_DEBOUNCE_TIMER_ID`), so one taskbar
move — a storm of `LOCATIONCHANGE`s — flips the island exactly once
(`anchor::edge_pending` / `edge_commit`). Real thickness is used everywhere.

* **Bottom** — v0.11's behavior, unchanged: pill in the bar, flyout 12 DIP above the
  bar, rail on the left, free-space rules intact (8 DIP gaps → one-time squeeze notice).
* **Top** — mirrors it: the pill still embeds in the bar, the flyout hangs **below the
  taskbar bottom + 12 DIP**, rail still left; everything else (400×192 flyout, free-space
  rules, 12 DIP edge margin) is the bottom behavior transposed.
* **Left / right** — no horizontal room, so the island **pauses**: the pill and flyout
  hide, drawing and animation stop, but clipboard capture and all timers keep running.
  One **notice card** per edge change (360×176, `design::NOTICE_W/H`) explains it with
  two exits — **Open taskbar settings** (`ms-settings:taskbar`) and **Quit island**; Esc or
  an outside click dismisses the card but **keeps the island paused**. The tray menu
  shows a grayed `Paused` row while paused, a finished countdown posts a `Timer
  finished` tray notification instead of an ended banner (there is no pill to show it
  on), and moving the bar back to the top/bottom **auto-resumes**. The primary monitor
  only. `NOTICE_BODY` copy in `render.rs` is a draft pending the owner's confirmation.

### `--simulate-edge`

`taskbar-island.exe --simulate-edge top|bottom|left|right` (`anchor::set_simulated_edge`)
transposes the bar onto any edge of the same monitor, so all four paths can be exercised
on a machine whose real taskbar never moves — including the pause/resume cycle. Parsed
in `main` before any geometry runs.
