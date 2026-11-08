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

## Blocked

- **B1 code work blocked on request 1** (base commit). Everything else in B1
  that does not require `src/engines` proceeds.
- Artboards (request 2) block B2 onward, not B1.
- Golden engine tests (`TI_ENGINES_DIR`) will be skipped until LibreOffice,
  Poppler, qpdf, Ghostscript and `tessdata` are present; they report skips, not
  failures, per §9.2.
