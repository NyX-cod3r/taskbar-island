# Agent B coordination

## Done

- Read `TaskbarIsland-v0.8-Design-System.md` and `design-tokens.json` in full
  (§10 is the working contract).
- Read A's `docs/CONTRACTS.md` (8d63d91) and `coordination/from-A.md`.
- Created the Agent B worktree `D:\ti-agent-b`, branch `agent-b/v0.8.0`, from
  `release/v0.8.0` @ `516a5ec`; `CARGO_TARGET_DIR=C:\build\ti-b`.
- **Engine inventory.** `ENGINES_DIR = C:\Users\satya\AppData\Local\Filee\engines`
  (build.ps1 default). Present: `ffmpeg\` (bin/lib/include/doc/presets),
  `pandoc\pandoc.exe` + MANUAL/COPYING, `tesseract\` (exe + DLLs + doc) — 644 MB,
  280 files, plus an empty `.downloads\`. **Missing: `tessdata\`** (build.ps1
  already reports "0 OCR language(s)"), and none of the four new engines
  (`libreoffice`, `poppler`, `qpdf`, `ghostscript`) exist anywhere on C:, D: or
  E: — searched for `soffice.exe`, `pdftoppm.exe`, `qpdf.exe`, `gswin64c.exe`,
  `*.traineddata`. Final layout will be documented in `docs/engines.md` once the
  folders arrive.

## Next

- B1 non-code prep while blocked (engine inventory above, resolver/probe design,
  options-table design review against §4).
- B1 code (resolver, capability probe, pipeline model, router entries, options
  data table, fake-engine tests, `docs/engines.md`) as soon as the base lands.

## Requests to the other agent

1. **The v0.6/v0.7 base is not committed — please commit it to
   `release/v0.8.0` (owner approved this).** `docs/CONTRACTS.md` says
   "Convert/clipboard/engines: Not present in this baseline; B creates them",
   but that is incorrect: the code exists, it is simply uncommitted in the
   shared worktree `D:\dynamic\taskbar-island` (branch `main`, HEAD `516a5ec`,
   ~11k lines of working-tree changes). What is missing from git:

   - `src/engines/` (mod, exec, probe, router, output, ffmpeg, pandoc,
     tesseract) — my brief *extends* this `Engine` trait/router; recreating it
     would fork the design
   - `src/convert/` (mod, detect, jobs, options)
   - `src/clipboard/` (mod, capture, store, dpapi, secret, write, writer)
   - `src/tooltip.rs`, `src/tray.rs`, `src/uia.rs`, `src/thumb.rs`,
     `src/offline.rs`, `src/activities/shelf` split, `build.rs`, `assets/`,
     `scripts/update.iss`, `scripts/update-config.iss`, `scripts/tests/`,
     `scripts/release.ps1`, `scripts/clean-dist.ps1`, `scripts/engine-includes.ps1`

   My brief forbids me from editing A-owned files and from touching shared
   branches, so I cannot commit it myself without stepping on you. Either you
   commit it from `D:\dynamic\taskbar-island` (selectively — `dist/` and
   `target/` are NOT ignored by the current `.gitignore` and must be excluded),
   or tell me in this file that I should do it and I will.
2. **Design artboards are missing.** No `design` folder and no
   `Shelf*/Convert*/Clip*` artboards exist on this machine (only
   `TaskbarIsland Card v0.5.pdf` / `v0.6.pdf` and `.recon/boards/*.png`).
   B2–B4 UI work and the visual QA (`docs/qa/B/`) need them — please add the
   folder or point me at it.
3. **Tooltip API:** I will need the exact Phase A1 tooltip function names for
   option tiles/row actions (B3/B4). I will consume them as published and keep
   a local shim until then, per §10.7.
4. FYI: my brief's placeholder `ENGINES_DIR = <PATH-I-GIVE-YOU>` was resolved
   by the owner to `%LOCALAPPDATA%\Filee\engines`. The four new engines will be
   placed there by the owner; I do not download anything.

## Status updates (append-only)

- **2026-11-08 19:25** — Checked A's state: `agent-a/v0.8.0` at `14ac00a`
  (tooltip + pill priority, pushed 18:12), worktree clean, no Codex session
  activity since 18:12:32 (session ended `task_complete` after the push
  approval). Read A's session log: at 18:03 A looked for
  `D:\ti-agent-a\coordination\from-B.md` and got `NO_FROM_B` — my branch was
  never fetched, so **request 1 below has not been seen yet**. To make it
  findable without a fetch I copied this file to
  `D:\ti-agent-a\coordination\from-B.md` (untracked in A's worktree; it is
  my file, not one of A's). `origin/agent-b/v0.8.0` also carries it, so
  `git show origin/agent-b/v0.8.0:coordination/from-B.md` works from any
  worktree of this repo. Still waiting on the base commit; no B1 code touched.
- **2026-11-08 19:40** — Non-code B1 deliverable pushed:
  `docs/convert-options.md` — the full §4 options-table spec (FileKind → Tab →
  [Option] with id/icon/tooltip/needs/builder, candidate orders per action
  class, §9 tooltip copy, visibility rules, the 10 B1 tests, open questions:
  missing artboards, `IconId` additions for the two new tabs, Ghostscript
  keep/drop).

- **2026-11-09** — **Base landed and the gate is green.** `release/v0.8.0`:
  `fa33b68` imported the v0.6/v0.7 base (69 files, +25,441/-2,663 — request 1
  resolved) and `393f52c` burned the clippy debt down to zero: `cargo fmt
  --check`, `cargo clippy --all-targets -- -D warnings` (192 -> 0 lints) and
  `cargo test` (186 passed, 1 ignored, 0 failed) all pass; the installer was
  rebuilt from this tree and `scripts/tests/install-smoke.ps1` reports 8/8
  (stop, install, exe, version, matches-target, launch, windows, single).
  **The owner retired Codex: this agent now works both scopes alone** — B-scope
  (Shelf/Convert/Clipboard/engines/installer/notices) and A-scope (foundation,
  Call, pill states, testing/GitHub/installer/release per
  `TaskbarIsland-v0.8-AgentA-Codex-Prompt.md`). A's branch
  (`agent-a/v0.8.0` @ `14ac00a`) is not merged yet; the burn-down kept every
  symbol A's branch uses (`SwitchChip`, `draw_small_button`, `.launch`,
  `add_minute`) alive behind `allow(dead_code)` so the merge stays clean.
  Next: rebase this branch onto `release/v0.8.0`, then B1 code.
- **2026-11-09** — **B1 code complete; gates green; commit + push of
  `agent-b/v0.8.0` with this entry.** Everything in phase B1 landed: four-engine
  resolver + capability probe, `Action` +7 groups, router entries with the
  documented orders and one fallback retry, the full data-driven options table
  (~48 new rows, all eight source kinds, group A/B targets, own-format hiding,
  `tab_strip_hidden`), magic-byte detection for HEIC/HEIF, OLE2 legacy Office,
  the ZIP family, CSV/TSV and a first-class PDF card, the `Step`/`run_pipeline`
  pipeline model (equal-weight progress to 1.0, cancel ⇒ `EngineError::Cancelled`
  with the temp dir dropped by `Workspace` RAII — no deletion call in the
  conversion path), and the exact §9 tooltip copy including all three chains.
  Gates: `cargo fmt --check` 0, `cargo clippy --all-targets -- -D warnings` 0,
  `cargo test` **204 passed / 0 failed / 1 ignored** (denylist + security
  scanners inside). Evidence and the missing-engine list for the owner:
  `coordination/ready-B-1.md`; changelog fragment
  `changelog.d/B-1-convert-foundation.md`. Next: B2 (`docs/CONTRACTS.md`
  adoption of `card_content_rect()` — still needs A's file/artboards) after the
  user-rule installer smoke.
- **2026-11-09** — **B2 code complete; gates green; commit + push of
  `agent-b/v0.8.0` with this entry.** All three cards (Shelf, Convert,
  Clipboard) now lay out inside the shared `card_content_rect()` (329 × 178)
  with the design-token numbers (toolbar/gap 8, shelf tile 66, option grid 106
  / 8 gap / 59.4 columns, clip list 142), the body fills the remaining height
  per design §1 (convert stack top-aligned, option rows split the grid 106 or
  49 DIP, never below the 40 minimum), the §5 single-tab rule is wired
  (`tab_strip_hidden` collapses the strip; the grid takes the space) and the
  Convert header shows the source format as a §7 chip. Layout invariants moved
  to module-level `const _: () = assert!(…)` so they run in every build (also
  clears `clippy::assertions_on_constants`). Gates: `cargo fmt --check` 0,
  `cargo clippy --all-targets -- -D warnings` 0,
  `cargo test` **206 passed / 0 failed / 1 ignored**. Evidence:
  `coordination/ready-B-2.md`; changelog fragment
  `changelog.d/B-2-card-layout.md`. **Owner open question in ready-B-2**: the
  artboard hint copy (`Drop files here`, `Release to convert. Your original
  stays put.`, …) and tile descriptor lines are NOT implemented — the design
  doc §4/§6 supersede them; exact strings listed for override. Next: B3
  (clipboard previews, engine pipeline argv builders, zip extract, batch/merge)
  — needs A's tooltip API for the row/option tooltips.
- **2026-11-10** — **B3 code complete; gates green; commit + push of
   `agent-b/v0.8.0` with this entry.** Clipboard history rows now carry a
   display type re-derived every frame (link / code / color / image / files /
   text — persisted `ClipKind` stays a hint, so older stores classify too),
   with mixed-height virtualization (34 DIP text rows, 50 DIP image rows with a
   64 × 40 checkerboard thumb, strides 36/52, gap 2) driven by
   `clip_strides` + variable `clip_scroll` at every scroll site. The 450 ms
   hover preview overlay landed: a 206 × 150 panel, horizontally centered and
   seated on the toolbar's bottom edge (`y = CLIP_TOOLBAR_H`; the popup height
   equals toolbar gap + list by const assert, so it never covers a toolbar
   control), showing the 192 × 118 picture (natural size / cover-crop, never
   stretched) or capped 200-char text, the `615 × 342 · PNG · 212 KB` caption
   and the `from <app>` line. Thumbnails gained an explicit box —
   `ThumbRequest{path,max}`, 128 px rows/tiles vs 384 px preview, size-salted
   `art_id`, a separate preview cache holding only the live file, decode always
   on the worker. Capture records the clipboard owner
   (`GetClipboardOwner` → process stem → friendly name) as optional
   `sourceApp` in the encrypted store (lenient JSON; older rows unaffected).
   The hovered row no longer shows the generic tooltip (its preview replaces
   it; keyboard focus keeps the row tip). Gates: `cargo fmt --check` 0,
   `cargo clippy --all-targets -- -D warnings` 0, `cargo test`
   **215 passed / 0 failed / 1 ignored**. Evidence:
   `coordination/ready-B-3.md`; changelog fragment
   `changelog.d/B-3-clipboard-rows-preview.md`. Next: B4 (golden engine tests
   — still gated on the owner dropping LibreOffice/Poppler/qpdf/Ghostscript +
   `tessdata` into `ENGINES_DIR` — plus PDF/Office/sheet/present UI), then B5
   (installer engine fragment, `THIRD_PARTY_NOTICES.md`, changelog assembly).
- **2026-11-10** — **B4 code complete; gates green; commit + push of
    `agent-b/v0.8.0` with this entry.** All four new engine runners landed
    (`ghostscript` with `-dSAFER` first and the no-gain contract, `qpdf`
    rotate/linearize/merge, `poppler` text/raster/images/split/merge with
    blank-page scan detection, `libreoffice` with the exact spec argv, a
    private macro/link-free profile and a one-job-at-a-time lock), plus
    `convert/multi.rs` (image/PDF merge, ZIP + extract with zip-slip and
    size/ratio caps, batch maps, native CSV → JSON), the Pandoc → DOCX →
    LibreOffice PDF chain, and collision-safe delivery (`stem (2).ext`,
    staging temps in the destination folder). The Convert card got the
    **hybrid card kind** (all-PDF batches stay on the PDF card, 2+
    convertible files open the Several-files card) and **§5 needs-dimming**
    (40 % opacity, "needs 2+ …" tooltip, clicks ignored on needs tiles).
    §9.2's **golden suite** (`src/convert/golden.rs`, 13 tests, one per §4
    row) runs the real engines on in-test fixtures and asserts the whole
    delivery contract; rows whose engine is missing print
    `SKIP golden <row>: …` and pass — this machine runs 23 conversions
    (FFmpeg/Pandoc/Native) and skips 22 lines naming LibreOffice, Poppler,
    qpdf, Ghostscript and the missing `tessdata`. §9.4 security tests added:
    zip bomb refused before any write, zip-slip + hostile names, single-argv
    hostile names, remote-image HTML under `--sandbox`, GS `-dSAFER`, LO
    profile disables macros/link updates. Gates: `cargo fmt --check` 0,
    `cargo clippy --all-targets -- -D warnings` 0, `cargo test`
    **256 passed / 0 failed / 1 ignored**. Evidence:
    `coordination/ready-B-4.md` (full SKIP list); changelog fragment
    `changelog.d/B-4-engine-jobs-golden.md`. **Owner decisions still open**:
    drop the four engines + `tessdata` into `ENGINES_DIR` (skipped rows then
    run unmodified), and Ghostscript AGPL-3.0 keep/drop. Next: B5
    (installer `engines.<ext>` fragment, `THIRD_PARTY_NOTICES.md`, changelog
    assembly, self-test), then A5 release.

## Blocked

- ~~**B1 code work blocked on request 1** (base commit)~~ — resolved: the base
  landed on `release/v0.8.0` (`fa33b68`), clippy/fmt/tests green (`393f52c`);
  B1 code starts with this rebase.
- Artboards (request 2) block B2 onward, not B1.
- Golden engine tests (`TI_ENGINES_DIR`) will be skipped until LibreOffice,
  Poppler, qpdf, Ghostscript and `tessdata` are present; they report skips, not
  failures, per §9.2.
- Ghostscript AGPL-3.0 keep/drop is an owner decision, still pending (notices
  written in `docs/third-party-notices` work either way).
