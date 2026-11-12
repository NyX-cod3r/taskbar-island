# ready — A3 (Call activity)

Phase: **A3 (prompt §5)** — complete for the NO-GO scope agreed after the
A2 spike. Branch: `agent-b/v0.8.0`.

## What landed

### Slice 1 — tooltip placement (A1 leftover, `5d1da6c`)
- `tooltip::place()` flips to the **left** of the control when the tip would
  clip the right edge, with two placement tests.

### Slice 2 — call pill priority + live clock
- `state::pill_winner()` resolves the pill independently of the open card,
  with tests (`pill_winner_maps_the_ranked_state_to_an_activity`,
  `pill_priority_restores_the_next_live_state`).
- `view::PillOverride` + `window::sync_pill_override()`: `pill()` runs
  overlay > override > main; call/clipboard overrides always interrupt and
  other winners yield to an explicit pin; the pin clause
  `call_live && pin != Call` is gone.
- `refresh()` freezes call-driven card swaps while `flyout_open`, bypassed
  by the one-shot `user_card_switch` (wheel/rail), so an open card is never
  yanked away; `pop_pin` releases a transient "+N" clipboard pin.
- Pill click with an override opens **that** state's card (spec: clicking the
  call pill opens the Call card).
- Call clock: `sync_tick_timer()` runs 1000 ms only while `call_live()`;
  `WM_TIMER` refreshes the view so the pill and card tick mm:ss.
- `draw_pill` takes its accent from `pill.tile.color` (call/clipboard pills
  were painting the wrong activity colour).
- §9 rail tooltips fixed: `Calls` → `Call`, `File shelf` → `Shelf`.

### Slice 3 — honest CallIdle (A prompt §5)
- `call::PhoneLinkState` — `NotInstalled` / `NotSignedIn` / `NotConnected` /
  `Available` — proven **offline**: per-user AppX registration
  (`HKCU\…\AppModel\Repository\Packages\Microsoft.YourPhone_*`) and Phone
  Link's own `LocalCache\DeviceMetadataStorage.json` (`"IsLinked"`). No
  network, no WinRT, no private API; results cached 30 s because the Call
  card is rebuilt on every refresh (music ticks at 80 ms).
- Unavailable → the card is spec §8's empty state: **one 36 DIP muted
  phone icon** (`design::EMPTY_ICON` = `#6A6A6A`), no sentences, no controls,
  no signal bars. The icon's tooltip says which condition applies:
  "Phone Link not installed" / "Phone Link not signed in" /
  "Phone not connected".
- Available (a phone is linked) → the idle card keeps its honest "No active
  call" status, real endpoint mute buttons and an **Open Phone Link** button
  that launches `shell:AppsFolder\Microsoft.YourPhone_8wekyb3d8bbwe!App`
  (AUMID verified against the Start menu entry). `Element::CallOpen` was a
  dead button before; both it and the idle icon now go through
  `open_call_app()`, which logs a static `call.open failed` on failure.
- Idle cards no longer draw signal bars — an idle card has no signal to
  report; only a live call does.

## Honesty notes (what is *not* claimed)
- Windows exposes no "is my phone connected" API, so **"connected" is never
  asserted**. `Available` means "a phone is linked on this PC"; unreadable
  metadata degrades to `Available` rather than a negative guess.
- Sign-in vs. connection is separated by local evidence only (metadata file
  absent ⇒ never set up). A wrong reading would show the muted icon with a
  tooltip, never a healthy-looking card that lies.
- No fake call control ships: no Answer/Decline/End, no device tiles, no
  volume slider, no release-detection UI (A2 spike: detect NO-GO, hide
  NO-GO, actions NO-GO, audio PARTIAL).

## Evidence
- Log audit: no caller/app-name data reaches any `log::log` — every call
  site is a static string, an enum, a file path or an error code; masking
  (`mask_number`) keeps `+91 98•• ••• 421` out of logs.
- Phone Link on this machine: package `Microsoft.YourPhone_1.26092.119.0_x64`
  status Ok, Start-menu AUMID `Microsoft.YourPhone_8wekyb3d8bbwe!App`,
  notification key `…!FileTransfer_POCO F6`, device metadata
  `"IsLinked":true` → this PC reports `Available`.
- New tests (6): package-family recognition, metadata → state derivation,
  `"IsLinked"` spacing tolerance (incl. no read past the document end),
  idle-tip copy for all four states, launch AUMID, and a "state is observed,
  not invented" guard.

## Gates
- `cargo fmt --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo test` — **271 passed / 0 failed / 1 ignored**.

## Open items
- A2 spike re-validation still needs a **real linked phone** before detect /
  hide-popup / actions can move off NO-GO; audio stays PARTIAL.
- Golden rows for LibreOffice / Poppler / qpdf / Ghostscript and the
  tesseract OCR skip stay skipped until the owner drops the payloads into
  `ENGINES_DIR`.
- A4 (`docs/release/v0.8.0/test-report.md`, screenshots, soak) and A5
  (release) follow on this branch.
