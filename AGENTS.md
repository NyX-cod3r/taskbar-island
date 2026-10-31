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
