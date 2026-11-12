# TaskbarIsland — Clipboard activity + 5-icon rail (v0.7)

Add a fifth activity, **Clipboard**, and adapt every existing card to the new rail. The approved design is in the design folder next to you (new artboards: Clip, ClipHover, ClipPinned, ClipConfirm, ClipEmpty, ClipPaused; every Shelf/Convert artboard was updated with the new rail). Match them. If this prompt and an artboard disagree, ask me.

## 0. Hard constraints (unchanged)
- Pill `PILL_W = 224` DIP, height = taskbar band minus 4 DIP top/bottom (anchor.rs:136). Card 400 × 192 DIP (`FLY_W/FLY_H`, design.rs:16), centered on the pill, bottom 6 DIP above the taskbar. All activities share these exact sizes. No scaling on hover. Windows-style corners (card 8, tiles 8, small buttons 6 DIP).
- Fully offline. No network, no telemetry. Clipboard data never leaves the machine.
- Icon-first UI: no label text on buttons; names come from the Windows-style tooltip system built in v0.6 (450 ms delay, instant when moving between neighbors, popup window, accessible name = tooltip text). Reuse it; do not build a second one.
- Keep the constants-guard test and add the new constants below.

## 1. Rail adaptation (all cards)
Five activities: Music, Timer, Call, Shelf, Clipboard (in that order, top to bottom). The 56 DIP rail (+1 px divider) is 190 DIP tall, so the old 40 DIP tiles no longer fit:
- Rail tile 40 → **32 × 32 DIP**, radius 8, gap 6 → **5 DIP**, icon 20 → **18 px** (stroke 1.8). Five tiles = 160 + 4 × 5 = 180 DIP, centered vertically.
- Active indicator: 3 × **16** DIP bar, radius 2, at the rail's left edge (8 DIP from the tile top), in the activity accent. Active tile bg `rgba(255,255,255,.10)`.
- Accents: Music `#F0617F`, Timer `#FFB547`, Call `#5BE38F`, Shelf `#8FA6D6`, **Clipboard `#5FD0C5`**.
- Clipboard icon: stroke clipboard (`rect x8 y3 w8 h4 rx1` + body path `M16 5h2a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2`).
- Apply to EVERY card (Music, Timer, Call, Shelf, Convert views, Converting, Done). Switching by scroll on the pill keeps working and now cycles five activities. Add the Clipboard pill state too: the pill shows the clipboard icon and, for 2 s after a new copy, a small count pop (e.g. `+1`), then returns to its idle look. Pill size unchanged.
- Update the constants-guard test: rail tile 32, gap 5, icon 18, indicator 16, and a layout test that five tiles fit inside the rail's inner height with ≥ 4 DIP spare.

## 2. Clipboard history — behavior (the special rules)
### 2.1 Retention: 7 days
- Every item stores `created_at` (UTC). An item older than **7 days** is deleted automatically.
- Expiry check runs: at app start, on every new copy, and on a 1-hour timer. Also when the card opens. Deleting expired items must be silent and instant; never show a "cleared" message.
- Use a monotonic-safe approach: if the system clock moves backwards, do not delete anything early; if it jumps forward past 7 days, expire normally.
- Items in their last 24 h show their age in amber (`#FFB547`) with tooltip "Expires soon" + muted "Cleared automatically".

### 2.2 Capacity: max 30
- Maximum **30 items** at once. When a 31st item arrives, remove the **oldest unpinned** item (oldest by `created_at`) in the same transaction, so the store never holds more than 30.
- Pinned items are exempt from the 7-day expiry and from oldest-first eviction, but they still count toward the 30. If all 30 are pinned, a new copy is NOT stored (and the pill shows no `+1`); pinning is also refused with the pin button disabled (tooltip "Unpin something first") once 29 are pinned, so there is always one free slot. (This pin behavior is my addition; keep it unless I say remove it.)
- Duplicate copy of the same content as an existing item: do not add a second row; move it to the top and refresh its `created_at` (this resets its 7 days).
- Add constants `CLIP_MAX_ITEMS = 30`, `CLIP_RETENTION_DAYS = 7` to the guard test, and test: 31st item evicts the oldest unpinned; all-pinned case; expiry at 7 d − 1 s (kept) and 7 d + 1 s (removed); duplicates; clock moving backwards.

### 2.3 Capture
- Use `AddClipboardFormatListener` on a message-only window (event-driven, zero polling). Debounce 150 ms; coalesce repeated `WM_CLIPBOARDUPDATE`.
- Supported types: text (CF_UNICODETEXT), images (CF_DIBV5/CF_DIB or "PNG"), files (CF_HDROP), links (text that parses as a URL; show link style). Rich text/HTML: store the plain text only.
- **Privacy rules (non-negotiable):**
  - Skip any copy where the source set `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0`, or `CanUploadToCloudClipboard = 0` (password managers use these). Never store those, never show them.
  - Skip copies made by the app itself (when it writes an item back to the clipboard, ignore the resulting update via a sequence-number check).
  - **Pause capture** toggle (icon in the toolbar): while paused, nothing is recorded. State persists across restarts; the pause icon turns into a play icon with the accent fill.
  - Do not capture when the clipboard text is empty/whitespace, or larger than 1 MB (text) / 20 MP or 25 MB (image) — silently ignore.
- Image items store a PNG copy (not the original DIB) plus a 96 px thumbnail; files items store paths only (never copy file contents), and show the file name.

### 2.4 Storage
- Persist across restarts in `%LOCALAPPDATA%\TaskbarIsland\clipboard\` (SQLite is fine if it is already a dependency; otherwise a small append-safe JSON/binary store with atomic writes). Encrypt at rest with DPAPI (`CryptProtectData`, current-user scope) because clipboard data can hold secrets. Offline, no extra services.
- Image blobs go in a `blobs\` subfolder keyed by content hash; deleting an item deletes its blob in the same operation. On startup, remove orphan blobs.
- Clearing history overwrites nothing fancy, just deletes records + blobs. Pinned items survive "Clear history".
- Corrupt store → start empty, never crash.

## 3. Clipboard card — UI (see artboards)
Same shell as the other cards: rail on the left, content padding 12 DIP (inner 319 × 166), NO text lines other than previews and numbers.
1. **Toolbar, 28 DIP**, left to right:
   - Filter segmented control (2 DIP padding, 28 DIP high, 26 × 24 tabs): All, Text, Images, Links, Pinned. Active tab fill `rgba(95,208,197,.24)`.
   - Count, centered, 11 px muted, tabular numbers: `10 / 30` (Pinned filter shows `3 pinned`). Tooltip: "Expires after 7 days" + muted "Oldest is removed past 30".
   - **Pause/Resume capture** icon button (28 DIP).
   - **Clear history** icon button (28 DIP), tooltip "Clear history" + muted "Pinned items stay".
2. **List, 130 DIP**, vertical scroll (wheel/touch/draggable 4 DIP scrollbar; wheel inside the list must NOT switch activities). Newest first. Rows 32 DIP, gap 2, radius 6, hover `rgba(255,255,255,.07)`, virtualized.
   - Row: type glyph (20 DIP slot: text, code-like text in Cascadia Mono, link in blue `#8FB8FF`, file, or a 24 × 18 image thumbnail), single-line preview (ellipsis), age at the far right (`2m`, `3h`, `2d`; pinned rows show the accent pin before it).
   - On hover/keyboard focus the age is replaced by three 24 DIP icon buttons: **Pin/Unpin**, **Copy**, **Delete**. Their tooltips open to the LEFT of the button so they stay inside the card.
   - Click a row = put it back on the clipboard (and ignore our own update, 2.3). Enter does the same. **Double-click or Shift+Enter = copy back AND paste** into the previously focused app: the flyout must be non-activating (`WS_EX_NOACTIVATE`), so the target window keeps focus; send Ctrl+V with `SendInput` only after the flyout is dismissed and the original foreground window is still the foreground. If it is not, just copy.
   - Delete key removes the focused row; no confirmation for a single delete.
   - Tooltip for a row preview (full text up to ~200 chars, or image dimensions + size) after the usual delay.
3. **Clear history** is two-step inline: click the trash → the trash is replaced by a red-tinted ✓ (tooltip "Clear N items", muted "Pinned items stay") and a ✕ (cancel). Auto-cancels after 4 s. Clears everything except pinned items.
4. **Empty state**: one large muted clipboard icon centered in the list area, count `0 / 30`, no sentences.
5. **Paused state**: the pause button shows as a play icon on an accent fill; the list still works; the pill shows a small pause dot instead of `+1`.
6. Sensitive-content hint: items that look like secrets (long random strings, or text matching common token patterns) are NOT hidden or blocked, but show the preview masked as `••••••••` with reveal on hover. Keep the matcher simple and local. (Optional; do it last and tell me if it makes false positives.)

## 4. Security & robustness
- Treat clipboard content as hostile: never render it as markup, never auto-open links, never execute anything. Text previews are plain text; links open only on an explicit Ctrl+click and only http/https.
- File items: store paths only; if a path no longer exists, show the row dimmed and make Copy a no-op with the tooltip "File moved or deleted".
- Memory: cap in-memory image thumbnails (≤ 30 × 96 px) and decode off the UI thread.
- The clipboard listener must never block the message loop; heavy work (PNG encode, hashing, DPAPI) goes to a worker thread.
- Dependency denylist test stays (no network crates, no HTTP client).

## 5. Tests
1. Retention/capacity/duplicates/pins/clock tests from 2.2.
2. Privacy: formats that opt out of history are never stored; our own writes are not re-captured; paused = nothing stored.
3. Rail layout fits five tiles; guard constants.
4. Clear history keeps pinned items and removes blobs of deleted items.
5. List virtualization with 30 mixed items; wheel in the list doesn't switch activity.
6. Toolbar always shows Pause and Clear in every state (empty, populated, paused, confirm).
7. Copy-back does not trigger a duplicate capture; double-click paste only fires when the original window is foreground.

## 6. Phases (stop and show me after each)
1. Rail adaptation across all existing cards + pill clipboard state + guard tests.
2. Store: capture listener, retention, capacity, dedupe, DPAPI, tests (no UI).
3. Clipboard card UI: list, filters, row actions, tooltips, empty/paused.
4. Pin, Clear confirm, copy-back/paste, secrets masking.

Do not start phase N+1 until I approve phase N. Keep help/usage text out; I will add it later.
