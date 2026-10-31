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

## Status (updated 2026-10-31)
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
