# Coexistence with other taskbar tools

**Applies to v0.12.0** (`src/coexist.rs`, `Taskbar mode` in the tray menu).

Six tools customize `Shell_TrayWnd` in some way: **TranslucentTB**, **Windhawk**,
**ExplorerPatcher**, **StartAllBack**, **TaskbarX** and **RoundedTB**. Since v0.11 the
pill is a `WS_CHILD` re-parented into the taskbar, which is exactly the arrangement that
has produced the worst failure we have ever seen from this app:

* **Event Viewer 1000** — *faulting module: `explorer.exe`, unhandled exception in the
  taskbar's own window procedure* (`Shell_TrayWnd`'s wndproc), and
* **Event Viewer 1002** — *the application crashed through the Windows Error Reporting
  pipeline*, with a **WER CrashDump** on disk.

Both trace back to handing Explorer a child window while another tool has already
re-styled or subclassed the bar. So v0.12 does not attempt to embed in that
configuration: it falls back to the **separate** top-level pill (the arrangement every
release before 0.11 shipped, where the card window draws the pill itself and nothing is
parented into the taskbar).

## `Taskbar mode`

Right-click the tray icon → **Taskbar mode**:

| Mode | Behavior |
| --- | --- |
| **Auto** (default) | Embed when it is safe to. A detected taskbar tool — or a refused embed — drops the pill out of the bar and draws it separately. |
| **Always embedded** | Always `WS_CHILD`, even with a tool present. This is the owner's explicit override of the safety net. |
| **Always separate** | Never embed; the card window draws the pill. |

The choice is remembered in `%APPDATA%\TaskbarIsland\settings.json` under `taskbarMode`
(`auto` / `embedded` / `separate`; an unknown or absent value loads as `auto`). It takes
effect immediately — no restart — and applies again on the next launch.

Only **Auto** reacts to a detected tool. `Always embedded` embeds regardless (verified
live: `pill.embed: hwnd=… taskbar=…` with TranslucentTB running), `Always separate`
never does.

## Detection

Detection is event-driven end to end — no polling loop anywhere:

| When | How |
| --- | --- |
| **At launch** (before the first embed decision) | One `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` pass matches running processes; one `TH32CS_SNAPMODULE` pass over `explorer.exe` catches the two tools that are *not* processes of their own — ExplorerPatcher and StartAllBack live as DLLs Explorer has loaded (matched on the module path fragment `\explorerpatcher\` / `\startallback\`, so the install location does not matter). |
| **After launch** | A worker thread blocks in `IWbemServices::ExecNotificationQuery` on `SELECT * FROM __InstanceCreationEvent WITHIN 1 WHERE TargetInstance ISA 'Win32_Process'` and only wakes for a real process creation. The thread sleeps in `Next(WBEM_INFINITE)`; it never spins. |
| **On `TaskbarCreated`** (Explorer restarted) | The whole scan runs again, because a restarted Explorer loads whatever is installed into it afresh. |

Process-name matching is lower-cased on both sides, so `TranslucentTB.exe`,
`translucenttb.exe` and a mixed-case rename all match. The eight executables are
`translucenttb`, `windhawk`, `windhawk_svc`, `taskbarx`, `taskbarxconfigurator`,
`roundedtb`, `startallbackcfg`, `startallbacktray` (each `.exe`).

Log markers (with `TBI_LOG=1`): `coexist.scan: <Tool> is running`,
`coexist: safe mode under <Tool>`, `coexist.watch: listening for process creation`,
`coexist.watch: <Tool> started`, `pill.mode: <mode>`, `pill.embed: released`.

### Two WMI requirements (load-bearing)

Both cost real debugging time and are recorded in `src/coexist.rs` so they are not
rediscovered:

1. **`CoInitializeSecurity` must run first, in `main`.** Without an explicit call, COM
   falls back to implicit defaults that register no authentication service, and the WMI
   service then answers *every* read with `WBEM_E_ACCESS_DENIED` — including a plain
   `GetObject` with no query involved. A `CoSetProxyBlanket` on our proxy cannot repair
   it. `main` therefore does `CoInitializeEx(STA)` + `CoInitializeSecurity` (auth level
   `RPC_C_AUTHN_LEVEL_DEFAULT`, impersonation `RPC_C_IMP_LEVEL_IMPERSONATE`) before any
   other COM work; the UI thread's later `OleInitialize` is happy with a thread that is
   already STA.
2. **The notification query needs `WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY`
   together.** Either bit alone, or the synchronous `0`, is rejected with
   `WBEM_E_INVALID_PARAMETER` (`0x80041008`). `ExecQuery` is not a substitute: it opens
   an event query happily and then fails the first `Next` with `WBEM_E_INVALID_QUERY`.

## What safe mode does (and does not do)

* **The pill separates.** In `Auto`, the pill child is released
  (`pill.embed: released`) and the card window draws the pill itself — a top-level
  window with no parent, so nothing of ours is handed to Explorer.
* **One notice, once per process.** A tray balloon explains it:

  > TaskbarIsland — \<Tool\> is customizing the taskbar, so the island is showing a
  > separate pill instead of embedding in the bar. Right-click the tray icon > Taskbar
  > mode to change.

  It fires on the `0 → found` transition only, however many tools appear, and never
  re-raises while the process lives. `Always separate` shows no notice (nothing to
  explain) and `Always embedded` is not in safe mode.
* **Detection does not un-detect.** If the tool exits after being seen, the session stays
  in safe mode until the next `TaskbarCreated` re-scan (or a `Taskbar mode` re-choice,
  which resets the refused-embed latch). This is deliberate: a tool that starts, stops
  and restarts would otherwise bounce the pill between embedded and separate, and every
  bounce re-hands Explorer a child window — the exact sequence behind the Event Viewer
  1000/1002 reports.
* **No effect on anything else.** The card, the call card, timers, clipboard capture,
  the media worker and the tray all run exactly as in safe mode; only the pill's
  parenting changes.
* **A refused embed also separates.** If `SetParent`/`SetWindowPos` fails for any other
  reason (the pre-0.11 fallback path), `EMBED_REFUSED` latches for the session and the
  pill stays separate until the mode is re-chosen.

## Look test: materials × pill modes

The brief's §4 look test. TranslucentTB's three materials, each in both pill modes, in
light and dark taskbar themes — the pill must be visible, undistorted and never
double-drawn in every cell:

| TranslucentTB material | Auto (→ separate) | Always embedded |
| --- | --- | --- |
| Clear | ☐ light / ☐ dark | ☐ light / ☐ dark |
| Blur | ☐ light / ☐ dark | ☐ light / ☐ dark |
| Acrylic | ☐ light / ☐ dark | ☐ light / ☐ dark |

Plus, in each configuration: 20 × open/close the card with zero blinks, and 20 clicks on
each shell element (Start, search, task view, tray chevron, clock) with zero blinks.

## Crash evidence to keep watching

* **Event Viewer 1000/1002** for `explorer.exe` (`Application` log): any new occurrence
  while the island runs is a regression against this design — record the faulting
  module and the pill mode in use.
* **WER CrashDumps** (`%LOCALAPPDATA%\CrashDumps\`): a fresh
  `explorer.exe.*.dmp` alongside an island session is the same signal.
* Both were seen only in the embedded-under-a-tool configuration; v0.12's premise is
  that refusing to embed removes the cause. The look test above is the evidence.

## Known limitations

* **The module pass needs access to Explorer's modules.** Explorer can run at a higher
  integrity level than the island; if the `TH32CS_SNAPMODULE` snapshot is refused the
  in-process tools (ExplorerPatcher, StartAllBack) are missed at launch. Process-based
  detection is unaffected, and `TaskbarCreated` retries.
* **No "tool exited" event.** As above — safe mode sticks for the process lifetime.
* **StartAllBack's tray process is the marker** (`startallbacktray.exe` /
  `startallbackcfg.exe`); the DLL fragment `\startallback\` also covers Explorer's load.
* **Only the six listed tools are detected.** A tool we do not know about embeds
  nothing on our side, but can still produce its own conflicts — file an issue with the
  exe name and it is one line in `TOOLS`.
