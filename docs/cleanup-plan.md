# Repo cleanup plan (v0.9, branch `chore/cleanup`)

Inventory taken 2026-11-12 **before** any move or delete. Every top-level entry of the
repository (`D:\dynamic\taskbar-island`) and of the workspace root (`D:\dynamic`, itself a
git repository with **zero commits**, so nothing there is tracked) is listed with its size,
its git status and a decision: **Keep**, **Move** or **Delete**, with a one-line reason.

Safety rules applied, in order:

1. Work happens on `chore/cleanup` in separate commits: **moves first, then deletes**.
2. Never deleted: source code, tests and the fixtures they use, `LICENSE` and third-party
   notices, installer scripts, CI/GitHub config, user-data folders.
3. Ambiguous files go to `_to_delete/` at the repo root, are listed here and are asked
   about — never guessed.
4. Git history is kept; tracked files move with `git mv`. History is **not** rewritten.

## 0. Sizes before

| Scope | Bytes | Human |
| --- | ---: | --- |
| Repo working tree (no `.git`, no `target/`) | 3,085,672 | 2.94 MB |
| Repo `.git/` | 141,469,753 | 134.92 MB |
| Repo `target/` | 11,214,211,785 | 10.44 GB |
| **Repo total** | **11,358,767,210** | **10.58 GB** |
| Workspace root outside `taskbar-island/` | 4,682,471,000 | 4.36 GB |

`target/` is 97 % of the repository. The `.git` directory is large because **history
contains committed binaries** (engine installers, PDBs, an old `dist/` Setup.exe — the
repo audit estimates ~40–60 MiB of the 134.92 MB). Per the ground rules these are
**reported, not purged**; history is never rewritten without asking the owner.

## 1. Inventory — repository top level

| Path | Size | Git | Decision | Reason |
| --- | ---: | --- | --- | --- |
| `.git/` | 134.92 MB | — | Keep | history; never rewrite without asking |
| `.github/` | 1.4 KB | tracked | Keep | PR + issue templates (repo config) |
| `.gitattributes` | 577 B | tracked | Keep | CRLF policy for the repo |
| `.gitignore` | 36 B | tracked | Keep (edited) | cleanup patterns added in the delete commit |
| `assets/` | 451 KB | tracked | Keep | app icon, wizard art, `app.rc` |
| `changelog.d/` | 13 KB | tracked | Keep | release-note fragments for `CHANGELOG.md` |
| `coordination/` | 49 KB | tracked | **Move** → `docs/archive/coordination/` | superseded two-agent coordination records |
| `docs/` | 925 KB | tracked | Keep + restructure | gains `design/`, `prompts/`, `testing.md`; `audit/`, `release/`, `spikes/` stay |
| `installer/` | 9 KB | tracked | Keep + absorb | all `*.iss` installer scripts move here |
| `scripts/` | 54 KB | tracked | **Move** | `*.iss` → `installer/`, the rest → `tools/` (target tree) |
| `spikes/` | 678 B | tracked | **Move** → `docs/archive/spikes/` | experiment source kept for reference (source code is never deleted) |
| `src/` | 1.46 MB | tracked | Keep | product source |
| `target/` | 10.44 GB | untracked | **Delete** | build output; rebuildable (`cargo build --release` gate re-runs after cleanup) |
| `AGENTS.md` | 36 KB | tracked | Keep | agent rules for this repo |
| `build.rs` | 2.7 KB | tracked | Keep | cargo build script (Windows resource) |
| `call.svg`, `music.svg`, `shelf.svg`, `timer.svg` | 4.1 KB | tracked | **Move** → `docs/design/icons/` | design references; the glyphs are baked into `src/icons.rs` as constants |
| `Cargo.lock` | 16 KB | tracked | Keep | locked dependencies |
| `Cargo.toml` | 2.7 KB | tracked | Keep | package manifest |
| `CHANGELOG.md` | 7 KB | tracked | Keep | release history |
| `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `PRIVACY.md`, `SECURITY.md`, `TERMS.md`, `NOTICE` | 6.6 KB | tracked | Keep | governance / license notices (never delete) |
| `design-tokens.json` | 8.4 KB | tracked | **Move** → `docs/design/design-tokens.json` | tokens belong to the design spec; `include_str!` in `src/tokens.rs` updated in the same commit |
| `LICENSE` | 35 KB | tracked | Keep | GPL-3.0-or-later (owner-approved switch) |
| `script.py` | 0 B | **untracked** | **Delete** | empty AI scratch file |
| `test_output.txt` | 0 B | **untracked** | **Delete** | empty scratch output |
| `THIRD_PARTY_NOTICES.md` | 3 KB | tracked | **Move** → `THIRD_PARTY_LICENSES.md` | target-tree name; expanded in §1 of the v0.9 brief (every bundled engine + license) |
| `README.md` | — | — | **Create** | target tree; SmartScreen warning and usage (v0.9 brief §1) |
| `tests/` | — | — | **Create** | integration tests + fixtures (v0.9 brief §3) |
| `tools/` | — | — | **Create** | dev scripts: soak, dpi smoke, diag helpers, build scripts (from `scripts/`) |
| `_to_delete/` | — | — | **Create (transient)** | ambiguous items awaiting the owner's decision |

## 2. Inventory — workspace root `D:\dynamic` (untracked; the root repo has no commits)

| Path | Size | Decision | Reason |
| --- | ---: | --- | --- |
| `.git/` | 27 KB | Keep | workspace repository metadata (no commits; harmless) |
| `.recon/` | 1.79 MB | **Delete** | design-recon scratch (board renders + splice script), superseded by the design PDFs |
| `docs/audit/repo-audit.md` | 1.5 KB | **Delete** | stub superseded by the full `docs/audit/repo-audit.md` in the repo |
| `taskbar-island/` | 10.58 GB | Keep | the repository itself |
| `tbi-dist/TaskbarIsland-Setup-0.6.0.exe` | 3.5 MB | **Delete** | old duplicate installer (v0.6.0) |
| `ti-agent-a/` | 429 KB | **Delete** (`git worktree remove`) | stale agent worktree; its work is merged in PR #2; only untracked duplicate file present |
| `ti-agent-b/` | 2.89 GB | **Delete** (`git worktree remove`) | stale worktree parked at tag `v0.8.0`, `git status` clean |
| `ti-release-v08/` | 1.63 GB | **Delete** (`git worktree remove`) | stale release worktree parked at tag `v0.8.0`, `git status` clean |
| `build_script.py`, `license_script.py`, `modscript.py`, `script.py`, `tmp_audit.py` | 880 B | **Delete** | broken AI scratch scripts (`license_script.py` is what gutted `src/` on 2026-11-11) |
| `build-*`, `clippy-*`, `final-test-*`, `rv8-*`, `test-*` `*.txt`/`*.out` | ~130 KB | **Delete** | old build/test logs; the passing results are quoted into `docs/testing.md` first |
| `cargo-winlibs.cmd` | 235 B | **Move** → `tools/cargo-winlibs.cmd` | working toolchain shim: puts the WinLibs `mingw64\bin` first on `PATH` (the installed `C:\MinGW\bin\gcc.exe` is 64-bit-incapable and breaks `libwebp-sys`) |
| `design-tokens.json` | 8.2 KB | **Delete** | stale duplicate of the repo's tracked token file (both are `0.8.0-design`) |
| `design-tokens_v0.9.0.json` | 9.8 KB | **Move** → `docs/design/design-tokens-v0.9.0.json` | current v0.9 token set (source for the v0.9 design work) |
| `inno-setup.exe`, `isdl.html` | 23 KB | **Delete** | failed download — both are HTML pages, not installers |
| `innosetup-6.7.4.exe` | 10.6 MB | **Delete** | unused tool download; `tools/build.ps1` requires an *installed* Inno Setup 6 and never references this file |
| `install.log` | 0 B | **Delete** | empty log |
| `repo-audit.md` | 630 B | **Delete** | stub superseded by `docs/audit/repo-audit.md` |
| `Taskbar Island Design v0.9.0.pdf` | 2.7 MB | **Move** → `docs/design/` | current v0.9 artboards |
| `TaskbarIsland Card v0.5.pdf`, `v0.6.pdf` | 1.0 MB | **Move** → `docs/archive/` | superseded card designs |
| `TaskbarIsland-Clipboard-v0.7-Prompt.md` | 10 KB | **Move** → `docs/archive/` | superseded prompt |
| `TaskbarIsland-v0.8-AgentA-Codex-Prompt.md` | 15 KB | **Move** → `docs/archive/` | superseded prompt |
| `TaskbarIsland-v0.8-AgentB-MiMo-Prompt.md` | 15 KB | **Move** → `docs/archive/` | superseded prompt |
| `TaskbarIsland-v0.8-Design-System.md` | 12 KB | **Move** → `docs/archive/` | older design system, superseded |
| `TaskbarIsland-v0.9-Design-System-Addendum.md` | 6 KB | **Move** → `docs/design/` | v0.9 design addendum; where it conflicts with the current prompt in `docs/prompts/`, the prompt wins |
| `TaskbarIsland-v0.9.0-Prompt.md` | 13 KB | **Move** → `docs/archive/` | superseded by the current v0.9 prompt |
| `ti-backup.bundle` | 139.7 MB | **Move** → `_to_delete/` + **ask owner** | git bundle backup — backup or scratch? ambiguous, not guessed |

## 3. Target tree (after cleanup)

```
/  src/                 Rust source (app, anchor.rs, ui, music, call, clipboard, shelf, timer, audio, win)
   tests/               integration tests and fixtures
   assets/              icons, sparse package manifest
   installer/           core installer and optional converters-pack scripts
   tools/               dev scripts (soak, dpi smoke, diag helpers, build/release)
   docs/
      design/           the current design spec (v0.9) and tokens
      prompts/          the current prompt only
      testing.md        what is tested, what is skipped and why
      cleanup-plan.md   this file
      archive/          old prompts and the older design system, kept for reference
   THIRD_PARTY_LICENSES.md
   README.md
   LICENSE
   Cargo.toml
   .gitignore
```

Not listed but kept because the build, the release process or the tests need them:
`Cargo.lock`, `build.rs`, `AGENTS.md`, `CHANGELOG.md`, `changelog.d/`, `NOTICE`,
governance docs (`CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `PRIVACY.md`, `SECURITY.md`,
`TERMS.md`), `.github/`, `.gitattributes`, `design-tokens` (moved under `docs/design/`),
`docs/CONTRACTS.md`, `docs/engines.md`, `docs/convert-options.md`, `docs/audit/`,
`docs/release/`, `docs/spikes/`.

## 4. Commits on `chore/cleanup`

1. `docs: add cleanup plan (inventory, sizes, decisions)` — this file.
2. `docs: archive superseded prompts/design, add current v0.9 prompt, design and tokens`
   — workspace-root archives into `docs/archive/`, `docs/design/`, `docs/prompts/`.
3. `refactor: restructure scripts/ → tools/ + installer/, tokens, notices rename`
   — `git mv` for every tracked move, references updated (`AGENTS.md`, `*.iss`, `*.ps1`,
   `src/tokens.rs`, `.github` PR template).
4. `chore: delete build output, stale worktrees and scratch files` — `target/`, the three
   stale worktrees (`ti-agent-a`, `ti-agent-b`, `ti-release-v08`), workspace-root scratch,
   logs, duplicate installers, empty repo-root scratch files; `_to_delete/` created for
   `ti-backup.bundle`.
5. `gitignore: cover build output, engines, caches, logs and scratch patterns`.

Gate after the deletes: `cargo build --release` and `cargo test` must both pass on the
cleaned tree (recorded in `docs/testing.md`).

## 5. Ambiguous — owner decision required

| Item | Where it went | Question |
| --- | --- | --- |
| `ti-backup.bundle` (139.7 MB) | `_to_delete/ti-backup.bundle` | Keep as a history backup, or delete? It duplicates `.git` history. |
| Committed binaries in git history (~40–60 MB of the 134.92 MB `.git`) | untouched | Purge with a history rewrite (`git filter-repo`), or leave history alone? |

## 6. Sizes after *(filled in after the delete commit)*

| Scope | Bytes | Human |
| --- | ---: | --- |
| Repo working tree (no `.git`, no `target/`) | _pending_ | |
| Repo `.git/` | _pending_ | |
| Repo `target/` (after the `--release` gate rebuild) | _pending_ | |
| **Repo total** | _pending_ | |
| Workspace root outside `taskbar-island/` | _pending_ | |

## 7. Everything removed *(filled in after the delete commit)*

_pending._
