# Spike: embed the pill in the Win11 taskbar (v0.11.0 §7)

**Question.** Is a plain window embedded in the Win11 taskbar (`Shell_TrayWnd`) as a
`WS_CHILD` stable on this machine — visible, above the XAML bridge
(`Windows.UI.Composition.DesktopWindowContentBridge`), surviving Start / tray flyouts, and
re-embedded after `explorer.exe` restarts? This decides §7 "the pill never hides":
**Stage A** (ship the embed) or the **fallback** (keep the current top-level `WS_POPUP`
+ `WS_EX_TOPMOST` + 2 s z-guard).

**Verdict: Stage A is feasible and ships.** Baseline, Start ×5 and all four tray flyouts
pass; re-embedding after an Explorer restart is effectively instant; the one real failure
in the first full run was a self-defeating z-heal method, fixed and verified in run 3.

## How the spike works

- Raw Win32 FFI, zero dependencies (`--offline` build), independent of the app's
  `windows` crate feature set — the spike must not be able to fail for app reasons.
- A GDI window (240 x 34, flat green `rgb(11,255,12)` so a pixel sample proves
  visibility) is created as a `WS_POPUP` + `WS_EX_NOACTIVATE`, then:
  `SetParent(h, Shell_TrayWnd)` → `GWL_STYLE = WS_CHILD | WS_VISIBLE`,
  `GWL_EXSTYLE = 0` → `SetWindowPos(HWND_TOP, SWP_SHOWWINDOW | SWP_FRAMECHANGED |
  SWP_NOACTIVATE)` → `InvalidateRect` + `UpdateWindow` (force the first paint).
  Placed at x = 8, vertically centred in the taskbar client (taskbar h = 60 → y = 13).
- A `WS_CHILD` never receives `HWND_BROADCAST`, so a **hidden top-level relay window**
  owns the `TaskbarCreated` registered message, the 100 ms observability poll and the
  120 s self-terminate timer. `drive.ps1` has a `Stop-Process` backstop.
- `drive.ps1` walks the scenarios and writes `DRIVER` markers into the same log:
  1. baseline (5 s) — 2. Start open/close x5 — 3. tray clock / Win+A / Win+B / Ctrl+Esc
  x3 each — 4. auto-hide on/off (registry + Explorer restart) — 5. Explorer restart
  (re-embed latency) — 6. fullscreen (only when safe; otherwise NOT TESTED).
- Every line is `[+<ms>][epoch=<unix ms>] …` so driver actions correlate with spike
  events. `STATE` lines sample visibility, rect, z-order (`zok`), the centre pixel and
  the foreground window (heartbeat-logged on change).

**`zok` ground truth** = no `DesktopWindowContentBridge` sibling is above the child
(bridge present → not above it; bridge absent → child is top of the child list).
`zok=0` means the taskbar's XAML layer paints over us — the pill would be hidden.

## Results

| Run | Log | Coverage | Outcome |
|---|---|---|---|
| smoke | `log-smoke.txt` | embed only | `parented z='HWND_TOP (above bridge first try)'`, `visibleAfter=1`, stable 120 s, exit 0 |
| fresh shell | `log-freshshell.txt` | embed only | same clean first-try embed (taskbar `0x70398`, bridge `0xb0128`) |
| baseline | `log.txt` | embed only | clean embed, 120 s stable, exit 0 |
| full run 1 | `log-fullrun1.txt` | 1–6 | sc1–3 **PASS (18/18 checks)**, sc4 **FAIL**, sc5 **FAIL**, `failures=2`, `blink-count=6`, re-embed 544 ms external / 0 ms internal, z-heal `insert-after-bridge` fired **x334 and never stuck** |
| run 3 | `log-run3-sc123.txt` | 1–4 (log ends mid-sc4) | sc1 **PASS**, sc2 **5/5 PASS**, sc3 **12/12 PASS**; sc4 re-embed OK, z-heal `hwnd-top` **x1 and stuck**; sc5/6 and the summary never ran (log truncated) |

### 1. Embedding works (scenarios 1–3, both full runs)

Baseline, Start open/close x5, and tray clock / Win+A / Win+B / Ctrl+Esc x3 each:
`vis=1 zok=1 green=True` on every check — the child sits above the XAML bridge, the
centre pixel is our own green (nothing paints over it), the foreground changes
(Start, Search, Quick settings, Task switcher) never disturb it. 120 s runs exit 0
with no crash and no leaked windows.

### 2. Re-embed after `explorer.exe` restart works (scenarios 4–5)

When Explorer dies, `Shell_TrayWnd` dies and takes the child with it (parent death
destroys children — the blink pairs). The spike re-creates and re-embeds on
`TaskbarCreated`, with the 100 ms poll as fallback:

- `re-parented why=poll-detect latency=0ms taskbarWait=0ms` (x3 in full run 1) —
  detection-to-embed is **0 ms** once the taskbar exists; the external
  **544 ms** (Stop-Process → visible again) is dominated by Explorer's own restart.
- `blink-count=6` = 3 destroy/recreate pairs across the three Explorer restarts in
  full run 1 — expected, not flicker: each pair is one restart.
- Note: on this machine Explorer broadcasts `TaskbarCreated` ~2.9 s **after**
  `Shell_TrayWnd` exists, so the broadcast alone is a late signal; the spike's poll was
  the fast path. (The app already receives `TaskbarCreated` on its hidden tray window
  and by broadcast time the taskbar exists, so the app can use the event directly.)

### 3. The one real failure: `insert-after-bridge` z-heal is self-defeating

`hWndInsertAfter` names the window that ends up **above** the moved window, so
"insert after the bridge" can only ever land the child directly **below** the bridge.
Full run 1, after the auto-hide registry restart: `zok=0 green=False`
(px `28,28,28` = taskbar/bridge paint over us), and the heal fired **334 times over
82 s without ever sticking** → scenario 4 `RESULT=FAIL`, scenario 5 `at3s RESULT=FAIL`.

Run 3 changed the heal to: attempt insert-after-bridge once (logged purely to prove the
semantics — `ok1=true after1=[DesktopWindowContentBridge]`), then when the bridge is
still above, `SetWindowPos(HWND_TOP)` — top of `Shell_TrayWnd`'s child list:

```
event z-heal #1 method=hwnd-top ok1=true after1=[…DesktopWindowContentBridge] ok2=true after2=[]
```

`after2=[]` = nothing above us; `zok=1` then held for the rest of the log (10 s,
through Start/Search foreground churn) with **exactly one heal** versus 334.
**`hwnd-top` is the method that ships.**

### 4. Auto-hide follows the parent

With auto-hide on, the taskbar slides off-screen and the child goes with it — no code
needed, being a child *is* the correct behaviour. The failure in run 1 was not the
slide itself but the z-order after the second (auto-hide-off) restart, i.e. finding 3.

### 5. Not tested

- **Scenario 6 (fullscreen)** — `drive.ps1` only runs it when safe; offline runs
  skipped it. Untested.
- Run 3's log ends mid-scenario-4 (no `RESULT` for sc4–6, no summary), so the
  auto-hide-**off** step that failed in run 1 was not re-executed end-to-end; it is
  covered by the heal-method fix and the verified `hwnd-top` stickiness, not by a
  re-run. Untested as an exact step.
- A real multi-monitor / DPI-change re-embed, and a long (> 120 s, spike cap) soak.

## What ships (Stage A)

- The **pill becomes a `WS_CHILD` of `Shell_TrayWnd`** (spike sequence above), so the
  taskbar's XAML bridge can no longer paint over it and auto-hide carries it with the
  taskbar. The card/flyout stays a top-level popup (a child is clipped to the taskbar,
  and the card opens above it).
- **z-heal: `hwnd-top`** — when a bridge sibling is above the pill, raise to the top of
  the taskbar's child list; verify with an immediate `GW_HWNDPREV` re-walk.
- **Re-embed on `TaskbarCreated`** (received by the existing hidden tray window / the
  card window — both top-level), re-creating the pill child and re-registering the drop
  target; no sub-second polling.
- The current top-level `WS_POPUP` + `WS_EX_TOPMOST` + 2 s `HWND_TOPMOST` guard remains
  only for the **card** window while it is open.

The fallback (keep the pill top-level) is **not** shipped: it keeps the z-fight the
spike exists to remove, and the embed passed every scenario that ran.
