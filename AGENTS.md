# TaskbarIsland: agent rules
- **Files stay inside `D:\dynamic`.** Never create, write or copy files anywhere else — no
  scratch dirs in `%TEMP%`, no user-profile folders, no drives outside `D:\dynamic`. Temporary
  scripts, logs and captures belong under the repo (`D:\dynamic\taskbar-island`, e.g. `target\`
  or `tools\`); the only exception is Windows itself writing its own runtime data (registry,
  `%APPDATA%\TaskbarIsland\settings.json` created by the app at runtime).
- Rust + windows-rs, Win32 + DirectComposition + Direct2D. No webview/Electron/WinUI.
- Event-driven only. No busy loops or polling under 1s except while animating.
- Idle target: ~0% CPU, <15 MB RAM. Report measurements in every PR.
- Never block the UI thread; WinRT calls go on a worker, results posted via PostMessage.
- All taskbar-geometry logic lives in anchor.rs only.
- Do not display date/time/notifications/calendar. Those already exist on the taskbar.
- No network calls. No telemetry. Everything runs locally.
- Each activity is a module implementing the Activity trait: id, priority, poll/subscribe, render(compact|expanded), on_click.
- Add unit tests for state.rs priority logic. Keep unsafe blocks small and commented.
- **Verify on the real install, every time the code changes**: stop the running island
  (`Stop-Process -Name taskbar-island`), install the freshly built `dist\TaskbarIsland-Setup.exe`
  over the real per-user install, launch it and confirm it comes up. Run
  `powershell -File tools\tests\install-smoke.ps1` — it does the whole loop (stop → install →
  byte-match against `target\release` → launch → pill/tray windows → single-instance) and exits
  non-zero on any failure. `cargo test` alone is not evidence that a change works: the pill, the
  tray, the clipboard listener and the installer only exist in the installed copy. This replaces
  the running app, so do it as the last step before reporting, not in the middle of a session.
- **Two installers, one identity.** `tools\build.ps1 -Mode Installer` builds the *core* installer
  (`dist\TaskbarIsland-Setup.exe`: our exe + LICENSE + NOTICE only — no third-party code), and
  `-Mode Converters` builds the optional engines pack (`dist\TaskbarIsland-Converters-Setup.exe`,
  unsigned under our name). Releasing is `docs/release-process.md`; code signing is
  `docs/signing.md` (all steps still `[AFTER APPROVAL]`).

## Status (updated 2026-11-28)
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
  - Click behavior branches: shelf -> open the clicked entry (`ShellExecuteW`); Ctrl+click
    toggles selection instead of opening; media -> play/pause toggle as before.
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
- **Shelf drag-out and folders** (2026-11-06):
  - `ShelfStore` parks files *and* folders (`Path::exists` instead of `is_file`); folder
    tiles render a `DIR` badge and the "Folder" line, and the subtitle counts items.
  - Dragging a tile starts a real OLE drag (`FileDataObject` CF_HDROP, several paths in one
    medium) offering `DROPEFFECT_COPY | DROPEFFECT_MOVE`; Explorer chooses (default copy,
    Shift move). A move outcome removes the parked paths (`remove_paths`), a copy keeps them.
    Drag start uses the system `SM_CXDRAG` threshold and `SetCapture` so a fast pointer still
    reaches it; `DoDragDrop` runs after the surface borrow is released.
  - Ctrl+click toggles a tile into the selection (async `GetAsyncKeyState` because the click
    may arrive without focus), selected tiles get an accent tint + 2 px ring, and dragging
    any selected tile takes the whole selection. Selection clears when new drops reorder the
    shelf or a move removes entries.
  - The card shows three tiles per page (`view::SHELF_PAGE_SIZE`) with an "n / m" indicator;
    the wheel pages it while the pointer is over the open card and still cycles activities
    anywhere else. New drops and Clear all reset to page 1; pages clamp after removals.
    Page slicing keeps absolute shelf indices, so Ctrl+selection and drag-out cross pages.
  - Verified: synthetic real-mouse drag of one tile and of a two-tile selection both reached
    `DoDragDrop` and returned Copy when dropped on our own drop target; seeded six items
    showed "1 / 2" → "2 / 2" (folder on page 2) and wrapped back with the wheel, while the
    wheel over the pill still switched Shelf → Timer. Folder/paging unit tests added
    (43/43 pass).
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

- **Real data + empty states (2026-11-03)**:
  - **All placeholder/demo data removed**: no Priya, Deewana Kar Raha Hai, Invoice.pdf,
    Screenshot.png, TechHub.zip, Study session, demo times, or demo volume anywhere in the shipping
    binary. The renderer starts at `ActivityId::Idle`; the prototype panel is opt-in via
    `--prototype` only and contains no demo content.
  - **Honest empty states**: pinned Music shows "Nothing playing" (flyout: "Nothing playing right
    now"), Call shows "No call", Shelf shows "Shelf empty", Timer shows "No timer". Verified on the
    release build: idle pill 280×62 px, all four empty pills captured, empty Music flyout captured.
    Idle (Auto with no events) does not create the flyout at all, matching the lazy-flyout RAM design.
  - **Manual switcher**: scroll wheel over the pill cycles Auto → Music → Call → Shelf → Timer;
    right-click → Show mirrors the same order. A pin holds until a new automatic winner changes
    (incoming call, timer ending, …) or the pinned activity ends; an open flyout closes instantly on
    switch. Five `resolve_pin` unit tests cover the hold/clear rules.
  - **Real volume**: `activities/volume.rs` registers an `IAudioEndpointVolumeCallback` on the default
    render endpoint via WASAPI and posts `WM_APP_VOLUME = WM_APP + 5`; the 224 DIP HUD shows the real
    level. Verified live at 0/2/4 % changes. No keyboard hook.
  - **Real media**: GSMTC worker initializes COM (`CoInitializeEx(MTA)`) on both the worker and the
    WinRT completion thread, holds `CurrentSessionChanged`, `PlaybackInfoChanged`, and
    `MediaPropertiesChanged` handlers in a `SessionHandlers` struct, and keeps every manager/session
    query on the COM-initialized thread; callbacks only enqueue `Refresh`/`SessionChanged`. Real
    title/artist/playback/timeline verified with a WinRT `MediaPlayer` and a 300 s WAV: log
    `publish title="Taskbar Island Test Track" playing=true pos=29 dur=300`; pill 280 px, flyout
    `0:29` / `-4:31`, prev/next via `TrySkipPreviousAsync`/`TrySkipNextAsync`, click toggles.
  - **Autostart + installer**: `settings.rs` supports the HKCU Run key and the right-click
    **Start with Windows** toggle; `tools/build.ps1 Installer` invokes `installer/installer.iss`
    (Inno Setup 6). See the 2026-11-06 entry at the end for the shipped installer.
  - **Fixed compact width**: `anchor::COMPACT_WIDTH_DIP = 224.0` (280 px at 125 % DPI). The pill
    never moves or resizes when activities change; the flyout still opens 12 px above the taskbar,
    centered on the pill. Verified rect `(1224,1145)-(1504,1207)` for every activity.
  - **Final measurements (release, 125 % DPI)**: fresh idle **0 ms CPU / 10 s**, WS **58.8 MB**,
    private **47.7 MB**, 13 threads, 312 handles. **22/22 tests**, `clippy --all-targets` clean.
  - **Remaining gaps**: real Explorer drag-drop still needs a human (DoDragDrop cannot be
    scripted); Call has no real integration yet and always renders "No call"; compact icons are
    text placeholders (not demo data).

- **Pill fit, wheel switcher, and z-order guard** (2026-11-05):
  - **Pill inside the taskbar**: `PILL_INSET = 4.0` DIP top and bottom; the pill is
    `taskbar_height - 8` DIP tall (50 DIP = 62 px at 125 %), width unchanged at 224 DIP, so it is
    fully on-screen with equal margins and matches the hit box of a taskbar icon. The old
    bottom overhang is gone.
  - **Wheel switcher restored** on the live window path (`Surface::cycle_activity` + `refresh`):
    Auto → Music → Call → Shelf → Timer and back. A pin holds until another activity becomes the
    automatic winner or the pinned activity ends; an open flyout starts closing immediately on a
    switch. Empty states are back ("Nothing playing", "No call", "Shelf empty"/idle, "No timer").
  - **Z-order guard**: `ZORDER_TIMER_ID` (2 s) walks `GW_HWNDPREV` and re-raises the island to
    `HWND_TOPMOST` only when `Shell_TrayWnd` is actually above it, so it never climbs over the
    Start menu or other topmost flyouts. Also reasserted on `WM_SETTINGCHANGE`/`WM_THEMECHANGED`.
  - **Auto re-anchor**: `sync_anchor` recomputes `Anchor::compute` on the same 2 s timer and on
    `WM_SETTINGCHANGE`/`WM_THEMECHANGED`, then moves the island window (position only) when the tray's
    left edge moves, keeping the 12 DIP gap beside the hidden-menu chevron. Size changes are skipped
    because the composition swapchain has no runtime resize. Verified by shifting `TrayNotifyWnd`
    ±48 px — the island followed exactly and returned on restore.
  - Verified on the debug build (1920×1200, 125 %): wheel notches log
    `activity.show: Some(Music|Call|Shelf|Timer|None)` with matching shots; raising Shell_TrayWnd
    above the island (z 14 vs 12) is corrected by the guard within 2 s (z 9 vs 13). Exiting
    TranslucentTB to the opaque taskbar and relaunching it both leave the island visible.
  - **Pill radius**: default `Theme::radius_dip` is now 4 DIP (registry `Radius`, was 10 → 6 → 4), matching
    the corner rounding of Windows taskbar elements and the flyout's own tiles (6 DIP). Geometry and
    the DWM window region use the same value, so no square halo appears.


- **Activity identity icons** (2026-11-05):
  - The four SVGs in the repo root (`call.svg`, `music.svg`, `shelf.svg`, `timer.svg`) are rendered
    natively by D2D — no runtime SVG file parsing. `icons::AppIcon` (Music/Call/Shelf/Timer) holds
    each icon's gradient stops (SVG start 8,4 → end 56,60), 25 % corner radius, top sheen and white
    glyph layers as path data in the SVG's own 64-unit space; `icons::build_app` bakes the call icon's
    `translate(16 16) scale(1.333)` transform into its geometry.
  - `Painter::draw_app_icon` draws gradient rounded-square → sheen → glyph (fill layers and stroked
    layers, round caps) in one transform, cached per target: path geometries, gradient brush and
    sheen brush are reused and only their opacity changes per frame.
  - Used by the compact pill tile for all four activities (replacing the flat accent tile + glyph;
    the volume HUD keeps its flat speaker tile) and by the flyout's 56 DIP leading tile. Music still
    shows real album art when the session provides it; the music icon is the fallback. `Leading::Ring`
    and `Leading::Initial` were removed — the timer/call leading tiles now use their icons.
  - The sheen is filled as a *rounded* rect (same 16-unit radius as the tile). A square sheen rect
    painted white corners outside the tile's top curve — the "strange squared box" artifact.
  - SVG arc centres follow F.6.5 (`sign = +1` when large-arc ≠ sweep). The sign was inverted at
    first, which mirrored every arc's bulge and flung the call glyph to x −36..31 (white blob over
    the tile). `icons::tests::app_glyph_bounds` now asserts every glyph layer stays inside its
    64-unit tile.
  - Verified on the release build at 125 %: each pill state (Shelf/Call/Timer/Music) and the music
    flyout leading tile render the SVG gradients and glyphs; 40/40 tests, release build clean.

- **Live music progress + pill line alignment** (2026-11-06):
  - GSMTC publishes on events only, so a playing track froze the card at its last position. The
    surface now runs a music tick while `music_animating()` holds (main is Music, snapshot playing,
    duration > 0): **80 ms** with the music card open, **1000 ms** with just the resting pill, 0
    otherwise. `advance_music(now)` advances `music_phase` by `WAVE_LEN / 1.2` per second
    (frame-rate independent, dt clamped to 0.5 s) and calls `refresh(now)`; the card reads
    `MediaSnapshot::effective_position(now)`, so elapsed, remaining, thumb and wave all move.
  - `ProgressVisual { fraction, phase, amplitude }` carries the phase to `render.rs`; paused music
    keeps the wave flat (`WAVE_FLAT`). `sync_tick_timer()` is re-run from `apply_media`,
    `clear_media`, `cycle_activity`, `open_flyout` and `finish_close_flyout` so the cadence follows
    the card state.
  - Compact music pill progress line spans the words: starts at the first glyph of the title
    (`content_x`) and ends at the last glyph of the cluster — the value's right edge when a value
    exists, else the measured label width. The timer pill follows the same rule (label "Timer" /
    custom name on the left, clock value right-aligned, so the line ends at `content_right`).
    Other activities keep the full-width line.
  - A long artist is capped at 60 % of the content width and ellipsized, so it can never run under
    the leading icon and hide the title.
  - Verified live: elapsed 1:37→1:42 and 1:18→1:23, remaining −1:49→−1:44, wave/thumb advancing.
    CPU (release): resting with music playing **0 ms / 3 s**; card open **250 ms / 3 s** at the
    80 ms cadence (was 266 ms at 50 ms). 43/43 tests.
  - Timer verified the same way (60 s countdown, pill pinned with the wheel): value 0:37→0:32 over
    5 s, fill shrinking in step, and the line starting under the first "T" of "Timer" and ending
    under the last digit of the clock. The timer card's own timeline and trailing seconds tick at
    1 Hz (the countdown path never needed a new timer).

- **Pill wave + text lift + one-bar pause glyph** (2026-11-06):
  - The compact music and timer pills draw their progress line as the same travelling wave as the
    cards (`WaveVisual { phase, amplitude }` in `RenderInput`, `PILL_WAVE_AMP 1.6`,
    `PILL_WAVE_STROKE 1.6`). The bar baseline sits exactly on the icon tile's bottom edge, so the
    crests stay beside the icon and the troughs use the free band below it; the line still spans
    the words (first letter of the label to the last glyph of the value).
  - Music/timer pill text rises `PILL_TEXT_DY = -2` DIP only while its wave is present; pills
    without a wave keep the centred baseline.
  - `wave_phase` travels whenever `wave_animating()` (music playing, or a countdown running on
    screen). The resting pill now ticks at **80 ms** instead of 1000 ms so the wave moves smoothly;
    paused/finished states are flat and kill the timer again (0 % CPU).
  - Measured (release, resting pill, music playing): **172 ms CPU / 3 s** (~5.7 % of one core,
    versus 0 ms when paused) and 78.9 MB WS. 44/44 tests.
  - **Pause icon showed one bar**: the SVG path lexer peeked for the next command before matching
    the current one, so a `z` that was followed by more path data was skipped. The figure stayed
    open, and D2D does not fill open (hollow) figures — Pause lost its first bar and
    Previous/Next lost theirs. `Z` is now handled before the peek; `pause_glyph_keeps_both_bars`
    guards it. Verified both card pause buttons and the timer card's pause control at 4×.

- **Album cover in the compact music pill** (2026-11-06):
  - `PillView` gained `art: Option<Arc<AlbumArt>>`; `show_music()` copies `snapshot.art` into it.
    Every other pill (shelf, call, timer, drag, placeholders) sets `None`.
  - `Painter::draw_art_tile()` now owns the center-cropped, rounded-clipped `DrawBitmap` that
    `Leading::Art` used, so the card (radius `R_TILE_LEAD 8`) and the 24 DIP pill tile (radius
    `R_TILE_PILL 6`) share one path. The bitmap cache is per art id and target, so the pill and
    card reuse a single `ID2D1Bitmap` of the same cover.
  - When the snapshot has no art (or bitmap creation fails) the pill falls back to the existing
    full-color Music app icon; the fallback is unchanged for all other activities.
  - Verified live on the release build: "Agar Tum Mil Jao" shows the same dark temple cover in the
    pill tile and the card leading tile; resting CPU **188 ms / 3 s** and 76.2 MB WS (unchanged
    from the 172 ms before the blit was added). 44/44 tests.
  - **Equal pill padding**: the leading tile sat 4 DIP from the left edge while the value text
    stopped 10 DIP from the right edge, which the full-bleed cover made obvious. The two
    constants collapsed into one `PILL_PAD = 10.0` (also close to the ~12–13 DIP vertical margin
    around the 24 DIP tile): tile left inset = text layout right inset = 10 DIP on every pill.
    `content_x` moves from 36 to 42 DIP; the wave start follows it. 44/44 tests, verified on the
    release build with the cover track playing.

- **Volume HUD removed** (2026-11-06):
  - Windows already shows its own volume OSD, so the island's transient "Volume" pill was redundant.
    `activities/volume.rs` is deleted: no `IAudioEndpointVolumeCallback`, no
    `WM_APP_VOLUME`, no `taskbar-island-volume` worker thread, no 250 ms expiry cadence, no
    WASAPI subscription at startup (idle CPU is unchanged because the old listener was
    event-driven, but the process no longer holds a callback on the render endpoint).
  - Removed with it: `VolumeHud`, `PillKind::Volume`, `TileView::volume()`, the pill's slider/value
    render branch, `VOL_SLIDER_H` / `VOL_VALUE_W` / `VOL_VALUE_GAP` / `VOLUME_TILE_A` /
    `VOLUME_TRACK_A`, `ActivityKind::Volume` and its `state` rank + test, and `enabled.volume` in
    settings (an existing `"volume"` key in settings.json is ignored). `overlay_pill` stays for the
    shelf drag-over pill; `Speaker` / `SpeakerMuted` stay for call output mute; `audio.rs` is now
    call-only.
  - Verified on the release build: three volume-up presses leave the pill untouched (no Volume
    card appears); 41/41 tests (the three HUD tests are gone), release build clean.

- **App icon, tray presence, and per-user installer shipped** (2026-11-06):
  - **Icon + version resources**: `assets/app.ico` / `app.png` (owner-supplied mark) are embedded
    by `build.rs` with MinGW `windres` from `assets/app.rc` (`1 ICON` + VERSIONINFO 0.5.0), so
    Explorer, the Start menu shortcut and Add/Remove Programs all show the real icon and product
    name. The island window class and the notification icon load resource 1 via
    `tray::load_app_icon` (`LoadIconW`). `SetCurrentProcessExplicitAppUserModelID("TaskbarIsland.TaskbarIsland")`
    keeps taskbar identity stable, and `#![windows_subsystem = "windows"]` means launching from the
    Start menu opens no console and no main window (only the pill + hidden tray window).
    `build.ps1` prepends the WinLibs toolchain so `windres` never falls back to legacy MinGW.
  - **Tray icon** (`src/tray.rs`): a hidden 0×0 `WS_POPUP` + `WS_EX_TOOLWINDOW` window owns the
    `Shell_NotifyIconW(NIM_ADD)` entry (app icon, tooltip "TaskbarIsland - right-click for
    options", `WM_APP_TRAY` callback). Left/right click open one native popup menu
    (`CreatePopupMenu`/`TrackPopupMenu`, `TPM_RETURNCMD`): a **Start with Windows** item that reads
    and flips the HKCU Run value live (mirrored into settings.json `runAtStartup`) and **Exit**,
    which posts `WM_CLOSE` to the island. `TaskbarCreated` re-adds the icon after an Explorer
    restart; `WM_DESTROY` removes it. Menus follow the system theme through uxtheme ordinals
    135/136 (`AllowDark`, `FlushMenuThemes`), re-flushed on `WM_SETTINGCHANGE`/`WM_THEMECHANGED`.
    Event-driven: no timers or polling added.
  - **Single instance**: a named mutex `TaskbarIsland.SingleInstance` (plus the existing window
    check) makes a second Start-menu click a no-op. The installer's `AppMutex` uses the same name,
    so Setup detects a running copy.
  - **Installer** (`installer/installer.iss`, Inno Setup 6.7, built by `tools/build.ps1 Installer` →
    `dist/TaskbarIsland-Setup-0.5.0.exe`): per-user with no UAC
    (`{localappdata}\Programs\TaskbarIsland`), Start menu shortcut (`IconLocation` = the exe),
    optional desktop shortcut, and a **Start when I sign in** task checked by default on a fresh
    install (HKCU Run). Modern dynamic wizard with the owner's mark as the large and small images —
    `WizardImageFileDynamicDark` / `WizardSmallImageFileDynamicDark` must be set too, otherwise
    dynamic-dark mode silently swaps in Inno's built-in art. `[Code]` keeps the checkbox and the
    Run value in sync on upgrades (`InitializeWizard` preselects when autostart is on;
    `CurStepChanged` deletes a stale value when the task is unchecked). Uninstall removes files,
    shortcut and Run value (`CurUninstallStepChanged` + `[UninstallDelete]`); `%APPDATA%\TaskbarIsland`
    user data is kept deliberately.
  - **Verified end-to-end on the release build**: fresh interactive install → startup task checked,
    app auto-launched (single process; visible `TaskbarIsland` pill window + hidden
    `TaskbarIslandTray` + IME only), Start-menu shortcut launches it, second launch keeps the same
    PID, NotifyIconSettings registers the installed path, Run value points at the installed exe.
    Silent uninstall (`/VERYSILENT`) removes exe, install dir, shortcut and Run value; silent
    reinstall with `/MERGETASKS=startup` restores all four. 46/46 tests pass.
  - Known limitation: synthetic keyboard input cannot drive the tray popup when the hidden owner
    window cannot become foreground in this automation session; real mouse input works.

- **Transient media loss no longer closes the card** (2026-11-06):
  - GSMTC players (measured: Chrome / YouTube Music) publish a `None` snapshot for roughly
    1.9–2.3 s while skipping or auto-advancing. `WM_APP_MEDIA` used to clear the snapshot
    immediately, so arbitration fell through to Shelf, `draw()` saw a different flyout owner and
    closed the card, then Music returned — the retract/Shelf flicker reported on Next/Previous.
  - `window.rs` now holds the last snapshot on `None` (`media_gone`): one-shot
    `MEDIA_CLEAR_TIMER_ID` clears it after `design::MEDIA_CLEAR_MS` (3 s), and
    `note_media_command` stretches that deadline to `design::MEDIA_COMMAND_HOLD_MS` (6 s) after one

    of our transport clicks because a slow track load can outlast the normal grace. A returning
    snapshot cancels the timer; a real session end still clears when the window passes. Still
    event-driven: one timer per gap, killed in `WM_DESTROY`.
  - Verified on the release build with Chrome: Next gap 2.25 s and Previous gap 1.89 s both
    resolved with no `expired -> clear`, so the card stayed on Music; 46/46 tests pass.
  - Not a bug: shuffle/repeat gray out when the session's `PlaybackInfo.Controls()` reports no
    shuffle/repeat capability, and `IsShuffleActive` / `AutoRepeatMode` come back `null` on this
    PC's Chrome session. A player that exposes the capability (e.g. Spotify desktop) shows both
    bright and clickable; GSMTC offers no fallback for players that omit it.

- **The open card owns the wheel** (2026-11-06): while the flyout is open, `wheel()`
  (window.rs) no longer falls through to `cycle_activity`, which retracted the card and pinned a
  different activity mid-scroll. Only the shelf card still scrolls, and only when it has another
  page; every other wheel over an open card is ignored (accumulator zeroed). Verified: wheel over
  the open music card logs nothing and never switches; after closing the card the same wheel
  still pins the next activity. 46/46 tests pass.

- **v0.9 spec sections 2-7 COMPLETE on branch `v0.9`** (2026-11-15), shipped as
  `0.9.0`:
  - **§2 clipboard file badge**: `CF_HDROP` is classified by content, so a copied
    image file lands in *All* and *Images* with a thumbnail and the `FILE` badge
    instead of being filed as text (`810bc5d`).
  - **§3 call sources**: one call-source state machine behind four honest sources
    with a `--diag-call` log (`%LOCALAPPDATA%\TaskbarIsland\diag\call-*.log`,
    phone numbers masked); Answer/Decline/End go through UIA or an honest
    fallback (`385d379`). **Not proven on a real call** — owner must run one.
  - **§4 output picker**: music chip 105 x 36 at the end of the controls row and
    call chip 150 x 44 between mute and End open a sheet that slides up *inside*
    the fixed flyout (240 ms `EASE_CONTENT`, `rgb(38,38,38)`, header + 3 visible
    44 DIP rows, extra rows wheel-scroll, flyout never resizes).
    `IPolicyConfig::SetDefaultEndpoint` sets all three roles (call also sets the
    comms default), failure opens `ms-settings:sound`; Phone transfers through a
    UIA invoke (`Phone` / `Transfer to phone` / `Transfer call`) with an honest
    "Open Phone Link to transfer" fallback; `IMMNotificationClient` refreshes the
    list on device/default changes — **no polling**. Row choice: device rows keep
    the sheet open and move the check, Phone closes and transfers, "Cast to a
    device…" opens `ms-settings-connectabledevices:devicediscovery`.
  - **§5 music two-line text**: title (16 px semibold ellipsis) over
    `artist <12 px gap> app` (78 % / 52 % white), the app resolved from the
    session's `SourceAppUserModelId`.
  - **§6 timer stepper**: presets gone; −/big MM:SS/+ (1..180, default 15,
    hold-to-repeat 420 ms then 110 ms) with a full-width amber Start, and a
    running view that shares one `TIMER_BODY_H` so the shell never resizes.
  - **§7 flyout motion**: the card morphs out of the pill rect over **460 ms**
    on `cubic-bezier(.2,1.1,.3,1)`, leaves over **260 ms** on
    `cubic-bezier(.4,0,.2,1)`, and its content holds **160 ms** then fades in
    over **280 ms**. The card is laid out at its final size the whole time —
    only a scale/translate of the surface moves — so nothing inside re-flows,
    and `update_window_region` + `WM_NCHITTEST` follow the *painted* rect, so
    the un-painted area around a half-open card never swallows a click.
    Closing on outside click / Esc / 700 ms pointer-leave is unchanged.
    Wave per brief: wavelength 20, amplitude 3, stroke 3 round cap, **700 ms per
    wavelength**, track 3 px at white 22 %, thumb 4 x 18 white; music and timer
    wave rows grew 14 → 20 DIP so the thumb fits its row. Shell sizes stay at
    design.rs (400 x 192 flyout, 56 rail, 224 pill).
  - **Gates**: `cargo test` **325 passed / 0 failed / 1 ignored**, clippy 0,
    fmt 0, `tools\build.ps1 -Mode Installer` green with the manifest gate, and
    `tools\tests\install-smoke.ps1` **8/8 PASS** on the freshly installed
    release. 60-minute soak CSV in `target\soak\`.
  - **Not tested** (also in `docs/testing.md`): a real Phone Link call, real
    device plug/unplug churn, the DPI 100/125/150 light+dark re-run, clean-VM
    install, the SignPath flow, Explorer drag-out and human-eye checks of the
    new motion/sheet/wave. (CI's first tag run, pending when this was written,
    has since gone green — see the v0.10.0 entry below.)
- **v0.10.0 sections 1-2 COMPLETE on branch `v0.10`** (2026-11-16), shipped as
  `0.10.0`:
  - **§1 slim rail**: the side rail narrows to 48 design px (38.4 DIP) - five
    28.8 DIP r6 buttons, 14.4 DIP glyphs, 6.4 DIP gaps, a 2.4 x 14.4 accent bar
    and a white 8 % divider - and the content column widens to 346.6 DIP
    (shelf tiles 78.65, convert options 62.92, music chip 122.6). Tokens and
    `design-tokens.json` follow.
  - **§2 call card**: three equal rows (header / connected box / action row)
    fill the fixed 178 DIP column - device name over its meta line with signal
    bars (14.4 x 12.8) and the in-box green Output chip (which opens the 0.9.0
    output sheet), then three equal Hold / End call / Mute buttons; incoming
    calls get Decline / Answer. Hold is honest: disabled with a reason until a
    400 ms probe has seen Phone Link's own button for *this* call, amber while
    held, dropped the moment the call ends.
  - **Gates**: `cargo test` **326 passed / 0 failed / 1 ignored**, clippy 0,
    fmt 0, `tools\build.ps1 -Mode Installer` green with the manifest gate, and
    `tools\tests\install-smoke.ps1` **8/8 PASS** on the freshly installed
    0.10.0 release. `tools\tests\ui-smoke.ps1` re-captured all 13 card shots
    (`target\soak\ui-v0.10\`).
  - **CI**: tag `v0.10.0` pushed → workflow `release` run **43755768572**
    `success` (tag↔`Cargo.toml` check, tests, installer, hash, draft release
    with 4 assets); the draft was hash- and version-verified, then published
    with these notes. The `v0.9.0` tag run (43745736845) was also green, so
    CI's first tag run is no longer untested.
  - **Not tested** (also in `docs/testing.md`): a real call and live
    Hold/Resume, live-call card states, the DPI re-run, clean-VM install, the
    SignPath flow, Explorer drag-out, human-eye checks, and a 60-minute soak
    re-run (the v0.9 soak stands; the 0.10 delta is layout-only).
- **v0.11.0 sections 1-8 COMPLETE on branch `v0.11.0`** (2026-11-19), shipped as
  `0.11.0`:
  - **§1 one shared card grid**: `CARD_PAD = 8` DIP gives every card the same
    content rect (434 x 222 at the reference size, minus the 48 px rail); no
    card-private outer margin; `--debug-layout` draws the single cyan rect
    plus each card's used bounds and a unit test fails on drift. Idle/empty
    states keep the layout, dimmed. Shelf and Clipboard were re-checked
    against the grid, not rebuilt (§5).
  - **§2-§4 cards replaced**: music (album glow, 96 px art, 12 px artist/app
    gap, wavy bar + times, centred controls with white play, output button top
    right, dimmed "Nothing playing"), timer (64 px stepper, sync-locked
    5/15/30 presets, full-width Start, running view with four equal buttons),
    call (header/box/buttons in every state, three exactly equal
    Hold/End/Mute, Output inside the box, honest UIA-confirmed Hold/Mute).
  - **§6 output sheet** restyled onto the grid (240 ms slide inside the fixed
    flyout, 36 px header, 44/52 px rows); behavior unchanged from 0.9.0.
  - **§7 Stage A shipped: the pill is a `WS_CHILD` of `Shell_TrayWnd`.**
    Spike first (`tools\spike-embed\`, verdict in `RESULTS.md`): `HWND_TOP`
    above the XAML bridge heals once and holds; the old fullscreen-hiding
    logic for the pill is gone; click-through via the pill's own rounded
    region; re-embed on `TaskbarCreated` (verified live after killing
    explorer: new taskbar handle, placement still correct). Flyout stays a
    separate popup.
  - **§8 taskbar placement**: `TaskbarAl` watched via `RegNotifyChangeKeyValue`;
    UIA icon-group/tray scans debounced 150 ms; centered → pill 12 DIP from the
    screen edge in the empty left band, left-aligned → 8 DIP before the tray
    (flush right + one-time squeeze notice if the band is too narrow); 200 ms
    ease-out slides; geometry only in `anchor.rs`. Auto is the only mode (the
    spec's `Island position` menu was dropped per the owner). **Alignment
    debounce split**: a `TaskbarAl` flip gets a one-shot 900 ms wait
    (`ALIGN_DEBOUNCE_MS`) because Windows slides the icons ~550 ms — the old
    150 ms scan read the half-way group and Auto mis-picked the tray band;
    structure events never shorten a running alignment wait.
  - **Owner decisions**: screen-edge spacing = card edge spacing = 12 DIP
    (`EDGE_MARGIN_DIP`); music content aligns to the shared cyan rect (the
    private `MUSIC_INNER` box is deleted); final `TaskbarAl = 0` (left); the
    design PDFs' idle runner animation is **out of scope for 0.11.0** (it
    never existed in code — artboards only — and needs its own y-bump).
  - **Owner bug fixes (2026-11-21/22)**: (1) *auto-hide reveal delay* —
    a sync while the bar was tucked baked the hidden parent-relative offset,
    so on reveal the child rode up 58 px and stayed clipped until the 2 s
    guard tick. Fixed with a `SetWinEventHook` on
    `LOCATIONCHANGE|SHOW|HIDE` filtered to `Shell_TrayWnd` →
    `WM_APP_TASKBAR_MOVED` → re-anchor + `sync_pill_child` (which now also
    runs at the end of every `sync_anchor` visible path), and startup gates
    on `anchor::is_allowed()` so a hidden bar at launch no longer blocks the
    island. Hook proven live (fired on Explorer restart; alignment held
    across 124 samples + both restarts) — **a natural hide→reveal cannot be
    forced in this environment** (3 methods failed) and needs an owner-machine
    check. (2) *performance* — the per-window-open UIA subtree dump is gated
    behind `looks_like_toast`, and the registry watcher only reacts to a real
    `TaskbarAl` flip (sibling writes under `Explorer\Advanced` are ignored).
    Measured: window churn 625 ms/9 s with **zero** UIA timeouts (old log had
    them regularly), sibling write → 0 log lines/0 slides, each flip → exactly
    1 debounced slide both ways, idle 16–47 ms/10 s, card-open 0 ms/10 s,
    music-playing 656 ms/10 s (the 80 ms wave tick), 0 ms after stop.
  - **Gates**: `cargo test` **338 passed / 0 failed / 1 ignored**, clippy 0,
    fmt 0, `tools\build.ps1 -Mode Installer` green with the manifest gate, and
    `tools\tests\install-smoke.ps1` **8/8 PASS** on the installed 0.11.0.
    Live: idle CPU **0 ms / 10 s** (installed release), WS 82.3 MB / private
    56.2 MB; both alignments pixel-measured (15..294 / 1229..1508 with the
    8 DIP tray gap); zero spurious squeeze notices across a 1→0→1 flip cycle;
    Start open/close leaves the pill at 1229..1508 throughout (no hide path
    exists - Stage A gave the pill none). `tools\tests\ui-smoke.ps1` was
    adapted to Stage A (`FindPill` + `ScreenRects` union of the pill child's
    region and the card host's card+halo region; `FindByClass` alone resolved
    to the host's empty region) and then passed **13 shots** in
    `target\soak\ui-v0.11\`.
  - **CI + release (2026-11-22)**: branch pushed, tag `v0.11.0` pushed at
    `6007b3b` → workflow `release` run **45012540083** `success` (tag↔`Cargo.toml`,
    tests, installer, hash, draft with 4 assets). The draft was verified
    (`target\verify-draft.ps1`: setup hash = published `.sha256`, manifest
    version + payload hash match, both VERSIONINFOs 0.11.0, `NotSigned`) and
    published; `releases/latest` resolves to v0.11.0 with all four assets.
    The published bytes were then installed on this machine and verified
    (hash match, pill+tray windows, single instance, pill = `WS_CHILD` of
    `Shell_TrayWnd`). Gotcha: `GET /releases/tags/{tag}` 404s for drafts and
    the local `gh` keyring token cannot see drafts — use the list endpoint
    with the git credential token.
  - **Not tested** (also in `docs/testing.md`): everything under the v0.9 and
    v0.10 "not tested" lists (real call, device churn, clean-VM, SignPath,
    Explorer drag-out, human-eye card checks), the DPI 100/150 % + light
    re-run, a natural auto-hide hide→reveal cycle for the bug-1 fix (this
    environment cannot hide the taskbar), and a 60-minute soak re-run.
- **v0.12.0 sections 1-7 COMPLETE on branch `v0.12.0`** (2026-11-28), shipped
  as `0.12.0` (a `y` bump; the version lives only in `Cargo.toml`):
  - **§1 the island is part of the taskbar**: it hides/shows with the bar with
    no blink, and a shell flyout taking foreground closes the card — event
    hooks only (a `SetWinEventHook` foreground hook posts
    `WM_APP_FOREGROUND` = `WM_APP+55`).
  - **§2 top/bottom edges**: the edge comes from
    `SHAppBarMessage(ABM_GETTASKBARPOS)`'s `uEdge` with a rect fallback
    (`appbar_edge(u_edge, rc)` — the return value is a status code, not the
    edge), re-read on LOCATIONCHANGE / `WM_SETTINGCHANGE` / `TaskbarCreated` /
    display-DPI change behind a **150 ms debounce**
    (`EDGE_DEBOUNCE_TIMER_ID` 22, `design::EDGE_DEBOUNCE_MS`); bottom keeps
    today's behavior, top hangs the flyout 12 px below the bar's bottom, real
    thickness, and the `Island position` setting is gone (Auto only, owner).
    Flyout stays **400x192** (owner rejected the brief's 500x240).
  - **§3 left/right edges**: the island pauses (pill + card hidden, clipboard
    capture and timers keep running), raises **one notice card per edge
    change** (`notice_decision(notice_edge, anchor_paused, anchor_edge)` is
    pure and unit-tested; copy = title `Island needs a top or bottom taskbar`,
    edge-aware body, button `Open taskbar settings` → `ms-settings:taskbar`,
    plus `Quit island`; Esc/outside dismiss keeps it paused), shows a paused
    row in the tray menu, fires a `Timer finished` tray notification while
    paused, and auto-resumes on a top/bottom edge. Primary monitor only;
    `--simulate-edge top|bottom|left|right` delivers to the running instance
    via `WM_APP_EDGE_SIMULATE` (`WM_APP+58`).
  - **§4 coexistence**: `src/coexist.rs` detects TranslucentTB / Windhawk /
    ExplorerPatcher / StartAllBack / TaskbarX / RoundedTB and enters safe
    mode (separate pill) under them, with a `Taskbar mode` setting
    (Auto / Always embedded / Always separate) persisted in `settings.json`
    and applied through `WM_APP_PILL_MODE` (`WM_APP+57` = `0x8039`);
    `embed_allowed()` (window.rs) maps Separate→false, Embedded→true (safe
    mode is overridden), Auto→`!coexist::active()`, with a sticky per-process
    `EMBED_REFUSED` that a `0x8039` post clears. `TaskbarCreated` re-attaches
    and an embed failure falls back to a separate pill by design. Evidence in
    `docs/compat.md`.
  - **§7 call card None state**: the dimmed Hold/End/Mute row is gone — the
    None layout is a header row (60 DIP green `#4ADE80` 18 % tile, 28 DIP
    phone glyph, `No active call` 18 semibold) over a bottom box (radius 6,
    white 6 %, `Phone Link` 14 semibold + `Open` 132x44 green 16 %); `Phone
    Link calls show up here` removed. Live/incoming/dialing, the pill, the
    progress line and every other string are untouched. **Owner flags**: the
    None header carries no sub/status line and its two rows float centred in
    the 176 DIP column; `Ended` deliberately keeps the old layout; the box
    fill stays `theme.card_button()`.
  - **Gates**: `cargo test` **353 passed / 0 failed / 1 ignored**, clippy 0,
    fmt 0, `tools\build.ps1 -Mode Installer` green, and
    `tools\tests\install-smoke.ps1` **8/8 PASS** on the installed 0.12.0.
  - **Acceptance**: `tools\tests\edge-smoke.ps1` (new, phases 1-6) run 4 =
    **44 passed / 1 failed / 4 skipped** with §1/§2/§3/§5/§6 green except the
    Explorer-restart phase; evidence in `target\soak\edge\` (log, `mat-*.png`
    TTB matrix, fullscreen, notice). The 1 failure is the environmental
    taskbar wedge: one shell-flyout cycle makes `EnumWindows` miss
    `Shell_TrayWnd` while `FindWindowW` still finds it, so `SetParent` fails
    with error 87 and the embed is refused (the island then runs separate by
    §4 design; geometry is unaffected because the island looks the bar up with
    `FindWindowW`). `probe-phase3.ps1` proved: a Start-menu cycle wedges it, a
    TTB bounce does not heal it, an Explorer restart does; the script now
    detects the signature and restarts Explorer before the embed attempts.
    Also investigated post-run: a temporarily unpainted pill window (correct
    geometry/region/z-order, flat pixels) healed on hover+click and never
    reproduced on a clean restart with zero input — a stale present in the
    wedged environment, not an app bug (`pill-fresh-start.png`).
  - **Not tested** (also in `docs/testing.md`): everything under the v0.9-v0.11
    "not tested" lists, the DPI matrix, five of six coexistence targets never
    seen live (only TranslucentTB is installed here), primary-monitor-only on
    real second-monitor hardware, a 60-minute soak re-run, and one unexplained
    `cargo test` STATUS_ACCESS_VIOLATION (nine runs since were green).

## Version naming (x.y.z)

Current version: `0.11.0` → `x = 0`, `y = 11`, `z = 0`.

| Part | Meaning | When to bump |
|------|---------|--------------|
| `x` (major) | A **huge overhaul upgrade** — the whole app / all cards reworked at once. | Only when *every* card is at its peak **and I (the owner) approve** moving to the next major. The first public release will be `1.0.0` (not before). |
| `y` (minor) | An **upgrade on a single feature** plus small incremental upgrades that come with it — e.g. one card gets a complete overhaul or a large new capability. | Bump `y` (and reset `z` to 0) when a card or feature is materially upgraded/overhauled. |
| `z` (patch) | **Minor bug fixes** and tiny changes. | Bump `z` for small bug fixes, minor tweaks, and small changes that do not overhaul a card. |

Rules:
- Never hardcode the version or the product name in multiple places. Both live in `Cargo.toml` (`version = "x.y.z"`, `[package.metadata] product-name = "..."`) and nowhere else: `build.rs` renders them into the Windows VERSIONINFO, `tools\build.ps1` passes them to Inno Setup (`/DMyAppVersion`, `/DMyAppName`), and CI refuses a tag that disagrees. `version_tests` in `src/main.rs` fails the suite if a literal copy reappears in `assets\app.rc`, `installer\*.iss` or `tools\build.ps1`.
- Before `1.0.0` we stay at `x = 0`. `1.0.0` is the first public release, gated on all cards being perfect and owner approval.
- Example flow from here: small bug fix → `0.10.1`; a card overhaul → `0.11.0`; owner-approved public launch → `1.0.0`.
