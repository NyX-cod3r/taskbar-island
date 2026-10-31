# TaskbarIsland: agent rules
- Rust + windows-rs, Win32 + DirectComposition + Direct2D. No webview/Electron/WinUI.
- Event-driven only. No busy loops or polling under 1s except while animating.
- Idle target: ~0% CPU, <15 MB RAM. Report measurements in every PR.
- Never block the UI thread; WinRT calls go on a worker, results posted via PostMessage.
- All taskbar-geometry logic lives in anchor.rs only.
- Do not display date/time/notifications/calendar. Those already exist on the taskbar.
- No network calls. No telemetry. Everything runs locally.
- Each activity is a module implementing the Activity trait: id, priority, poll/subscribe, render(compact|expanded), on_click.
- Add unit tests for state.rs priority logic. Keep unsafe blocks small and commented.

## Status (updated 2026-11-01)
- **Phase 1 COMPLETE** (commit a8f4744): empty anchored pill, `WS_CHILD` inside `Shell_TrayWnd`
  (no topmost/z-order fight), geometry only in anchor.rs. Rendering: D3D11 ->
  `CreateSwapChainForComposition` (premultiplied alpha) -> DComp visual on the pill HWND -> D2D
  device context. Per-frame backbuffer binding is recomputed every draw (no cached refs across
  Present). Verified visually at 125% DPI.
- Measurements (release): idle CPU **0 ms / 5 s** (0% of a core); RAM **~55 MB** working set
  (target <15 MB: miss, mostly graphics DLL/GPU pages — revisit later); pill 150x62 physical px,
  rect (1279,1145)-(1429,1207). 5/5 unit tests, clippy --all-targets clean.
- Known geometry quirk: computed pill bottom (y=1207) is 7 px past the screen bottom; the child
  window is clipped there. Intentional? — pending owner review.
- Phase 2 scaffolding landed: theme.rs (registry light/dark), state.rs (Activity trait +
  ActivityQueue), activities/media.rs (GSMTC worker -> mpsc -> WM_APP_MEDIA),
  activities/timer.rs, Indicator bars in render.rs. Next: hover/click input, animation engine,
  media pill polish.
- **Phase 3 COMPLETE**: media pill interaction.
  - Hover expand: 120 -> 220 DIP via `anchor::compute(width_dip)`, right edge stays pinned
    next to the tray. 16 ms WM_TIMER smoothstep tween (~180 ms, EXPAND_SECONDS 0.18); timer is
    killed after the tween — the only "polling" that ever runs is during animation.
  - Expanded layout: title + ellipsized dimmed artist line via
    `Renderer::ensure_layout` (cached by (text, max width)).
  - Click = play/pause toggle: WM_LBUTTONUP -> mpsc -> media worker
    `TryTogglePlayPauseAsync`; `PlaybackInfoChanged` republishes. Paused: flat bars +
    0.55-dimmed text; playing: uneven bars.
  - **Reentrancy crash found & fixed**: `SetWindowPos` (called under the APP RefCell borrow,
    e.g. first expand tick) synchronously dispatches WM_WINDOWPOSCHANGING/NCCALCSIZE/NCPAINT/
    ERASEBKGND/WINDOWPOSCHANGED/MOVE/SIZE into wnd_proc -> nested `borrow_mut()` panicked
    ("RefCell already borrowed"). wnd_proc now `try_borrow_mut()` and defers nested messages
    to DefWindowProcW; empirically verified pair set. WM_NCDESTROY clears the slot
    best-effort too. Phases 1-2 never hit this only because apply() skipped SetWindowPos
    when the rect was unchanged.
- Phase 3 measurements (release): idle CPU **0 ms / 5 s**; working set **59.4 MB**, private
  49.5 MB (RAM pass is deliberately last). 7/7 unit tests, clippy --all-targets clean.
  Synthetic repro (SetCursorPos + mouse_event, DPI-aware ctx): hover enter -> expand -> leave
  -> collapse -> click pause -> click resume; zero panics, log 237 lines clean.
- **Phase 4 COMPLETE**: file shelf (activities/shelf.rs).
  - OLE drop target (`#[implement(IDropTarget)]` + `RegisterDragDrop`) installed on the pill
    HWND; `OleInitialize(None)` once on the UI thread. Copy semantics; the callback only
    fills a shared `Arc<Mutex<Vec<String>>>` slot and posts `WM_APP_SHELF` (drop / hover
    enter / hover leave) — mirroring the media bridge so COM callbacks can never re-enter
    `App`. CF_HDROP enumeration via `DragQueryFileW` with `ReleaseStgMedium` on all paths.
  - Arbitration now uses `ActivityQueue` (state.rs): `Priority::Shelf` sits ABOVE `Media`
    (a drop is a direct user action on the pill); queue entries update only on events.
    `App.refresh_content()` pushes the winner into the renderer (media / shelf / idle) and
    draws; renderer recreation paths push the same resolved state.
  - Click behavior branches: shelf -> open all parked files (`ShellExecuteW`, fire-and-forget)
    and clear; media -> play/pause toggle as before.
  - Rendering: `Indicator::Files(count)` draws a staggered thin-line stack (max 4);
    one file shows name + parent folder, a batch shows first file + "N files shelved"; an
    empty pill under a drag shows the "Drop to shelve" hint; drop-hover brightens fill/border
    like mouse hover.
  - Feature gates added: Win32_System_Com, Win32_System_Com_StructuredStorage,
    Win32_System_Ole, Win32_System_SystemServices; windows-core 0.62 as an explicit dep
    (the `implement` macro emits `windows_core::` paths).
- Phase 4 measurements (release): idle CPU **0 ms / 5 s**; working set **59.6 MB**, private
  49.3 MB. 11/11 unit tests (4 new shelf + priority-order update), clippy --all-targets
  clean. Regression synthetic pass (hover expand/leave/toggle) reproduced media pill under
  the new refresh_content flow; process alive, no panics. Real drag-drop from Explorer is
  the remaining user-eye verification (DoDragDrop cannot be scripted reliably).
- **Design Spec v0.1 Implemented**:
  - **Pill stays put**: The compact pill never moves or resizes on hover. Vertical thickness follows the
    owner's request: it keeps the first build's chunky band (62 px at 125%, tray height + 2 px, 4 DIP below
    the tray top). Widths remain per-activity. Radius 10 DIP.
  - **Windows material**: Compact fill white 9%, 1 px border at 10%. Hover/press fill white 15%,
    border white 20%, with a 200 ms cubic-bezier(.2,.8,.2,1) ease on fill+border only (16 ms WM_TIMER
    that dies when the tween lands). No scaling on hover or press.
  - **Flyout above taskbar**: Separate `WS_POPUP` window opens 12 px above taskbar, centered over pill
    (verified: card bottom 15 px above the taskbar top at 125%, delta 0 px from pill center).
    Uses Windows dark flyout material (`rgb(40, 40, 40)` at 95%, 1 px white 9% border, 8 px radius,
    shadow `0 8 32 black 37% + 0 2 8 black 25%`).
  - **Motion**: Open: 180 ms fade, 220 ms scale .94 to 1 and rise 6 px (`cubic-bezier(.2, .8, .2, 1)`).
    Close: 110 ms fade and scale back. Auto-close 700 ms after pointer leaves pill + flyout.
  - **Click outside**: a low-level mouse hook is installed only while the flyout is open and unhooked on
    hide/destroy; an outside click starts the 110 ms close immediately. The hook never swallows clicks.
  - **All 5 Activities & Sizes**:
    - Music: Compact 184 wide (Art, title, 3-bar equalizer); Expanded 384x164 (Art, title, equalizer, progress, previous, play, next).
    - Call: Compact 176 wide (Mic dot, name, timer); Expanded 384x136 (Avatar 'P', name, timer, Mute, Speaker, End).
    - File shelf: Compact 136 wide (204 on drag: dashed border "↓ Drop to hold"); Expanded 384x176 (Header, 3 file tiles: PDF, PNG, ZIP, hint, Clear).
    - Timer: Compact 116 wide (Ring, time left); Expanded 320x124 (Ring with "12:40", "Study session", "Running", pause, +1 min, stop).
    - Volume HUD: Compact 224 wide (Speaker, level bar, value); Never expands, dismisses after 1.8 s.
  - **Priority order**: Call > Timer finishing > Volume HUD > Music > Shelf. The expanded card is never
    swapped while open (flyout content is only pushed on open).
  - **Interactive preview controls**: Companion prototype panel (`--prototype` or right-click pill menu)
    with buttons to preview each activity, simulate file drag, and collapse island. Hotkeys Ctrl+Alt+1..5.
  - Tests: 16/16 passed, clippy --all-targets clean.

- **Phase 5 COMPLETE — countdown + ended banner** (uncommitted with the design slice):
  - Ctrl+Alt+T starts the countdown; the hotkey cancels while running and dismisses the ended banner.
  - Running timer uses `Priority::Idle` so media outranks it; at zero it promotes to `Priority::TimerEnding`
    (above the volume HUD). The 1 Hz ticker is killed at zero, so an idle ended banner costs 0 % CPU.
  - Ended banner: "Timer done", click the pill or press Ctrl+Alt+T to dismiss. Log markers:
    `timer.start: Ns`, `timer.cancel`, `timer.ended`, `timer.dismiss`, `timer.dismiss (hotkey)`,
    `timer.cancel (stop)`.
  - Flyout: pause/resume kills/restarts the 1 Hz ticker; +1 min extends; stop closes and clears.
  - `TBI_TIMER_SECS` (1..=3600) stays as an automated-verification override.

- **Phase 6 COMPLETE — registry Tweaks**:
  - `HKCU\Software\TaskbarIsland`: `FillPercent` (4–20, default 9), `Radius` (4–16, default 10),
    `Accent` (COLORREF 0x00BBGGRR, absent = system accent), `TimerMinutes` (1–60, default 5).
  - Right-click the pill → **Tweaks** submenu: fill 4/6/9/12/15/20 %, radius 6/8/10/12/14,
    timer 1/5/10/25 min, accent System/Blue/Amber/Red/Green/Purple. Choices persist and apply live to
    the pill and the open flyout (no restart). Values are clamped on read, so hand-edited registry
    cannot break geometry. `WM_SETTINGCHANGE` still refreshes light/dark + system accent.
  - Verified by driving `WM_COMMAND`: fill 20 % sampled rgb(51,51,51) (20 % of white), radius 14
    visibly rounder, red accent on the timer ring, reset back to 9 %/10/system; timer hotkey logged
    `timer.start: 60s` after choosing "Timer 1 min".

- **Verification pass (Design Spec v0.1)** — all measured on the running release build at 125 % DPI:
  - Idle pill 80×62 px, rect (1384,1145)-(1464,1207), 12 DIP left of tray, hover never changes the rect.
  - Fill/border: idle rgb(23,23,23) → hover mid rgb(36,36,36) at ~50 ms → hover end rgb(38,38,38);
    press equals hover fill, no scaling.
  - Flyout: card 480×205 px (384×164 DIP), bottom 15 px above the taskbar (12 DIP), centered on the pill
    (0 px delta), never over the tray.
  - Auto-close after leaving: true; click outside: hides immediately; volume HUD click never expands and
    auto-dismisses at 1.8 s.
  - Drag target: 255×62 px (204 DIP), dashed border + "↓ Drop to hold".
  - Countdown 20 s → ended banner → click dismiss → idle; flyout buttons (music play, call End, shelf
    Clear, timer pause/stop) all act and close as specified.
  - Measurements (release): fresh idle CPU **0 ms / 10 s**, working set **82.9 MB** (target <15 MB —
    RAM pass is next and deliberately last), 16/16 tests, clippy clean.
  - Known gaps: real drag from Explorer still needs a human (DoDragDrop cannot be scripted); Volume HUD
    uses a demo level (no volume-key hook yet); Call is a demo activity; music prev/next only change the
    demo title; compact icons use text placeholders; the pill's bottom 7 px sit below the screen edge,
    exactly as in the first build (owner-approved thickness).

- **RAM pass (deliberately last) — practical floor reached, <15 MB is not reachable with this stack**:
  - **Lazy flyout**: `FlyoutWindow::register_class()` runs at startup; the flyout HWND and its GPU
    resources are created on first open (`FlyoutWindow::ensure`) and are skipped entirely for Idle and
    the Volume HUD, which never expand. Idle dropped **82.9 → 55.7 MB** WS (82.1 → 45.3 MB private).
  - **One shared GPU stack** (`src/gpu.rs`): one D3D11 device + DXGI factory + DComp device + D2D device
    + DWrite factory for the whole process, created on first use. Each window keeps its own swapchain,
    composition target/visual and D2D device context. The flyout's incremental cost fell from ~27 MB to
    **~5.6 MB** (63.4 MB WS / 52.5 MB private with a card open).
  - **Final measurements (release, 125 % DPI, Intel iGPU)**:
    - Fresh idle: **0 ms CPU / 10 s**, WS **57.6 MB**, private **47.3 MB**, 11 threads, 282 handles.
    - First flyout open: visible ~100 ms after the click (one-time device + swapchain creation), then the
      normal 180/220 ms open animation.
    - Repeated open/close ×20: plateaus at 67.0 MB WS / 56.2 MB private (+3.5 MB D2D/DWrite cache, no
      linear leak). 16/16 tests, clippy clean.
  - **Where the memory is** (measured by temporarily gating subsystems, both flags removed afterwards):
    - App + OLE + media listener, no renderer: **17.4 MB WS / 2.4 MB private**.
    - Media listener alone: **+5.3 MB WS**, private unchanged (+0.3 MB) — WinRT/shared pages.
    - D3D11 + DXGI + DComp + D2D + DWrite + Intel UMD: **~+40 MB WS / ~+45 MB private**.
  - Lowering the requested D3D feature level to 10.0 changed nothing; `SetProcessWorkingSetSize(-1,-1)`
    can force WS to ~0 but private commit stays at 47 MB, so it is not used (cosmetic only).
  - **Conclusion**: the <15 MB target cannot be met while the mandated Win32 + DirectComposition +
    Direct2D rendering stack is in use on this GPU; ~47 MB private / ~57 MB WS is the honest floor.
    The remaining lever would be replacing the renderer (layered-window GDI or D3D9Ex), which contradicts
    the architecture rules. Idle CPU (0 %) and event-driven behavior are unaffected.
