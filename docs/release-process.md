# Release process

One version = one tag = one CI-built draft release that the owner publishes by
hand. Nothing is published automatically, and nothing is signed until the
SignPath steps in [signing.md](signing.md) are approved and implemented.

Roles: **author** prepares the release, **reviewer** checks it, **approver
(Satyam, owner)** publishes it and approves any signing request. See
`README.md → Code signing policy`.

## 0. Ground rules

- Work happens on the feature branch (`v0.9` for this cycle). **Never push
  `main` directly.**
- The version lives in exactly one place: `Cargo.toml` → `version = "x.y.z"`
  (plus `[package.metadata] product-name`). Nothing else may hardcode it —
  `version_tests` fails the suite if a copy appears in `assets\app.rc`, the
  `.iss` scripts or `tools\build.ps1`.
- Version bumps follow `AGENTS.md → Version naming`: patch for small fixes,
  minor for a card/feature overhaul, `1.0.0` only with owner approval.

## 1. Bump the version

1. Edit `Cargo.toml` only: `version = "0.9.0"`.
2. Nothing else to touch — `build.rs` renders the exe's VERSIONINFO from it,
   `tools\build.ps1` passes it to Inno Setup as `/DMyAppVersion`, and CI
   refuses a tag that disagrees.

## 2. Test (all locally, before tagging)

```powershell
cargo fmt --all
cargo clippy --all-targets -- -D warnings
.\tools\build.ps1 -Mode Test
.\tools\build.ps1 -Mode Installer        # core installer -> dist\TaskbarIsland-Setup.exe
powershell -File tools\tests\install-smoke.ps1
```

`install-smoke.ps1` is the gate: it installs the fresh `dist\TaskbarIsland-Setup.exe`
over the real per-user install, byte-matches it against `target\release`,
launches it and checks the pill/tray windows and single-instance behaviour.
`cargo test` alone is not evidence that a release works.

Optionally also build the engines pack (`-Mode Converters`) — it is a separate,
unsigned download and is not part of this gate.

## 3. Update the docs and merge

- Assemble `changelog.d/*.md` fragments into `CHANGELOG.md` under the new
  version heading.
- Record test evidence (and anything **not** tested) in `docs/testing.md`.
- Update the status log in `AGENTS.md`.
- Get the change reviewed, then merge the feature branch.

## 4. Tag

```powershell
git tag v0.9.0
git push origin v0.9.0
```

The tag must be `v<x.y.z>` and must equal `Cargo.toml`'s version — CI checks
this as its first real step and fails the run otherwise.

## 5. CI builds a draft release

`.github/workflows/release.yml` (triggered by the tag) does, on a clean
`windows-latest` runner:

1. Installs MinGW and Inno Setup, adds the Rust `x86_64-pc-windows-gnu` target.
2. **Fails if the tag ≠ `Cargo.toml` version**, or if the product name is
   missing.
3. Runs `tools\build.ps1 -Mode Test`.
4. Runs `tools\build.ps1 -Mode Installer` (which also enforces the manifest
   gate on the built exe).
5. Writes `TaskbarIsland-Setup.exe.sha256`, builds the self-update payload and
   `manifest.txt`.
6. Creates a **draft** GitHub release with:
   `TaskbarIsland-Setup.exe`, `TaskbarIsland-Setup.exe.sha256`,
   `taskbar-island-<ver>.exe`, `manifest.txt`.

CI stops there. It holds no signing key and cannot publish.

> **Never re-run a step in a way that replaces an asset already on the
> draft** — after signing, the uploaded setup is not the one CI first built.
> `tools\release.ps1` follows the same rule: it fills in missing assets and
> leaves what is already there alone.

## 6. Owner reviews the draft

- Download `TaskbarIsland-Setup.exe` from the draft and check it against the
  published SHA-256.
- Check the release notes and the changelog entry.

## 7. Signing — `[AFTER APPROVAL]`, see [signing.md](signing.md)

Only after SignPath has accepted the project and TODO-1…TODO-5 are done:

1. CI submits a **signing request** for the exe and then for the installer,
   each carrying the build artifact and the tag as its description.
2. The request sits at *Pending* — **the approver (Satyam) approves it
   manually** in the SignPath portal. CI waits; it cannot approve its own
   request.
3. CI downloads the signed bytes, recomputes the SHA-256 (signing changes the
   file) and updates the draft's assets.
4. Verify: `Get-AuthenticodeSignature` is `Valid`, a timestamp is present, and
   a clean Windows 11 VM installs it with no SmartScreen prompt.

Until this section is implemented the release is published **unsigned**, and
`README.md` explains the SmartScreen warning instead of claiming a signature.

## 8. Owner publishes (manual, always)

Open the draft → **Publish release**. This is the only step that makes the
release public, and it is the owner's alone.

Publishing also switches on self-update: `installer\update.iss` follows
`releases/latest/download/manifest.txt`, which now points at the payload and
hash uploaded with the release.

## 9. After publishing

- Confirm `https://github.com/NyX-cod3r/taskbar-island/releases/latest`
  resolves and the four assets are there.
- Install the published build once on a clean machine/VM and launch it.
- If the release was signed, replace the README placeholder with the SignPath
  Foundation line (TODO-7).

## Rollback

A bad release is corrected by fixing forward: bump the patch version, repeat
this checklist, publish the new version. Never move or re-point an existing
tag — self-update compares versions numerically and a re-used tag would leave
installs on the wrong bytes.
