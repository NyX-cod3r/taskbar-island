# TaskbarIsland Design System (v0.8)

One source of truth for both agents. Machine-readable twin: `design-tokens.json` (same folder). If this file and the tokens disagree, the tokens win; tell me.
Visual reference: the design canvas ("TaskbarIsland Card v0.8", all artboards in the design folder). If code and artboard disagree, the artboard wins unless the artboard is physically impossible (then ask).

## 1. Fixed (never change)
| Name | Value |
|---|---|
| `PILL_W` | 224 DIP |
| Pill height | taskbar band minus 4 DIP top and bottom (anchor.rs:136) |
| `FLY_W` × `FLY_H` | 400 × 192 DIP (design.rs:16) |
| Card position | centered on the pill, bottom 6 DIP above the taskbar |
| Card corner / border | 8 DIP / 1 px |
Every activity and every state uses exactly these sizes. No scale-on-hover. No new window sizes. Pill states (call, clipboard count, etc.) live inside the same pill.

## 2. Layout of an expanded card
```
┌──────────────────────────── 400 × 192 ────────────────────────────┐
│ rail 56 │  content: padding CARD_PAD = 7  (was 12, -40%)           │
│ 5 tiles │  column, gap CARD_GAP = 8                                │
│ 32×32   │  toolbar/header 28 · tabs 28 · body fills the rest       │
└─────────┴──────────────────────────────────────────────────────────┘
```
- Content rect ≈ 328 × 176 DIP. Always compute it with ONE function, `card_content_rect()`. No card may keep its own padding constant.
- Body areas (grids, lists) use all remaining height (`flex: 1`). No fixed 130 DIP boxes, no dead space.
- Rail: 56 wide incl. 1 px divider; five tiles 32 × 32 (radius 8), gap 5, icon 18 px stroke 1.8, centered vertically. Active tile bg `rgba(255,255,255,.10)`, active bar 3 × 16 DIP in the activity accent at the rail edge.
- Order: Music, Timer, Call, Shelf, Clipboard.

## 3. Color (dark; follows Windows theme, light values are proposals)
Card `#272727`, border `#3C3C3C`, rail `#212121`, divider `#343434`, tile `#303030`/border `#3A3A3A`/hover `#3B3B3B`, segmented track `#1F1F1F`, tooltip `#2C2C2C`/border `#4A4A4A`.
Text white / body `#EAEAEA` / muted `#9A9A9A` / faint `#6A6A6A`. Success `#34D987`, danger `#E5484D`, warning `#FFB547`, link `#8FB8FF`.
Accents: Music `#F0617F`, Timer `#FFB547`, Call `#5BE38F`, Shelf `#8FA6D6`, Clipboard `#5FD0C5`. An activity's accent is used for its rail bar, active tab/segment fill (`accent` at 24–26% alpha), selection ring and primary progress only.
Pill fill `rgba(52,52,52,.94)` over the taskbar, border `#4A4A4A`, radius 10. (The earlier rule stays: pill transparency should feel like the open-app highlight box on the taskbar; tune by eye.)

## 4. Type
Segoe UI Variable Text. Body 12/16, semibold 12/16, card title 13.5–16, small 11/14, tile text 12/14 bold +0.3px tracking. Mono (Cascadia Mono, Consolas) 11.5 for code and hex colors. Tabular numbers for counts, timers and percentages.

## 5. Icons
24 × 24 grid, stroke only, round caps and joins, stroke 1.9 (rail 1.8), `currentColor`. All path data is in `design-tokens.json → icons.paths` (38 icons). Do not draw new ones unless needed; if you add one, add it to the tokens file in your own PR section (see §10) using the same style. Sizes: rail 18, tabs 16, toolbar buttons 16, tile icons 18, device tiles 22, call buttons 24.

## 6. Icon-first rule and tooltips
- No text labels on buttons. Allowed text: file names, previews, numbers (counts, timers, percentages, sizes), format names on option tiles (PNG, DOCX), and one-line status chips.
- Every icon control has a tooltip and a UI Automation `Name` equal to the tooltip text.
- One shared tooltip system (popup window, non-activating, click-through, within the monitor work area). Show after **450 ms** hover or keyboard focus; if another tooltip closed less than **300 ms** ago show instantly. Hide on click, leave, scroll, drag start, Esc. Style: fill `#2C2C2C`, 1 px `#4A4A4A`, radius 4, 12/16 text, padding 4×8, optional second line 11/14 `#A8A8A8`.
- Placement: below by default; above for bottom rows; left of row action buttons; right-aligned tooltips for the last column; never clipped by the card (separate popup window).
- Tooltip copy is exact (see §9) so both agents write identical strings.

## 7. Components (sizes in DIP)
| Component | Spec |
|---|---|
| Icon button | 28 × 28, radius 6, hover `rgba(255,255,255,.08)` |
| Toolbar | 28 high row: [left group] [count/center, 11 px muted] [right buttons] |
| Segmented control | 2 padding, 28 high, tabs 26 × 24 (or flex), track `#1F1F1F`, active fill accent@24% |
| Option tile | min 40 high, grows with free height (about 48 with two rows), radius 8, 5 columns, gap 8, text = format name or an 18 px icon. Disabled = 40% opacity |
| Shelf tile | 4 columns, row 66, gap 6, radius 8, thumbnail or type chip, one-line name, selection ring 2 px accent |
| List row | 34 high (text, link, color, code, file) or **50 high** (image), radius 6, gap 2, hover `rgba(255,255,255,.07)`. Age at far right; on hover the age is replaced by Pin / Copy / Delete (24 × 24) |
| Image thumbnail | 64 × 40, radius 5, 1 px inner border `rgba(255,255,255,.12)`; images smaller than the box show at natural size on a checkerboard |
| Hover preview | overlay inside the card, 206 wide, padding 6, radius 8, large image 192 × 118 + one caption line; shows after 450 ms |
| Scrollbar | 4 wide, thumb `#4A4A4A`, vertical only, wheel inside a scroll area never switches activity |
| Pill call buttons | 34 × 34 circles; Decline `#E5484D` (white glyph), Answer `#34D987` (dark glyph) |
| Call card buttons | Answer/Decline 60 high, flex 1, radius 10; End call 64 × 36 |
| Device tile | flex 1, 46 high, radius 8; active = green border + `rgba(91,227,143,.16)`; Bluetooth badge 10 px top right |
| Volume | 4 high track, green fill, 12 px white thumb, value at right (11 px muted) |
| Chip | radius 8, `#3A3A3A`, 10.5 px, e.g. `+2` |

## 8. States
Every interactive element needs: rest, hover, pressed, keyboard-focus (2 px accent ring, offset 1), disabled (40%), and for lists: selected. Empty states are one large muted icon (36 px, `#6A6A6A`), no sentences. Destructive actions (clear history) use an inline two-step confirm (✓ danger tint + ✕, auto-cancel 4 s). Success shows a green check; failure shows an amber/red icon with a tooltip that says what happened in one line.

## 9. Exact tooltip copy
Rail: Music, Timer, Call, Shelf, Clipboard.
Shelf: Select all · Clear shelf (2nd line "Your files stay where they are") · Convert · Remove selected.
Convert tabs: Convert to · Optimize · Edit · Extract text · PDF tools · Extract · Combine. Close · Cancel.
External-engine second line: `via FFmpeg`, `via Pandoc`, `via Tesseract`, `via LibreOffice`, `via Poppler`, `via qpdf`, `via Ghostscript`; chains: `Tesseract + Pandoc`, `Poppler + Pandoc`, `Tesseract + Poppler`. Native work shows no second line. DOCX from PDF: `via LibreOffice · layout may differ`.
Done: Show in folder · Copy file · Undo (2nd line "Moves the new file to the Recycle Bin").
Clipboard: All · Text · Images · Links · Pinned · Pause capture · Resume capture · Clear history (2nd line "Pinned items stay") · Pin · Unpin · Copy (2nd line "Click the row to copy back") · Delete · count tooltip "Expires after 7 days" (2nd line "Oldest is removed past 30") · "Expires soon" (2nd line "Cleared automatically") · "File moved or deleted" · "Unpin something first".
Call: Answer · Decline · End call · Switch call audio to this (2nd line device + transport) · Current output · Ringing on <device> (2nd line "Phone Link · <phone name>") · Phone Link connected (2nd line "Calls show here and on the pill") · Apply (2nd line "Turns off the banner only") · Windows blocked the change.

## 10. Collaboration contract (two agents, one repo)
Agent A = Codex ("Platform, Call, Release"). Agent B = opencode MiMo ("Files, Convert, Clipboard, Engines"). Each owns files; nobody edits a file they do not own.

| Area | Owner |
|---|---|
| `ui/tokens`, `ui/layout` (`card_content_rect`), `ui/tooltip`, rail, pill rendering and pill state machine, `activities/mod` registry/trait | **A** |
| `activities/call/**`, `activities/music/**`, `activities/timer/**`, `platform/audio/**`, `platform/notifications/**`, `platform/phonelink/**` | **A** |
| Main installer script, CI workflows, versioning, CHANGELOG assembly, GitHub release | **A** |
| `activities/shelf/**`, `activities/convert/**`, `activities/clipboard/**`, `engines/**`, `platform/clipboard/**`, `installer/engines.*` (an include fragment), `THIRD_PARTY_NOTICES.md`, `docs/engines.md` | **B** |
| `design-tokens.json`, this file | A edits; B requests changes |
| Tests | each owner writes tests for their own areas in files they own |

Rules:
1. **Worktrees, not one folder.** `git worktree add ..\ti-agent-a -b agent-a/v0.8.0 origin/release/v0.8.0` and `..\ti-agent-b -b agent-b/v0.8.0 origin/release/v0.8.0`. Each agent works only in its own worktree.
2. **Separate build output:** `CARGO_TARGET_DIR=C:\build\ti-a` for A, `C:\build\ti-b` for B. Separate app data in dev: set `TASKBARISLAND_DATA_DIR` to a per-agent folder.
3. **One app instance at a time.** Before running the real app (it attaches to the taskbar), take the lock: create `%TEMP%\taskbarisland-dev.lock` containing your agent name and the time. If it exists and is younger than 15 minutes, do other work (tests, code) instead. Delete it when done. Prefer automated tests over launching the app.
4. **Branches:** `main` is untouched until release. `release/v0.8.0` is created by A from `main`. Each agent pushes its branch and opens a PR into `release/v0.8.0`; only A merges. Rebase on `release/v0.8.0` at least at the start of every phase. No force-push to shared branches. No history rewriting after a PR is opened.
5. **Communication by files, append-only, one file each:** `coordination/from-A.md` and `coordination/from-B.md` (committed on each agent's own branch, so they never conflict). Sections: *Done*, *Next*, *Requests to the other agent*, *Blocked*. Read the other agent's file at the start of every phase (fetch their branch).
6. **Changelog fragments** instead of editing CHANGELOG.md: add `changelog.d/<agent>-<topic>.md` (one bullet list each). A assembles them at release.
7. **Shared interfaces are frozen after Phase 0.** If you need a change to `tokens`, `layout`, `tooltip` or the activity trait: write it under *Requests* and keep working around it (a local shim inside your own module). A answers within the next phase.
8. **Ready signal:** when a phase is done and gates pass, add `coordination/ready-<agent>-<phase>.md` with the evidence (commands and results, screenshots) and push.
9. Definition of done for any change: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, dependency denylist test (no network crates, no HTTP client, no updater), no `unwrap()` on I/O or external input, no new `unsafe` without a comment explaining the invariant.
10. Never commit engine binaries, installers, user data or secrets. `engines/` binaries are git-ignored; the build reads `ENGINES_DIR`.

## 11. Artboard index (what to build from what)
Shelf: ShelfFiles, ShelfTip, Shelf. Convert: Convert, ConvertOptimize, ConvertEdit, ConvertExtract, ConvertVideo, ConvertAudio, ConvertDoc, ConvertPdf, ConvertPdfTools, ConvertSheet, ConvertMulti, ConvertHover, Converting, Done. Clipboard: Clip, ClipHover, ClipPreview, ClipPinned, ClipConfirm, ClipEmpty, ClipPaused. Call: CallPillIncoming, CallPillActive, CallIncoming, CallActive, CallDeviceTip, CallSetup, CallIdle.
